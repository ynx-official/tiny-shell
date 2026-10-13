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
mod audit;
mod internal;
mod waiting;
pub use internal::{AiPolicy, InternalClient};

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
    audit_context: crate::audit::Context,
    audit_store: Arc<crate::audit::Store>,
    review: Review,
    session: String,
    request_id: String,
    revision: u64,
    mode: crate::policy::Mode,
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
    audit_context: crate::audit::Context,
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
    pub audit: Arc<crate::audit::Store>,
    pub store: Store,
    remote: Arc<dyn Remote>,
    inner: Mutex<Inner>,
    pub generation: String,
    stamp: AtomicU64,
    configuration_stamp: AtomicU64,
    ai: Mutex<internal::Access>,
    changes: watch::Sender<u64>,
}
impl Engine {
    pub fn new(store: Store, remote: Arc<dyn Remote>) -> anyhow::Result<Arc<Self>> {
        let directory = store
            .root()
            .parent()
            .ok_or_else(|| anyhow::anyhow!("AUDIT_INVALID_PATH"))?
            .to_path_buf();
        Self::with_audit(store, remote, crate::audit::Store::new(directory))
    }
    pub fn with_audit(
        store: Store,
        remote: Arc<dyn Remote>,
        audit: Arc<crate::audit::Store>,
    ) -> anyhow::Result<Arc<Self>> {
        let policy = store.load_policy()?;
        let ai = internal::Access::load(&store, &policy)?;
        let (changes, _) = watch::channel(0);
        Ok(Arc::new(Self {
            audit,
            store,
            remote,
            inner: Mutex::new(Inner {
                policy,
                tasks: HashMap::new(),
                sessions: HashMap::new(),
            }),
            generation: Uuid::new_v4().to_string(),
            stamp: AtomicU64::new(1),
            configuration_stamp: AtomicU64::new(1),
            ai: Mutex::new(ai),
            changes,
        }))
    }
    pub fn stamp(&self) -> u64 {
        self.stamp.load(Ordering::Relaxed)
    }
    pub fn configuration_stamp(&self) -> u64 {
        self.configuration_stamp.load(Ordering::Relaxed)
    }
    fn changed(&self) {
        self.stamp.fetch_add(1, Ordering::Relaxed);
        self.changes
            .send_modify(|version| *version = version.wrapping_add(1));
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>> {
        self.inner
            .lock()
            .map_err(|_| Failure::new("INTERNAL_ERROR"))
    }
    pub fn policy(&self) -> Result<Policy> {
        Ok(self.lock()?.policy.clone())
    }
    pub fn set_policy(&self, policy: Policy) -> Result<()> {
        self.set_policy_checked(policy, None)
    }
    pub fn set_synced_policy(&self, policy: Policy, expected_stamp: u64) -> Result<()> {
        self.set_policy_checked(policy, Some(expected_stamp))
    }
    fn set_policy_checked(&self, mut policy: Policy, expected_stamp: Option<u64>) -> Result<()> {
        let mut inner = self.lock()?;
        if expected_stamp.is_some_and(|stamp| stamp != self.configuration_stamp()) {
            return Err(Failure::new("POLICY_CHANGED"));
        }
        policy.revision = inner.policy.revision.saturating_add(1);
        self.store
            .save_policy(&policy)
            .map_err(|_| Failure::new("STORAGE_ERROR"))?;
        let actor = if expected_stamp.is_some() {
            "cloud_sync"
        } else {
            "local_user"
        };
        let channel = if expected_stamp.is_some() {
            "synchronization"
        } else {
            "application"
        };
        self.audit.record(crate::audit::Event::new(crate::audit::Context::new(crate::audit::Source::Local),"policy_changed","permissions","completed",json!({"scope":"external_mcp_cli","before":inner.policy,"after":policy,"actor":actor,"channel":channel})));
        inner.policy = policy;
        self.configuration_stamp.fetch_add(1, Ordering::Relaxed);
        self.cancel_source(&mut inner, false);
        self.changed();
        Ok(())
    }
    fn cancel_source(&self, inner: &mut Inner, ai: bool) {
        for task in inner
            .tasks
            .values_mut()
            .filter(|t| t.review.client.starts_with("ai:") == ai && !t.review.state.terminal())
        {
            task.record(
                "approval_invalidated",
                "invalidated",
                json!({"reason":"policy_changed"}),
            );
            if matches!(task.review.state, State::AwaitingUser | State::Queued) {
                task.review.state = State::Failed;
                task.review.error = Some(Failure::new("POLICY_CHANGED"));
                task.finished = Some(Instant::now());
            }
            task.cancel.send_replace(true);
        }
        let sessions = inner
            .sessions
            .iter()
            .filter(|(_, s)| s.client.starts_with("ai:") == ai)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for session in sessions {
            if let Some(active) = inner.sessions.remove(&session) {
                self.close_automatically(session, active, "policy_changed");
            }
        }
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
    /// Load only the operation being reviewed, without copying unrelated task previews.
    pub fn review(&self, id: &str) -> Result<Review> {
        self.lock()?
            .tasks
            .get(id)
            .map(|task| task.review.clone())
            .ok_or_else(|| Failure::new("TASK_NOT_FOUND"))
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
            if let Some(active) = inner.sessions.remove(session) {
                self.close_automatically(session.clone(), active, "idle_timeout");
            }
        }
        let mut changed = previous != inner.tasks.len() || !expired.is_empty();
        for task in inner.tasks.values_mut().filter(|t| t.approval_expired()) {
            task.record("approval_expired", "expired", json!({"reason":"deadline"}));
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
        let sessions = inner.sessions.drain().collect::<Vec<_>>();
        for (id, session) in sessions {
            self.close_automatically(id, session, "application_shutdown");
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
            if let Some(active) = inner.sessions.remove(&id) {
                self.close_automatically(id, active, "client_revoked");
            }
        }
        self.changed();
        Ok(())
    }
    pub fn approve(self: &Arc<Self>, id: &str, approved: bool) -> Result<()> {
        let mut inner = self.lock()?;
        let client = &inner
            .tasks
            .get(id)
            .ok_or_else(|| Failure::new("TASK_NOT_FOUND"))?
            .review
            .client;
        let revision = self.effective_policy(client, &inner.policy)?.revision;
        let task = inner
            .tasks
            .get_mut(id)
            .ok_or_else(|| Failure::new("TASK_NOT_FOUND"))?;
        if task.review.state != State::AwaitingUser {
            return Err(Failure::new("TASK_NOT_PENDING"));
        }
        self.changed();
        if task.revision != revision || task.approval_expired() {
            task.record(
                "approval_expired",
                "expired",
                json!({"decision":"expired","current_revision":revision}),
            );
            task.review.state = State::Failed;
            task.review.error = Some(Failure::new("APPROVAL_EXPIRED"));
            task.finished = Some(Instant::now());
            return Err(Failure::new("APPROVAL_EXPIRED"));
        }
        if !approved {
            task.record(
                "approval_rejected",
                "rejected",
                json!({"decision":"rejected","actor":"local_user","channel":"application"}),
            );
            task.review.state = State::Failed;
            task.review.error = Some(Failure::new("APPROVAL_DENIED"));
            task.finished = Some(Instant::now());
            return Ok(());
        }
        task.approved = true;
        task.record(
            "approval_approved",
            "approved",
            json!({"decision":"approved","actor":"local_user","channel":"application"}),
        );
        task.review.error = None;
        task.review.state = State::Queued;
        drop(inner);
        self.launch(id.to_owned());
        Ok(())
    }
    fn cancel(task: &mut Task) {
        task.record("cancel_requested", "cancel_requested", json!({}));
        task.cancel.send_replace(true);
        if matches!(task.review.state, State::Queued | State::AwaitingUser) {
            task.review.state = State::Cancelled;
            task.finished = Some(Instant::now());
        } else if task.review.state == State::Running {
            task.review.state = State::CancelRequested;
        }
        let state = serde_json::to_value(task.review.state)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".into());
        task.record("cancel_decision",&state,json!({"actor":"application","remote_outcome_may_be_unknown":task.review.state==State::CancelRequested}));
    }
    fn launch(self: &Arc<Self>, id: String) {
        let this = self.clone();
        tokio::spawn(async move {
            this.work(id).await;
        });
    }
    async fn validate(&self, target: &Target, revision: u64, client: &str) -> Result<()> {
        let targets = self.remote.targets().await?;
        let current = targets
            .into_iter()
            .find(|t| t.id == target.id && t.digest == target.digest)
            .ok_or_else(|| Failure::new("CONNECTION_NOT_ALLOWED"))?;
        let inner = self.lock()?;
        let policy = self.effective_policy(client, &inner.policy)?;
        if !policy.enabled
            || policy.revision != revision
            || !policy.authorized(&current.id, &current.digest)
        {
            return Err(Failure::new("POLICY_CHANGED"));
        }
        Ok(())
    }
    fn finish(&self, id: &str, result: Result<Value>) {
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
                        audit_context: task.audit_context.clone(),
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
                    self.changed();
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
                task.record(
                    "approval_requested",
                    "awaiting_user",
                    json!({"approval_kind":"credentials","credential_values_saved":false}),
                );
                task.review.error = Some(error);
                self.changed();
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
        self.changed();
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
        let mode = task.mode;
        let state = serde_json::to_value(task.review.state)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".into());
        task.record("execution_result", &state, json!({"result":task.result}));
        let record = json!({"timestamp":timestamp,"client_id":task.review.client,"connection_id":task.review.target.id,"operation":tool,"mode":mode,"state":task.review.state,"exit_code":task.result.get("exit_code")});
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
        if let Err(e) = self
            .validate(&review.target, revision, &review.client)
            .await
        {
            self.finish(&id, Err(e));
            return;
        }
        if !self.client_enabled(&review.client) {
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
            .lock()
            .and_then(|inner| self.effective_policy(&review.client, &inner.policy))
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
                task.record(
                    "approval_requested",
                    "awaiting_user",
                    json!({"approval_kind":"operation_permission"}),
                );
            }
            self.changed();
            return;
        }
        if *cancel.borrow() {
            self.finish(&id, Err(Failure::new("OUTCOME_UNKNOWN")));
            return;
        }
        if let Err(e) = self
            .validate(&review.target, revision, &review.client)
            .await
        {
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
        let durable = self.execution_record(&review, revision, approved).await;
        if review.call.mutation() && !durable {
            self.finish(&id, Err(Failure::new("AUDIT_UNAVAILABLE")));
            return;
        }
        // The durable write is an await point: revocation/cancellation must still win afterwards.
        if *cancel.borrow() || !self.client_enabled(&review.client) {
            self.finish(&id, Err(Failure::new("CANCELLED")));
            return;
        }
        if let Err(error) = self
            .validate(&review.target, revision, &review.client)
            .await
        {
            self.finish(&id, Err(error));
            return;
        }
        let mut result = self
            .remote
            .execute(&review.target, &session, &review.call, cancel)
            .await;
        if matches!(review.call, Call::OpenSession(_))
            && (*cancelled.borrow()
                || self
                    .validate(&review.target, revision, &review.client)
                    .await
                    .is_err()
                || !self.client_enabled(&review.client))
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
    async fn dispatch(
        self: &Arc<Self>,
        client: &str,
        call: Call,
        epoch: Option<u64>,
    ) -> Result<Value> {
        self.dispatch_traced(client, call, epoch, crate::audit::Context::client(client))
            .await
    }
    async fn dispatch_operation(
        self: &Arc<Self>,
        client: &str,
        call: Call,
        epoch: Option<u64>,
        audit_context: &mut crate::audit::Context,
    ) -> Result<Value> {
        if !self.client_enabled(client) {
            return Err(Failure::new("CLIENT_REVOKED"));
        }
        if !self.effective_policy(client, &self.policy()?)?.enabled {
            return Err(Failure::new("MCP_DISABLED"));
        }
        // A task query needs current authorization and ownership, not SSH target loading.
        if let Call::GetTask(query) = &call {
            return self.wait_task(client, query, epoch).await;
        }
        let targets = self.bound_targets(client, self.remote.targets().await?)?;
        {
            let mut inner = self.lock()?;
            self.verify_internal_epoch(client, epoch)?;
            let before = inner.policy.known_targets.len();
            if !client.starts_with("ai:") {
                inner
                    .policy
                    .observe(targets.iter().map(|t| (t.id.clone(), t.digest.clone())));
            }
            if before != inner.policy.known_targets.len() {
                self.store
                    .save_policy(&inner.policy)
                    .map_err(|_| Failure::new("STORAGE_ERROR"))?;
            }
            if !self.effective_policy(client, &inner.policy)?.enabled {
                return Err(Failure::new("MCP_DISABLED"));
            }
            let expired = inner
                .sessions
                .iter()
                .filter(|(_, s)| s.last_used.elapsed() > Duration::from_secs(600))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for session in expired {
                if let Some(active) = inner.sessions.remove(&session) {
                    self.close_automatically(session, active, "idle_timeout");
                }
            }
            inner.tasks.retain(|_, t| {
                t.finished
                    .is_none_or(|at| at.elapsed() < Duration::from_secs(1800))
            });
            for task in inner.tasks.values_mut().filter(|t| t.approval_expired()) {
                task.record("approval_expired", "expired", json!({"reason":"deadline"}));
                task.review.state = State::Failed;
                task.review.error = Some(Failure::new("APPROVAL_EXPIRED"));
                task.finished = Some(Instant::now());
            }
        }
        match &call {
            Call::GetStatus(_) => {
                return Ok(
                    json!({"app_version":self.remote.app_version(),"generation":self.generation,"mode":self.effective_policy(client, &self.policy()?)?.mode,"limits":{"text_bytes":crate::contracts::MAX_TEXT,"output_bytes":crate::contracts::MAX_OUTPUT}}),
                );
            }
            Call::ListConnections(p) => {
                let policy = self.effective_policy(client, &self.policy()?)?;
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
                return self.wait_task(client, p, epoch).await;
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
                audit_context.parent_id = Some(task.audit_context.operation_id.clone());
                audit_context.task_id = Some(task.review.id.clone());
                self.audit.record(crate::audit::Event::new(audit_context.clone(),"idempotent_reuse",audit::tool_name(&call),"idempotent",json!({"original_operation":task.audit_context.operation_id,"task_id":task.review.id,"remote_executed_again":false})));
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
        let policy = {
            let inner = self.lock()?;
            self.effective_policy(client, &inner.policy)?
        };
        let revision = policy.revision;
        audit_context.target_id = Some(target.id.clone());
        audit_context.target_name = Some(target.name.clone());
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
        self.validate(&target, revision, client).await?;
        if let Some(request_id) = call.request_id() {
            let mut inner = self.lock()?;
            self.verify_internal_epoch(client, epoch)?;
            let current = self.effective_policy(client, &inner.policy)?;
            if current.revision != revision || !current.enabled {
                return Err(Failure::new("POLICY_CHANGED"));
            }
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
            audit_context.task_id = Some(id.clone());
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
                    audit_context: audit_context.clone(),
                    audit_store: self.audit.clone(),
                    review,
                    session,
                    request_id: request_id.to_owned(),
                    revision,
                    mode: policy.mode,
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
    fn audit_store(&self) -> Option<Arc<crate::audit::Store>> {
        Some(self.0.audit.clone())
    }
    async fn call_with_context(
        &self,
        client: &str,
        call: Call,
        mut context: crate::audit::Context,
    ) -> Result<Value> {
        if client.starts_with("ai:") {
            return Err(Failure::new("CLIENT_REVOKED"));
        }
        context.source = crate::audit::Source::External;
        context.conversation_id = None;
        context.turn_id = None;
        context.model_request_id = None;
        self.0.dispatch_traced(client, call, None, context).await
    }
    fn generation(&self) -> Option<String> {
        Some(self.0.generation.clone())
    }
    async fn call(&self, client: &str, call: Call) -> Result<Value> {
        if client.starts_with("ai:") {
            return Err(Failure::new("CLIENT_REVOKED"));
        }
        self.0.dispatch(client, call, None).await
    }
}
