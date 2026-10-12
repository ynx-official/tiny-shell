//! Application-owned authorization and task execution. The bridge never approves work.
use crate::{
    contracts::{Call, Failure, Result},
    ipc::Handler,
    policy::{Decision, Policy, decide},
    storage::Store,
    tasks::State,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub group: Option<String>,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(skip)]
    pub digest: String,
}
#[async_trait]
pub trait Remote: Send + Sync {
    fn app_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    async fn targets(&self) -> Result<Vec<Target>>;
    async fn prepare(&self, _target: &Target, _session: &str, _call: &Call) -> Result<Value> {
        Ok(json!({}))
    }
    async fn execute(
        &self,
        target: &Target,
        session: &str,
        call: &Call,
        cancel: watch::Receiver<bool>,
    ) -> Result<Value>;
    async fn close(&self, session: &str);
}
#[derive(Clone, Serialize)]
pub struct Review {
    pub id: String,
    pub client: String,
    pub target: Target,
    pub call: Call,
    pub state: State,
    pub error: Option<Failure>,
    pub preview: Value,
}
struct Task {
    review: Review,
    session: String,
    request_id: String,
    revision: u64,
    created: Instant,
    awaiting_since: Option<Instant>,
    finished: Option<Instant>,
    approved: bool,
    result: Value,
    cancel: watch::Sender<bool>,
}
impl Task {
    fn approval_expired(&self) -> bool {
        self.review.state == State::AwaitingUser
            && self.awaiting_since.unwrap_or(self.created).elapsed() > Duration::from_secs(120)
    }
}
struct Session {
    client: String,
    target: Target,
    last_used: Instant,
}
struct Inner {
    policy: Policy,
    tasks: HashMap<String, Task>,
    sessions: HashMap<String, Session>,
}
pub struct Engine {
    pub store: Store,
    remote: Arc<dyn Remote>,
    inner: Mutex<Inner>,
    pub generation: String,
    stamp: AtomicU64,
}
impl Engine {
    pub fn new(store: Store, remote: Arc<dyn Remote>) -> anyhow::Result<Arc<Self>> {
        let policy = store.load_policy()?;
        Ok(Arc::new(Self {
            store,
            remote,
            inner: Mutex::new(Inner {
                policy,
                tasks: HashMap::new(),
                sessions: HashMap::new(),
            }),
            generation: Uuid::new_v4().to_string(),
            stamp: AtomicU64::new(1),
        }))
    }
    pub fn stamp(&self) -> u64 {
        self.stamp.load(Ordering::Relaxed)
    }
    fn changed(&self) {
        self.stamp.fetch_add(1, Ordering::Relaxed);
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>> {
        self.inner
            .lock()
            .map_err(|_| Failure::new("INTERNAL_ERROR"))
    }
    pub fn policy(&self) -> Result<Policy> {
        Ok(self.lock()?.policy.clone())
    }
    pub fn set_policy(&self, mut policy: Policy) -> Result<()> {
        let mut inner = self.lock()?;
        policy.revision = inner.policy.revision.saturating_add(1);
        self.store
            .save_policy(&policy)
            .map_err(|_| Failure::new("STORAGE_ERROR"))?;
        inner.policy = policy;
        for task in inner
            .tasks
            .values_mut()
            .filter(|t| !t.review.state.terminal())
        {
            if task.review.state == State::AwaitingUser || task.review.state == State::Queued {
                task.review.state = State::Failed;
                task.review.error = Some(Failure::new("POLICY_CHANGED"));
                task.finished = Some(Instant::now());
            }
            task.cancel.send_replace(true);
        }
        for session in inner.sessions.keys().cloned().collect::<Vec<_>>() {
            let remote = self.remote.clone();
            tokio::spawn(async move {
                remote.close(&session).await;
            });
        }
        inner.sessions.clear();
        self.changed();
        Ok(())
    }
    pub fn reviews(&self) -> Result<Vec<Review>> {
        let inner = self.lock()?;
        let mut tasks = inner.tasks.values().collect::<Vec<_>>();
        tasks.sort_by_key(|t| {
            (
                t.review.state != State::AwaitingUser,
                std::cmp::Reverse(t.created),
            )
        });
        Ok(tasks.into_iter().map(|t| t.review.clone()).collect())
    }
    pub fn maintain(&self) -> Result<()> {
        let mut inner = self.lock()?;
        let previous = inner.tasks.len();
        inner.tasks.retain(|_, t| {
            t.finished
                .is_none_or(|at| at.elapsed() < Duration::from_secs(1800))
        });
        let expired = inner
            .sessions
            .iter()
            .filter(|(_, s)| s.last_used.elapsed() > Duration::from_secs(600))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for session in &expired {
            inner.sessions.remove(session);
            let remote = self.remote.clone();
            let session = session.clone();
            tokio::spawn(async move {
                remote.close(&session).await;
            });
        }
        let mut changed = previous != inner.tasks.len() || !expired.is_empty();
        for task in inner.tasks.values_mut().filter(|t| t.approval_expired()) {
            task.review.state = State::Failed;
            task.review.error = Some(Failure::new("APPROVAL_EXPIRED"));
            task.finished = Some(Instant::now());
            changed = true;
        }
        if changed {
            self.changed();
        }
        Ok(())
    }
    pub fn credentials_session(&self, id: &str) -> Result<String> {
        let inner = self.lock()?;
        let task = inner
            .tasks
            .get(id)
            .filter(|t| {
                t.review.state == State::AwaitingUser
                    && t.review
                        .error
                        .as_ref()
                        .is_some_and(|e| e.code == "AUTH_REQUIRED")
            })
            .ok_or_else(|| Failure::new("TASK_NOT_PENDING"))?;
        Ok(task.session.clone())
    }
    pub fn stop_all(&self) -> Result<()> {
        let mut inner = self.lock()?;
        for task in inner
            .tasks
            .values_mut()
            .filter(|t| !t.review.state.terminal())
        {
            Self::cancel(task);
        }
        let sessions = inner.sessions.keys().cloned().collect::<Vec<_>>();
        inner.sessions.clear();
        for session in sessions {
            let remote = self.remote.clone();
            tokio::spawn(async move {
                remote.close(&session).await;
            });
        }
        self.changed();
        Ok(())
    }
    pub fn revoke_client(&self, client: &str) -> Result<()> {
        self.store
            .revoke(client)
            .map_err(|_| Failure::new("STORAGE_ERROR"))?;
        let mut inner = self.lock()?;
        for task in inner
            .tasks
            .values_mut()
            .filter(|t| t.review.client == client && !t.review.state.terminal())
        {
            Self::cancel(task);
        }
        let sessions = inner
            .sessions
            .iter()
            .filter(|(_, s)| s.client == client)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in sessions {
            inner.sessions.remove(&id);
            let remote = self.remote.clone();
            tokio::spawn(async move {
                remote.close(&id).await;
            });
        }
        self.changed();
        Ok(())
    }
    pub fn approve(self: &Arc<Self>, id: &str, approved: bool) -> Result<()> {
        let mut inner = self.lock()?;
        let revision = inner.policy.revision;
        let task = inner
            .tasks
            .get_mut(id)
            .ok_or_else(|| Failure::new("TASK_NOT_FOUND"))?;
        if task.review.state != State::AwaitingUser {
            return Err(Failure::new("TASK_NOT_PENDING"));
        }
        self.changed();
        if task.revision != revision || task.approval_expired() {
            task.review.state = State::Failed;
            task.review.error = Some(Failure::new("APPROVAL_EXPIRED"));
            task.finished = Some(Instant::now());
            return Err(Failure::new("APPROVAL_EXPIRED"));
        }
        if !approved {
            task.review.state = State::Failed;
            task.review.error = Some(Failure::new("APPROVAL_DENIED"));
            task.finished = Some(Instant::now());
            return Ok(());
        }
        task.approved = true;
        task.review.error = None;
        task.review.state = State::Queued;
        drop(inner);
        self.launch(id.to_owned());
        Ok(())
    }
    fn cancel(task: &mut Task) {
        task.cancel.send_replace(true);
        if matches!(task.review.state, State::Queued | State::AwaitingUser) {
            task.review.state = State::Cancelled;
            task.finished = Some(Instant::now());
        } else if task.review.state == State::Running {
            task.review.state = State::CancelRequested;
        }
    }
    fn launch(self: &Arc<Self>, id: String) {
        let this = self.clone();
        tokio::spawn(async move {
            this.work(id).await;
        });
    }
    async fn validate(&self, target: &Target, revision: u64) -> Result<()> {
        let targets = self.remote.targets().await?;
        let current = targets
            .into_iter()
            .find(|t| t.id == target.id && t.digest == target.digest)
            .ok_or_else(|| Failure::new("CONNECTION_NOT_ALLOWED"))?;
        let inner = self.lock()?;
        if !inner.policy.enabled
            || inner.policy.revision != revision
            || !inner.policy.authorized(&current.id, &current.digest)
        {
            return Err(Failure::new("POLICY_CHANGED"));
        }
        Ok(())
    }
    fn finish(&self, id: &str, result: Result<Value>) {
        self.changed();
        let Ok(mut inner) = self.lock() else { return };
        let Some(task) = inner.tasks.get_mut(id) else {
            return;
        };
        if task.review.state.terminal() {
            return;
        }
        match result {
            Ok(data) if *task.cancel.borrow() => {
                task.result = data;
                task.review.state = State::OutcomeUnknown;
                task.review.error = Some(Failure::new("OUTCOME_UNKNOWN"));
            }
            Ok(data) => {
                if matches!(task.review.call, Call::OpenSession(_)) {
                    let session = Session {
                        client: task.review.client.clone(),
                        target: task.review.target.clone(),
                        last_used: Instant::now(),
                    };
                    task.result = data;
                    task.review.state = State::Succeeded;
                    task.finished = Some(Instant::now());
                    let session_id = task.session.clone();
                    inner.sessions.insert(session_id, session);
                    drop(inner);
                    self.audit(id);
                    return;
                }
                task.result = data;
                task.review.state = if task
                    .result
                    .get("exit_code")
                    .and_then(Value::as_u64)
                    .is_some_and(|c| c != 0)
                {
                    task.review.error = Some(Failure::new("COMMAND_FAILED"));
                    State::Failed
                } else {
                    State::Succeeded
                };
            }
            Err(error)
                if error.code == "AUTH_REQUIRED"
                    && matches!(task.review.call, Call::OpenSession(_)) =>
            {
                task.review.state = State::AwaitingUser;
                task.awaiting_since = Some(Instant::now());
                task.review.error = Some(error);
                return;
            }
            Err(mut error) => {
                task.result = error.result.take().unwrap_or_else(|| json!({}));
                task.review.state = match error.code.as_str() {
                    "OUTCOME_UNKNOWN" => State::OutcomeUnknown,
                    "TIMEOUT" => State::TimedOut,
                    "CANCELLED" => State::Cancelled,
                    _ => State::Failed,
                };
                task.review.error = Some(error);
            }
        }
        task.finished = Some(Instant::now());
        drop(inner);
        self.audit(id);
    }
    fn audit(&self, id: &str) {
        let Ok(inner) = self.lock() else {
            return;
        };
        let Some(task) = inner.tasks.get(id) else {
            return;
        };
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|t| t.as_secs())
            .unwrap_or_default();
        let tool = serde_json::to_value(&task.review.call)
            .ok()
            .and_then(|v| v["tool"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let record = json!({"timestamp":timestamp,"client_id":task.review.client,"connection_id":task.review.target.id,"operation":tool,"mode":inner.policy.mode,"state":task.review.state,"exit_code":task.result.get("exit_code")});
        drop(inner);
        let _ = self.store.audit(record);
    }
    async fn work(self: Arc<Self>, id: String) {
        let job = (|| {
            let mut inner = self.lock()?;
            let pending = inner
                .tasks
                .get(&id)
                .ok_or_else(|| Failure::new("TASK_NOT_FOUND"))?;
            if pending.review.state != State::Queued {
                return Err(Failure::new("TASK_NOT_PENDING"));
            }
            let owner = pending.review.client.clone();
            let target_id = pending.review.target.id.clone();
            let mutation = pending.review.call.mutation();
            let busy = inner.tasks.values().any(|t| {
                matches!(t.review.state, State::Running | State::CancelRequested)
                    && mutation
                    && t.review.call.mutation()
                    && t.review.target.id == target_id
            });
            let count = inner
                .tasks
                .values()
                .filter(|t| {
                    matches!(t.review.state, State::Running | State::CancelRequested)
                        && t.review.client == owner
                })
                .count();
            if busy || count >= 2 {
                return Err(Failure::new("BUSY"));
            }
            let task = inner
                .tasks
                .get_mut(&id)
                .ok_or_else(|| Failure::new("TASK_NOT_FOUND"))?;
            task.review.state = State::Running;
            self.changed();
            Ok((
                task.review.clone(),
                task.session.clone(),
                task.revision,
                task.approved,
                task.cancel.subscribe(),
            ))
        })();
        let (review, session, revision, approved, cancel) = match job {
            Ok(j) => j,
            Err(e) => {
                if e.code == "BUSY" {
                    self.finish(&id, Err(e));
                }
                return;
            }
        };
        if let Err(e) = self.validate(&review.target, revision).await {
            self.finish(&id, Err(e));
            return;
        }
        if !self.store.client(&review.client).is_ok_and(|c| c.enabled) {
            self.finish(&id, Err(Failure::new("CLIENT_REVOKED")));
            return;
        }
        if !approved && matches!(review.call, Call::WriteFile(_)) {
            match self
                .remote
                .prepare(&review.target, &session, &review.call)
                .await
            {
                Ok(preview) => {
                    if let Ok(mut inner) = self.lock()
                        && let Some(task) = inner.tasks.get_mut(&id)
                    {
                        task.review.preview = preview;
                    }
                }
                Err(error) => {
                    self.finish(&id, Err(error));
                    return;
                }
            }
        }
        let needs_approval = self
            .policy()
            .is_ok_and(|p| p.mode == crate::policy::Mode::Controlled)
            && review.call.mutation()
            && !approved;
        if needs_approval {
            if let Ok(mut inner) = self.lock()
                && let Some(task) = inner.tasks.get_mut(&id)
                && task.review.state == State::Running
            {
                task.review.state = State::AwaitingUser;
                task.awaiting_since = Some(Instant::now());
            }
            self.changed();
            return;
        }
        if *cancel.borrow() {
            self.finish(&id, Err(Failure::new("OUTCOME_UNKNOWN")));
            return;
        }
        if let Err(e) = self.validate(&review.target, revision).await {
            self.finish(&id, Err(e));
            return;
        }
        if matches!(review.call, Call::CloseSession(_))
            && let Ok(mut inner) = self.lock()
        {
            for (other_id, task) in inner
                .tasks
                .iter_mut()
                .filter(|(_, t)| t.session == session && !t.review.state.terminal())
            {
                if other_id != &id {
                    Self::cancel(task);
                }
            }
        }
        let cancelled = cancel.clone();
        let mut result = self
            .remote
            .execute(&review.target, &session, &review.call, cancel)
            .await;
        if matches!(review.call, Call::OpenSession(_))
            && (*cancelled.borrow()
                || self.validate(&review.target, revision).await.is_err()
                || !self.store.client(&review.client).is_ok_and(|c| c.enabled))
        {
            self.remote.close(&session).await;
            result = Err(Failure::new(if *cancelled.borrow() {
                "CANCELLED"
            } else {
                "POLICY_CHANGED"
            }));
        }
        if matches!(review.call, Call::CloseSession(_))
            && let Ok(mut inner) = self.lock()
        {
            inner.sessions.remove(&session);
        }
        self.finish(&id, result);
    }
    async fn dispatch(self: &Arc<Self>, client: &str, call: Call) -> Result<Value> {
        if !self.store.client(client).is_ok_and(|c| c.enabled) {
            return Err(Failure::new("CLIENT_REVOKED"));
        }
        if !self.policy()?.enabled {
            return Err(Failure::new("MCP_DISABLED"));
        }
        let targets = self.remote.targets().await?;
        {
            let mut inner = self.lock()?;
            let before = inner.policy.known_targets.len();
            inner
                .policy
                .observe(targets.iter().map(|t| (t.id.clone(), t.digest.clone())));
            if before != inner.policy.known_targets.len() {
                self.store
                    .save_policy(&inner.policy)
                    .map_err(|_| Failure::new("STORAGE_ERROR"))?;
            }
            if !inner.policy.enabled {
                return Err(Failure::new("MCP_DISABLED"));
            }
            let expired = inner
                .sessions
                .iter()
                .filter(|(_, s)| s.last_used.elapsed() > Duration::from_secs(600))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for session in expired {
                inner.sessions.remove(&session);
                let remote = self.remote.clone();
                tokio::spawn(async move {
                    remote.close(&session).await;
                });
            }
            inner.tasks.retain(|_, t| {
                t.finished
                    .is_none_or(|at| at.elapsed() < Duration::from_secs(1800))
            });
            for task in inner.tasks.values_mut().filter(|t| t.approval_expired()) {
                task.review.state = State::Failed;
                task.review.error = Some(Failure::new("APPROVAL_EXPIRED"));
                task.finished = Some(Instant::now());
            }
        }
        match &call {
            Call::GetStatus(_) => {
                return Ok(
                    json!({"app_version":self.remote.app_version(),"generation":self.generation,"mode":self.policy()?.mode,"limits":{"text_bytes":crate::contracts::MAX_TEXT,"output_bytes":crate::contracts::MAX_OUTPUT}}),
                );
            }
            Call::ListConnections(p) => {
                let policy = self.policy()?;
                let search = p.search.to_lowercase();
                let listed = targets
                    .iter()
                    .filter(|t| {
                        policy.authorized(&t.id, &t.digest)
                            && (t.name.to_lowercase().contains(&search)
                                || t.host.to_lowercase().contains(&search))
                    })
                    .collect::<Vec<_>>();
                let page = listed
                    .iter()
                    .skip(p.offset)
                    .take(p.limit)
                    .collect::<Vec<_>>();
                return Ok(
                    json!({"connections":page,"next_offset":(p.offset.saturating_add(p.limit)<listed.len()).then_some(p.offset.saturating_add(p.limit))}),
                );
            }
            Call::GetTask(p) => {
                let inner = self.lock()?;
                let task = inner
                    .tasks
                    .get(&p.task_id)
                    .filter(|t| t.review.client == client)
                    .ok_or_else(|| Failure::new("TASK_NOT_FOUND"))?;
                let mut result = task.result.clone();
                let mut outputs = Vec::new();
                let stdout = result.get("stdout").and_then(Value::as_str).unwrap_or("");
                let stderr = result.get("stderr").and_then(Value::as_str).unwrap_or("");
                let all = format!("{stdout}{stderr}");
                let start = all.ceil_char_boundary(p.offset.min(all.len()));
                let end = all.floor_char_boundary(start.saturating_add(p.limit).min(all.len()));
                if start < stdout.len() {
                    outputs
                        .push(json!({"stream":"stdout","text":&all[start..end.min(stdout.len())]}));
                }
                if end > stdout.len() {
                    outputs
                        .push(json!({"stream":"stderr","text":&all[start.max(stdout.len())..end]}));
                }
                if let Some(object) = result.as_object_mut() {
                    object.remove("stdout");
                    object.remove("stderr");
                }
                return Ok(
                    json!({"id":p.task_id,"state":task.review.state,"error":task.review.error,"result":result,"output":outputs,"next_offset":(end<all.len()).then_some(end),"truncated":task.result.get("truncated").and_then(Value::as_bool).unwrap_or(false)}),
                );
            }
            Call::CancelTask(p) => {
                let mut inner = self.lock()?;
                let task = inner
                    .tasks
                    .get_mut(&p.task_id)
                    .filter(|t| t.review.client == client)
                    .ok_or_else(|| Failure::new("TASK_NOT_FOUND"))?;
                if !task.review.state.terminal() {
                    Self::cancel(task);
                }
                self.changed();
                return Ok(json!({"state":task.review.state,"cancellation_requested":true}));
            }
            _ => {}
        }
        if let Some(request_id) = call.request_id() {
            let inner = self.lock()?;
            if let Some(task) = inner
                .tasks
                .values()
                .find(|t| t.review.client == client && t.request_id == request_id)
            {
                if serde_json::to_value(&task.review.call).ok() != serde_json::to_value(&call).ok()
                {
                    return Err(Failure::new("REQUEST_ID_CONFLICT"));
                }
                return Ok(json!({"task_id":task.review.id}));
            }
        }
        let (target, session) = if let Call::OpenSession(p) = &call {
            let target = targets
                .iter()
                .find(|t| t.id == p.connection_id)
                .cloned()
                .ok_or_else(|| Failure::new("CONNECTION_NOT_ALLOWED"))?;
            (target, Uuid::new_v4().to_string())
        } else {
            let mut inner = self.lock()?;
            let session = inner
                .sessions
                .get_mut(
                    call.session_id()
                        .ok_or_else(|| Failure::new("INVALID_ARGUMENT"))?,
                )
                .filter(|s| s.client == client)
                .ok_or_else(|| Failure::new("SESSION_NOT_FOUND"))?;
            session.last_used = Instant::now();
            (
                session.target.clone(),
                call.session_id().unwrap_or_default().to_owned(),
            )
        };
        let policy = self.policy()?;
        if decide(
            policy.enabled,
            policy.authorized(&target.id, &target.digest),
            policy.mode,
            call.mutation(),
        ) == Decision::Deny
        {
            return Err(Failure::new(
                if !policy.authorized(&target.id, &target.digest) {
                    "CONNECTION_NOT_ALLOWED"
                } else {
                    "PERMISSION_DENIED"
                },
            ));
        }
        self.validate(&target, policy.revision).await?;
        if let Some(request_id) = call.request_id() {
            let mut inner = self.lock()?;
            if let Some(task) = inner
                .tasks
                .values()
                .find(|t| t.review.client == client && t.request_id == request_id)
            {
                if serde_json::to_value(&task.review.call).ok() != serde_json::to_value(&call).ok()
                {
                    return Err(Failure::new("REQUEST_ID_CONFLICT"));
                }
                return Ok(json!({"task_id":task.review.id}));
            }
            if inner
                .tasks
                .values()
                .filter(|t| !t.review.state.terminal())
                .count()
                >= 128
                || inner
                    .tasks
                    .values()
                    .filter(|t| t.review.client == client)
                    .count()
                    >= 256
            {
                return Err(Failure::new("TASK_LIMIT"));
            }
            if matches!(call, Call::OpenSession(_))
                && inner
                    .sessions
                    .values()
                    .filter(|s| s.client == client)
                    .count()
                    >= 8
            {
                return Err(Failure::new("SESSION_LIMIT"));
            }
            let id = Uuid::new_v4().to_string();
            let (cancel, _) = watch::channel(false);
            let review = Review {
                id: id.clone(),
                client: client.to_owned(),
                target,
                call: call.clone(),
                state: State::Queued,
                error: None,
                preview: json!({}),
            };
            inner.tasks.insert(
                id.clone(),
                Task {
                    review,
                    session,
                    request_id: request_id.to_owned(),
                    revision: policy.revision,
                    created: Instant::now(),
                    awaiting_since: None,
                    finished: None,
                    approved: false,
                    result: json!({}),
                    cancel,
                },
            );
            self.changed();
            drop(inner);
            self.launch(id.clone());
            return Ok(json!({"task_id":id}));
        }
        self.remote
            .execute(&target, &session, &call, watch::channel(false).1)
            .await
    }
}
pub struct EngineHandler(pub Arc<Engine>);
#[async_trait]
impl Handler for EngineHandler {
    fn generation(&self) -> Option<String> {
        Some(self.0.generation.clone())
    }
    async fn call(&self, client: &str, call: Call) -> Result<Value> {
        self.0.dispatch(client, call).await
    }
}
