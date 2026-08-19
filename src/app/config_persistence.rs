use std::{
    collections::HashSet,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::session::config::ConfigStore;

/// Storage backend used by [`ConfigRepository`].
///
/// Keeping file-system access behind this boundary makes repository lifecycle
/// and failure behavior testable without touching the user's configuration.
pub(crate) trait ConfigIo: Send + Sync + 'static {
    fn load(&self) -> anyhow::Result<ConfigStore>;
    fn save(&self, config: &ConfigStore) -> anyhow::Result<()>;
}

#[derive(Debug, Default)]
pub(crate) struct ConfigStoreIo;

impl ConfigIo for ConfigStoreIo {
    fn load(&self) -> anyhow::Result<ConfigStore> {
        ConfigStore::load()
    }

    fn save(&self, config: &ConfigStore) -> anyhow::Result<()> {
        config.save()
    }
}

type Io = Arc<dyn ConfigIo>;

type Reply = mpsc::Sender<anyhow::Result<()>>;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
const WORKER_JOIN_POLL_INTERVAL: Duration = Duration::from_millis(10);
const SAVE_QUEUE_CAPACITY: usize = 32;

fn receive_reply(reply: mpsc::Receiver<anyhow::Result<()>>) -> anyhow::Result<()> {
    reply
        .recv_timeout(OPERATION_TIMEOUT)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => anyhow::anyhow!("config save operation timed out"),
            mpsc::RecvTimeoutError::Disconnected => anyhow::anyhow!("config save worker stopped"),
        })?
}

enum SaveRequest {
    Preferences {
        source: ConfigStore,
        include_body_panels: bool,
        reply: Reply,
    },
    Full {
        config: ConfigStore,
        reply: Reply,
    },
    Flush(Reply),
    Shutdown(Reply),
}

pub(crate) struct SaveReceipt {
    reply: mpsc::Receiver<anyhow::Result<()>>,
}

impl SaveReceipt {
    pub(crate) fn wait(self) -> anyhow::Result<()> {
        receive_reply(self.reply)
    }

    pub(crate) fn try_result(&self) -> Option<anyhow::Result<()>> {
        match self.reply.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("config save worker stopped")))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RepositoryState {
    Running,
    ShuttingDown,
    Stopped,
}

pub(crate) type WindowLeaseId = u64;

pub(crate) struct ConfigRepository {
    sender: mpsc::SyncSender<SaveRequest>,
    state: Mutex<RepositoryState>,
    worker: Mutex<Option<JoinHandle<()>>>,
    windows: Mutex<HashSet<WindowLeaseId>>,
    next_window_id: Mutex<WindowLeaseId>,
}

impl ConfigRepository {
    pub(crate) fn new() -> Arc<Self> {
        Self::with_io(Arc::new(ConfigStoreIo))
    }

    pub(crate) fn with_io(io: Arc<dyn ConfigIo>) -> Arc<Self> {
        let (sender, receiver) = mpsc::sync_channel(SAVE_QUEUE_CAPACITY);
        let worker_io = Arc::clone(&io);
        let worker = match thread::Builder::new()
            .name("tiny-shell-config-save".to_string())
            .spawn(move || worker_loop(worker_io, receiver))
        {
            Ok(worker) => worker,
            Err(error) => {
                tracing::error!("failed to start config save worker: {error}");
                return Arc::new(Self {
                    sender,
                    state: Mutex::new(RepositoryState::Stopped),
                    worker: Mutex::new(None),
                    windows: Mutex::new(HashSet::new()),
                    next_window_id: Mutex::new(0),
                });
            }
        };

        Arc::new(Self {
            sender,
            state: Mutex::new(RepositoryState::Running),
            worker: Mutex::new(Some(worker)),
            windows: Mutex::new(HashSet::new()),
            next_window_id: Mutex::new(0),
        })
    }

