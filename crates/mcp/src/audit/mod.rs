//! Shared append-only audit actor. No caller performs database/keychain I/O on a UI thread.
mod database;
pub mod redact;
mod types;
use anyhow::{Result, anyhow};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};
use tokio::sync::oneshot;
pub use types::*;

enum Command {
    Append(Box<Event>, Option<oneshot::Sender<bool>>),
    Page(Filter, oneshot::Sender<Result<Page>>),
    Configure(Settings, oneshot::Sender<Result<()>>),
    Clear(oneshot::Sender<Result<()>>),
    Summary(Option<String>, oneshot::Sender<Result<Summary>>),
    Retry(oneshot::Sender<Result<()>>),
    StartProfile(String),
    Close(oneshot::Sender<()>),
}
pub struct Store {
    sender: mpsc::SyncSender<Command>,
    status: Arc<Mutex<Status>>,
    stamp: Arc<AtomicU64>,
}
impl Store {
    pub fn new(directory: PathBuf) -> Arc<Self> {
        Self::start(directory, None)
    }
    /// Explicit key injection for isolated embedders/tests; an existing key binding is never replaced.
    pub fn with_key(directory: PathBuf, key: [u8; 32]) -> Arc<Self> {
        Self::start(directory, Some(key))
    }
    fn start(directory: PathBuf, key: Option<[u8; 32]>) -> Arc<Self> {
        let (sender, receiver) = mpsc::sync_channel(32);
        let status = Arc::new(Mutex::new(Status::default()));
        let worker_status = status.clone();
        let stamp = Arc::new(AtomicU64::new(1));
        let worker_stamp = stamp.clone();
        let spawned = std::thread::Builder::new()
            .name("tiny-shell-audit".into())
            .spawn(move || worker(directory, key, receiver, worker_status, worker_stamp));
        if spawned.is_err()
            && let Ok(mut status) = status.lock()
        {
            status.ready = true;
            status.error = Some("AUDIT_UNAVAILABLE".into());
        }
        Arc::new(Self {
            sender,
            status,
            stamp,
        })
    }
    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
    pub fn stamp(&self) -> u64 {
        self.stamp.load(Ordering::Relaxed)
    }
    pub fn record(&self, event: Event) {
        if self
            .sender
            .try_send(Command::Append(Box::new(event), None))
            .is_err()
            && let Ok(mut status) = self.status.lock()
        {
            status.durable = false;
            status.error = Some("AUDIT_QUEUE_FULL".into());
        }
    }
    pub fn start_profile(&self, instance: String) {
        if self
            .sender
            .try_send(Command::StartProfile(instance))
            .is_err()
            && let Ok(mut s) = self.status.lock()
        {
            s.durable = false;
            s.error = Some("AUDIT_UNAVAILABLE".into());
        }
    }
    /// A mutation must wait for this acknowledgment before starting a remote side effect.
    pub async fn durable(&self, event: Event) -> bool {
        let (tx, rx) = oneshot::channel();
        self.sender
            .try_send(Command::Append(Box::new(event), Some(tx)))
            .is_ok()
            && rx.await.unwrap_or(false)
    }
    pub async fn page(&self, filter: Filter) -> Result<Page> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .try_send(Command::Page(filter, tx))
            .map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))?;
        rx.await.map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))?
    }
    pub async fn configure(&self, settings: Settings) -> Result<()> {
        settings.validate()?;
        let (tx, rx) = oneshot::channel();
        self.sender
            .try_send(Command::Configure(settings, tx))
            .map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))?;
        rx.await.map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))?
    }
    pub async fn clear(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .try_send(Command::Clear(tx))
            .map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))?;
        rx.await.map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))?
    }
    /// Explicitly retries keychain/storage access and persists the bounded temporary ring.
    pub async fn retry(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .try_send(Command::Retry(tx))
            .map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))?;
        rx.await.map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))?
    }
    /// Finish queued records and release database handles before profile shutdown/removal.
    pub async fn close(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        let mut command = Command::Close(tx);
        loop {
            match self.sender.try_send(command) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Full(pending)) => {
                    command = pending;
                    tokio::task::yield_now().await;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err(anyhow!("AUDIT_UNAVAILABLE"));
                }
            }
        }
        rx.await.map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))
    }
    pub async fn summary(&self, conversation: Option<String>) -> Result<Summary> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .try_send(Command::Summary(conversation, tx))
            .map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))?;
        rx.await.map_err(|_| anyhow!("AUDIT_UNAVAILABLE"))?
    }
}
fn worker(
    directory: PathBuf,
    key: Option<[u8; 32]>,
    receiver: mpsc::Receiver<Command>,
    status: Arc<Mutex<Status>>,
    stamp: Arc<AtomicU64>,
) {
    let opened = database::Database::open(&directory, key);
    let mut database = match opened {
        Ok(db) => Some(db),
        Err(error) => {
            if let Ok(mut s) = status.lock() {
                s.error = Some(error.to_string());
            }
            None
        }
    };
    if let Ok(mut s) = status.lock() {
        s.ready = true;
        s.durable = database.is_some();
        if let Some(db) = &database {
            s.settings = db.settings.clone();
            s.bytes = db.bytes();
        }
    }
    stamp.fetch_add(1, Ordering::Relaxed);
    let mut temporary = VecDeque::<(Event, usize)>::new();
    let mut temporary_bytes = 0usize;
    let mut sequence = i64::MAX - 1_000_000;
    let mut profile_instance: Option<String> = None;
    while let Ok(command) = receiver.recv() {
        let changes_data = !matches!(&command, Command::Page(..) | Command::Summary(..));
        match command {
            Command::Append(event, ack) => {
                let mut event = *event;
                // Use exactly the same rules for temporary memory records and encrypted records.
                let settings = status
                    .lock()
                    .map(|s| s.settings.clone())
                    .unwrap_or_default();
                let patterns = settings
                    .redact_patterns
                    .iter()
                    .filter_map(|p| regex::Regex::new(p).ok())
                    .collect::<Vec<_>>();
                let mut sealed_event = event.clone();
                let persisted = database.as_mut().map(|db| db.append(&mut sealed_event));
                let durable = matches!(persisted, Some(Ok(())));
                if !durable {
                    redact::event(&mut event, &patterns);
                    if let Some(Err(error)) = persisted
                        && let Ok(mut s) = status.lock()
                    {
                        s.error = Some(error.to_string());
                    }
                    event.temporary = true;
                    sequence += 1;
                    event.sequence = sequence;
                    let bytes = serde_json::to_vec(&event)
                        .map(|b| b.len())
                        .unwrap_or_default();
                    temporary_bytes += bytes;
                    temporary.push_back((event, bytes));
                    while temporary.len() > 512 || temporary_bytes > 16 * 1024 * 1024 {
                        if let Some((_, bytes)) = temporary.pop_front() {
                            temporary_bytes = temporary_bytes.saturating_sub(bytes);
                        } else {
                            break;
                        }
                    }
                }
                if let Ok(mut s) = status.lock() {
                    s.durable = durable && s.error.is_none();
                    if let Some(db) = &database {
                        s.bytes = db.bytes();
                    }
                }
                if let Some(ack) = ack {
                    let _ = ack.send(durable && status.lock().is_ok_and(|s| s.durable));
                }
            }
            Command::Page(filter, reply) => {
                let result = database
                    .as_ref()
                    .map(|db| db.page(&filter))
                    .unwrap_or_else(|| Ok(Page::default()))
                    .map(|mut page| {
                        page.events.extend(
                            temporary
                                .iter()
                                .rev()
                                .map(|(e, _)| e)
                                .filter(|e| matches_filter(e, &filter))
                                .cloned(),
                        );
                        page.events.sort_by_key(|e| std::cmp::Reverse(e.sequence));
                        let limit = filter.limit.clamp(1, 200);
                        if page.events.len() > limit {
                            page.events.truncate(limit);
                            page.next = page.events.last().map(|e| e.sequence);
                        }
                        page
                    });
                if let Err(error) = &result
                    && let Ok(mut s) = status.lock()
                {
                    s.error = Some(error.to_string());
                    s.durable = false;
                }
                let _ = reply.send(result);
            }
            Command::Configure(settings, reply) => {
                let result = database
                    .as_mut()
                    .ok_or_else(|| anyhow!("AUDIT_UNAVAILABLE"))
                    .and_then(|db| db.configure(settings.clone()));
                if result.is_ok()
                    && let Ok(mut s) = status.lock()
                {
                    s.settings = settings;
                }
                let _ = reply.send(result);
            }
            Command::Clear(reply) => {
                let result = database
                    .as_mut()
                    .ok_or_else(|| anyhow!("AUDIT_UNAVAILABLE"))
                    .and_then(|db| db.clear());
                if result.is_ok() {
                    temporary.clear();
                    temporary_bytes = 0;
                    if let Ok(mut s) = status.lock() {
                        s.error = None;
                        s.durable = database.is_some();
                        s.bytes = database.as_ref().map(|db| db.bytes()).unwrap_or_default();
                    }
                }
                let _ = reply.send(result);
            }
            Command::Summary(conversation, reply) => {
                let result = database
                    .as_ref()
                    .ok_or_else(|| anyhow!("AUDIT_UNAVAILABLE"))
                    .and_then(|db| db.summary(conversation));
                let _ = reply.send(result);
            }
            Command::Retry(reply) => {
                let result = (|| {
                    if database.is_none() {
                        database = Some(database::Database::open(&directory, key)?);
                    }
                    let db = database
                        .as_mut()
                        .ok_or_else(|| anyhow!("AUDIT_UNAVAILABLE"))?;
                    db.page(&Filter {
                        limit: 200,
                        ..Filter::default()
                    })?;
                    if let Some(instance) = &profile_instance {
                        db.start_profile(&directory, instance)?;
                    }
                    for (event, _) in &temporary {
                        let mut event = event.clone();
                        event.temporary = false;
                        event.sequence = 0;
                        db.append(&mut event)?;
                    }
                    db.append(&mut Event::new(Context::new(Source::System),"audit_recovered","audit","recorded",serde_json::json!({"temporary_records":temporary.len(),"possible_gap":true})))?;
                    temporary.clear();
                    temporary_bytes = 0;
                    if let Ok(mut s) = status.lock() {
                        s.error = None;
                        s.durable = true;
                        s.bytes = db.bytes();
                        s.settings = db.settings.clone();
                    }
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            Command::StartProfile(instance) => {
                profile_instance = Some(instance.clone());
                let result = database
                    .as_mut()
                    .ok_or_else(|| anyhow!("AUDIT_UNAVAILABLE"))
                    .and_then(|db| db.start_profile(&directory, &instance));
                if let Err(error) = result
                    && let Ok(mut s) = status.lock()
                {
                    s.error = Some(error.to_string());
                    s.durable = false;
                }
            }
            Command::Close(reply) => {
                drop(database.take());
                if let Ok(mut s) = status.lock() {
                    s.durable = false;
                    s.error = Some("AUDIT_UNAVAILABLE".into());
                }
                let _ = reply.send(());
                break;
            }
        }
        if changes_data {
            stamp.fetch_add(1, Ordering::Relaxed);
        }
    }
}
fn matches_filter(e: &Event, f: &Filter) -> bool {
    f.source.is_none_or(|s| e.context.source == s)
        && f.category.is_none_or(|c| e.category == c)
        && f.conversation_id
            .as_ref()
            .is_none_or(|id| e.context.conversation_id.as_ref() == Some(id))
        && f.target_id
            .as_ref()
            .is_none_or(|id| e.context.target_id.as_ref() == Some(id))
        && f.client_id
            .as_ref()
            .is_none_or(|id| e.context.client_id.as_ref() == Some(id))
        && f.operation_id.as_ref().is_none_or(|id| {
            &e.context.operation_id == id || e.context.parent_id.as_ref() == Some(id)
        })
        && f.state.as_ref().is_none_or(|s| &e.state == s)
        && f.since_ms.is_none_or(|at| e.timestamp_ms >= at)
        && f.until_ms.is_none_or(|at| e.timestamp_ms <= at)
        && f.before_sequence.is_none_or(|seq| e.sequence < seq)
        && (!f.approvals_only || e.kind.starts_with("approval_") || e.kind.starts_with("budget_"))
        && (!f.model_requests_only || e.details["request"].is_object())
        && (f.search.is_empty()
            || serde_json::to_string(e)
                .is_ok_and(|s| s.to_lowercase().contains(&f.search.to_lowercase())))
}