    pub(crate) fn register_window(&self) -> anyhow::Result<WindowLeaseId> {
        let mut next = self
            .next_window_id
            .lock()
            .map_err(|_| anyhow::anyhow!("config repository window id lock is poisoned"))?;
        *next = next
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("config repository window id exhausted"))?;
        let id = *next;
        self.windows
            .lock()
            .map_err(|_| anyhow::anyhow!("config repository window lock is poisoned"))?
            .insert(id);
        Ok(id)
    }

    pub(crate) fn close_window(&self, id: WindowLeaseId) -> anyhow::Result<WindowCloseResult> {
        let is_last = {
            let mut windows = self
                .windows
                .lock()
                .map_err(|_| anyhow::anyhow!("config repository window lock is poisoned"))?;
            if !windows.remove(&id) {
                return Ok(WindowCloseResult::AlreadyClosed);
            }
            windows.is_empty()
        };
        if is_last {
            self.shutdown()?;
            Ok(WindowCloseResult::ShutDown)
        } else {
            self.flush()?;
            Ok(WindowCloseResult::Flushed)
        }
    }

    pub(crate) fn persist_async(
        self: &Arc<Self>,
        source: ConfigStore,
    ) -> anyhow::Result<SaveReceipt> {
        self.persist_preferences_async(source, false)
    }

    pub(crate) fn persist_workspace_layout_async(
        self: &Arc<Self>,
        source: ConfigStore,
    ) -> anyhow::Result<SaveReceipt> {
        self.persist_preferences_async(source, true)
    }

    fn persist_preferences_async(
        self: &Arc<Self>,
        source: ConfigStore,
        include_body_panels: bool,
    ) -> anyhow::Result<SaveReceipt> {
        let (reply, result) = mpsc::channel();
        self.enqueue_async(SaveRequest::Preferences {
            source,
            include_body_panels,
            reply,
        })?;
        Ok(SaveReceipt { reply: result })
    }

    pub(crate) fn save_full_async(
        self: &Arc<Self>,
        config: ConfigStore,
    ) -> anyhow::Result<SaveReceipt> {
        let (reply, result) = mpsc::channel();
        self.enqueue_async(SaveRequest::Full { config, reply })?;
        Ok(SaveReceipt { reply: result })
    }

    pub(crate) fn save_full(&self, config: &ConfigStore) -> anyhow::Result<()> {
        let (reply, result) = mpsc::channel();
        self.enqueue_blocking(SaveRequest::Full {
            config: config.clone(),
            reply,
        })?;
        receive_reply(result)
    }

    pub(crate) fn persist_sync(&self, source: &ConfigStore) -> anyhow::Result<()> {
        self.persist_preferences_sync(source, false)
    }

    pub(crate) fn persist_workspace_layout_sync(&self, source: &ConfigStore) -> anyhow::Result<()> {
        self.persist_preferences_sync(source, true)
    }

    fn persist_preferences_sync(
        &self,
        source: &ConfigStore,
        include_body_panels: bool,
    ) -> anyhow::Result<()> {
        let (reply, result) = mpsc::channel();
        self.enqueue_blocking(SaveRequest::Preferences {
            source: source.clone(),
            include_body_panels,
            reply,
        })?;
        receive_reply(result)
    }

    pub(crate) fn flush(&self) -> anyhow::Result<()> {
        let (reply, result) = mpsc::channel();
        self.enqueue_blocking(SaveRequest::Flush(reply))?;
        receive_reply(result)
    }

    pub(crate) fn shutdown(&self) -> anyhow::Result<()> {
        let should_send_shutdown = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("config repository state lock is poisoned"))?;
            match *state {
                RepositoryState::Running => {
                    *state = RepositoryState::ShuttingDown;
                    true
                }
                RepositoryState::ShuttingDown => false,
                RepositoryState::Stopped => {
                    return Err(anyhow::anyhow!("config repository is already shut down"));
                }
            }
        };

        let operation_result = if should_send_shutdown {
            let (reply, result) = mpsc::channel();
            self.enqueue_unchecked_blocking(SaveRequest::Shutdown(reply))
                .and_then(|()| receive_reply(result))
        } else {
            Ok(())
        };
        let join_result = self.join_worker();
        if join_result.is_ok()
            && let Ok(mut state) = self.state.lock()
        {
            *state = RepositoryState::Stopped;
        }
        operation_result.and(join_result)
    }

    fn ensure_running(&self) -> anyhow::Result<()> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("config repository state lock is poisoned"))?;
        if *state != RepositoryState::Running {
            return Err(anyhow::anyhow!("config repository is shut down"));
        }
        Ok(())
    }

    fn enqueue_async(&self, request: SaveRequest) -> anyhow::Result<()> {
        self.ensure_running()?;
        self.sender.try_send(request).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => {
                anyhow::anyhow!("config save queue is temporarily full")
            }
            mpsc::TrySendError::Disconnected(_) => {
                anyhow::anyhow!("config save worker is unavailable")
            }
        })
    }

    fn enqueue_blocking(&self, request: SaveRequest) -> anyhow::Result<()> {
        self.ensure_running()?;
        self.enqueue_unchecked_blocking(request)
    }

    fn enqueue_unchecked_blocking(&self, request: SaveRequest) -> anyhow::Result<()> {
        self.sender
            .send(request)
            .map_err(|_| anyhow::anyhow!("config save worker is unavailable"))
    }

    fn join_worker(&self) -> anyhow::Result<()> {
        let worker = self
            .worker
            .lock()
            .map_err(|_| anyhow::anyhow!("config worker lock is poisoned"))?
            .take();
        let Some(worker) = worker else {
            return Ok(());
        };
        let deadline = Instant::now() + OPERATION_TIMEOUT;
        while !worker.is_finished() {
            if Instant::now() >= deadline {
                self.worker
                    .lock()
                    .map_err(|_| anyhow::anyhow!("config worker lock is poisoned"))?
                    .replace(worker);
                return Err(anyhow::anyhow!("config save worker shutdown timed out"));
            }
            thread::sleep(WORKER_JOIN_POLL_INTERVAL);
        }
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("config save worker panicked"))?;
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CloseErrorReport {
    failures: Vec<(&'static str, String)>,
}

impl CloseErrorReport {
    pub(crate) fn record(&mut self, stage: &'static str, error: impl ToString) {
        self.failures.push((stage, error.to_string()));
    }

    pub(crate) fn log(&self) {
        for (stage, error) in &self.failures {
            tracing::error!(target: "tiny_shell::window_close", close_stage = *stage, error = %error, "window close persistence failed");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowCloseResult {
    Flushed,
    ShutDown,
    AlreadyClosed,
}

fn config_fingerprint(config: &ConfigStore) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(&config.cache)
        .map_err(|error| anyhow::anyhow!("failed to fingerprint config: {error}"))
}

fn save_full_with_revision(
    io: &Io,
    last_saved: &mut Option<ConfigStore>,
    source: ConfigStore,
) -> anyhow::Result<()> {
    if let Some(previous) = last_saved.as_ref() {
        let current = io.load()?;
        let current_fingerprint = config_fingerprint(&current)?;
        let previous_fingerprint = config_fingerprint(previous)?;
        let source_fingerprint = config_fingerprint(&source)?;
        // Optimistic concurrency: if the file changed since the worker's last
        // write, only an identical snapshot may be committed. In particular,
        // writer identity must not allow a stale window to overwrite newer
        // preference changes from another window.
        if current_fingerprint != previous_fingerprint && current_fingerprint != source_fingerprint
        {
            return Err(anyhow::anyhow!(
                "config changed in another window; refusing stale full-config overwrite"
            ));
        }
    }
    io.save(&source)?;
    *last_saved = Some(source);
    Ok(())
}

fn worker_loop(io: Io, receiver: mpsc::Receiver<SaveRequest>) {
    let mut last_saved: Option<ConfigStore> = None;
    let mut last_error: Option<String> = None;
    while let Ok(request) = receiver.recv() {
        let (result, reply, should_stop, records_failure) = match request {
            SaveRequest::Preferences {
                source,
                include_body_panels,
                reply,
            } => {
                let result = io.load().and_then(|mut config| {
                    config.merge_interactive_preferences_from(&source);
                    if include_body_panels {
                        config.set_monitoring_position(source.monitoring_position());
                        config.set_body_panels(source.body_panels().cloned());
                    }
                    io.save(&config)?;
                    last_saved = Some(config);
                    Ok(())
                });
                (result, reply, false, true)
            }
            SaveRequest::Full { config, reply } => (
                save_full_with_revision(&io, &mut last_saved, config),
                reply,
                false,
                true,
            ),
            SaveRequest::Flush(reply) => {
                let result = last_error
                    .take()
                    .map_or(Ok(()), |error| Err(anyhow::anyhow!(error)));
                (result, reply, false, false)
            }
            SaveRequest::Shutdown(reply) => {
                let result = last_error
                    .take()
                    .map_or(Ok(()), |error| Err(anyhow::anyhow!(error)));
                (result, reply, true, false)
            }
        };
        if records_failure {
            if let Err(error) = &result {
                last_error = Some(error.to_string());
                tracing::warn!("failed to save config in background: {error:#}");
            } else {
                // Keep an earlier failure pending until a caller observes it via
                // flush/shutdown. A later successful write must not erase the
                // error before the close path has had a chance to report it.
            }
        }
        let _ = reply.send(result);
        if should_stop {
            break;
        }
    }
}

pub(crate) fn persist_sync(
    repository: &Arc<ConfigRepository>,
    source: &ConfigStore,
) -> anyhow::Result<()> {
    repository.persist_sync(source)
}

pub(crate) fn persist_workspace_layout_sync(
    repository: &Arc<ConfigRepository>,
    source: &ConfigStore,
) -> anyhow::Result<()> {
    repository.persist_workspace_layout_sync(source)
}

pub(crate) fn save_full(
    repository: &Arc<ConfigRepository>,
    config: &ConfigStore,
) -> anyhow::Result<()> {
    repository.save_full(config)
}

pub(crate) fn save_full_async(
    repository: &Arc<ConfigRepository>,
    config: &ConfigStore,
) -> anyhow::Result<SaveReceipt> {
    repository.save_full_async(config.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    struct TestIo {
        config: Mutex<ConfigStore>,
        fail_save: AtomicBool,
        saves: AtomicUsize,
        history: Mutex<Vec<String>>,
    }

    impl ConfigIo for TestIo {
        fn load(&self) -> anyhow::Result<ConfigStore> {
            Ok(self
                .config
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone())
        }
        fn save(&self, config: &ConfigStore) -> anyhow::Result<()> {
            self.saves.fetch_add(1, Ordering::SeqCst);
            self.history
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(config.locale().to_string());
            if self.fail_save.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("injected save failure"));
            }
            *self.config.lock().unwrap_or_else(|p| p.into_inner()) = config.clone();
            Ok(())
        }
    }

    fn repository() -> (Arc<ConfigRepository>, Arc<TestIo>) {
        let io = Arc::new(TestIo {
            config: Mutex::new(ConfigStore::in_memory()),
            fail_save: AtomicBool::new(false),
            saves: AtomicUsize::new(0),
            history: Mutex::new(Vec::new()),
        });
        (ConfigRepository::with_io(io.clone()), io)
    }

    #[test]
    fn close_error_report_preserves_all_failure_stages() {
        let mut report = CloseErrorReport::default();
        report.record("preferences", "preference save failed");
        report.record("layout", "layout save failed");
        report.record("close_window", "worker shutdown failed");
        assert_eq!(report.failures.len(), 3);
        assert_eq!(report.failures[0].0, "preferences");
        assert_eq!(report.failures[2].0, "close_window");
    }

    #[test]
    fn lifecycle_shares_worker_and_closes_only_after_last_window() {
        let (repository, io) = repository();
        let first = repository.register_window().unwrap();
        let second = repository.register_window().unwrap();

        let mut config = ConfigStore::in_memory();
        config.set_locale("zh-CN");
        repository.save_full_async(config).unwrap().wait().unwrap();
        assert_eq!(
            repository.close_window(first).unwrap(),
            WindowCloseResult::Flushed
        );

        let mut later_config = ConfigStore::in_memory();
        later_config.set_locale("en");
        repository
            .save_full_async(later_config)
            .unwrap()
            .wait()
            .unwrap();
        assert_eq!(io.saves.load(Ordering::SeqCst), 2);
        assert_eq!(
            repository.close_window(first).unwrap(),
            WindowCloseResult::AlreadyClosed
        );
        assert_eq!(
            repository.close_window(second).unwrap(),
            WindowCloseResult::ShutDown
        );
        assert_eq!(
            repository.close_window(second).unwrap(),
            WindowCloseResult::AlreadyClosed
        );
    }

    #[test]
    fn persisted_backend_survives_repository_recreation() {
        let (repository, io) = repository();
        let mut config = ConfigStore::in_memory();
        config.set_locale("zh-CN");
        repository.save_full_async(config).unwrap().wait().unwrap();
        repository.shutdown().unwrap();

        let recreated = ConfigRepository::with_io(io.clone());
        let restored = io
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert_eq!(restored.locale(), "zh-CN");
        recreated.shutdown().unwrap();
    }

    #[test]
    fn failed_flush_is_reported_once_then_recovery_can_shutdown() {
        let (repository, io) = repository();
        io.fail_save.store(true, Ordering::SeqCst);
        repository
            .save_full_async(ConfigStore::in_memory())
            .unwrap()
            .wait()
            .unwrap_err();
        assert!(repository.flush().is_err());
        assert!(repository.flush().is_ok());

        io.fail_save.store(false, Ordering::SeqCst);
        repository.save_full(&ConfigStore::in_memory()).unwrap();
        repository.shutdown().unwrap();
    }

    #[test]
    fn different_repositories_are_isolated_while_windows_share_one() {
        let (first, first_io) = repository();
        let (second, second_io) = repository();
        let first_window = first.register_window().unwrap();
        let second_window = first.register_window().unwrap();
        let other_window = second.register_window().unwrap();

        first.save_full(&ConfigStore::in_memory()).unwrap();
        assert_eq!(first_io.saves.load(Ordering::SeqCst), 1);
        assert_eq!(second_io.saves.load(Ordering::SeqCst), 0);
        assert_eq!(
            first.close_window(first_window).unwrap(),
            WindowCloseResult::Flushed
        );
        assert_eq!(
            first.close_window(second_window).unwrap(),
            WindowCloseResult::ShutDown
        );
        assert_eq!(
            second.close_window(other_window).unwrap(),
            WindowCloseResult::ShutDown
        );
    }

    #[test]
    fn instances_are_isolated() {
        let (first, first_io) = repository();
        let (second, second_io) = repository();
        let mut config = ConfigStore::in_memory();
        config.set_locale("zh-CN");
        first.save_full(&config).unwrap();
        assert_eq!(first_io.saves.load(Ordering::SeqCst), 1);
        assert_eq!(second_io.saves.load(Ordering::SeqCst), 0);
        first.shutdown().unwrap();
        second.shutdown().unwrap();
    }

    #[test]
    fn save_failure_is_observable_and_flush_is_barrier() {
        let (repository, io) = repository();
        io.fail_save.store(true, Ordering::SeqCst);
        let config = ConfigStore::in_memory();
        assert!(repository.save_full_async(config).unwrap().wait().is_err());
        assert!(repository.flush().is_err());
        repository.shutdown().unwrap();
    }

    #[test]
    fn stale_full_config_is_rejected_after_external_window_change() {
        let (repository, io) = repository();
        let mut baseline = ConfigStore::in_memory();
        baseline.set_locale("en");
        repository.save_full(&baseline).unwrap();

        let mut external = baseline.clone();
        external.set_locale("zh-CN");
        *io.config.lock().unwrap_or_else(|p| p.into_inner()) = external;

        let mut stale = baseline;
        stale.set_locale("ja");
        assert!(repository.save_full(&stale).is_err());
        assert!(repository.shutdown().is_err());
    }

    #[test]
    fn successful_save_does_not_clear_unobserved_failure() {
        let (repository, io) = repository();
        io.fail_save.store(true, Ordering::SeqCst);
        repository
            .save_full_async(ConfigStore::in_memory())
            .unwrap()
            .wait()
            .unwrap_err();
        io.fail_save.store(false, Ordering::SeqCst);
        repository.save_full(&ConfigStore::in_memory()).unwrap();
        assert!(repository.shutdown().is_err());
    }

    #[test]
    fn shutdown_reports_unobserved_final_save_error_and_joins_worker() {
        let (repository, io) = repository();
        io.fail_save.store(true, Ordering::SeqCst);
        let receipt = repository
            .save_full_async(ConfigStore::in_memory())
            .unwrap();
        assert!(receipt.wait().is_err());
        assert!(repository.shutdown().is_err());
        assert!(repository.flush().is_err());
    }

    #[test]
    fn shutdown_joins_worker_and_rejects_repeated_shutdown() {
        let (repository, _) = repository();
        repository.shutdown().unwrap();
        assert!(repository.shutdown().is_err());
        assert!(repository.flush().is_err());
    }

    #[test]
    fn save_requests_preserve_order_and_full_save_wins() {
        let (repository, io) = repository();
        let mut preferences = ConfigStore::in_memory();
        preferences.set_locale("zh-CN");
        let preference_receipt = repository.persist_async(preferences).unwrap();

        let mut full = ConfigStore::in_memory();
        full.set_locale("en");
        let full_receipt = repository.save_full_async(full).unwrap();

        preference_receipt.wait().unwrap();
        full_receipt.wait().unwrap();
        repository.flush().unwrap();
        assert_eq!(
            *io.history.lock().unwrap_or_else(|p| p.into_inner()),
            vec!["zh-CN".to_string(), "en".to_string()]
        );
        assert_eq!(
            io.config.lock().unwrap_or_else(|p| p.into_inner()).locale(),
            "en"
        );
        repository.shutdown().unwrap();
    }

    #[test]
    fn final_save_content_is_available_after_shutdown() {
        let (repository, io) = repository();
        let mut config = ConfigStore::in_memory();
        config.set_locale("zh-CN");
        repository.save_full_async(config).unwrap().wait().unwrap();
        repository.shutdown().unwrap();
        assert_eq!(
            io.config.lock().unwrap_or_else(|p| p.into_inner()).locale(),
            "zh-CN"
        );
    }

    #[test]
    fn workspace_layout_preferences_persist_monitoring_and_body_height_together() {
        let (repository, io) = repository();
        let mut baseline = ConfigStore::in_memory();
        baseline.set_locale("en");
        baseline.set_monitoring_position("Bottom");
        baseline.set_body_panels(Some(vec![420.0, 328.0]));
        repository.save_full(&baseline).unwrap();

        let mut stale_window = baseline.clone();
        let mut changed = baseline;
        changed.set_locale("zh-CN");
        changed.set_monitoring_position("Sidebar");
        changed.set_body_panels(Some(vec![420.0, 248.0]));
        repository
            .persist_workspace_layout_async(changed)
            .unwrap()
            .wait()
            .unwrap();

        stale_window.set_locale("ja");
        repository
            .persist_async(stale_window)
            .unwrap()
            .wait()
            .unwrap();

        let restored = io
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert_eq!(restored.locale(), "ja");
        assert_eq!(restored.monitoring_position(), "Sidebar");
        assert_eq!(restored.body_panels(), Some(&vec![420.0, 248.0]));
        repository.shutdown().unwrap();
    }
}
