use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use crate::session::config::ConfigStore;

struct ConfigRepository {
    save_sequence: AtomicU64,
    save_lock: Mutex<()>,
    save_tx: OnceLock<mpsc::Sender<(u64, SaveRequest)>>,
}

enum SaveRequest {
    Preferences(ConfigStore),
    Full(ConfigStore),
    Flush(mpsc::Sender<anyhow::Result<()>>),
}

static REPOSITORY: OnceLock<Arc<ConfigRepository>> = OnceLock::new();

fn repository() -> Arc<ConfigRepository> {
    REPOSITORY
        .get_or_init(|| {
            Arc::new(ConfigRepository {
                save_sequence: AtomicU64::new(0),
                save_lock: Mutex::new(()),
                save_tx: OnceLock::new(),
            })
        })
        .clone()
}

impl ConfigRepository {
    fn next_sequence(&self) -> u64 {
        self.save_sequence.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn is_current(&self, sequence: u64) -> bool {
        sequence == self.save_sequence.load(Ordering::SeqCst)
    }

    fn save_sender(self: &Arc<Self>) -> mpsc::Sender<(u64, SaveRequest)> {
        self.save_tx
            .get_or_init(|| {
                let (sender, receiver) = mpsc::channel::<(u64, SaveRequest)>();
                let repository = Arc::clone(self);
                let worker = thread::Builder::new()
                    .name("tiny-shell-config-save".to_string())
                    .spawn(move || {
                        while let Ok((sequence, request)) = receiver.recv() {
                            let (mut latest_full, mut latest_preferences) = match request {
                                SaveRequest::Full(config) => (Some((sequence, config)), None),
                                SaveRequest::Preferences(config) => {
                                    (None, Some((sequence, config)))
                                }
                                SaveRequest::Flush(reply) => {
                                    let _ = reply.send(Ok(()));
                                    continue;
                                }
                            };
                            let mut latest_sequence = sequence;
                            let mut barrier = None;

                            while let Ok((next_sequence, next_source)) =
                                receiver.recv_timeout(Duration::from_millis(100))
                            {
                                match next_source {
                                    SaveRequest::Full(config) => {
                                        latest_sequence = next_sequence;
                                        latest_full = Some((next_sequence, config));
                                    }
                                    SaveRequest::Preferences(config) => {
                                        latest_sequence = next_sequence;
                                        latest_preferences = Some((next_sequence, config));
                                    }
                                    SaveRequest::Flush(reply) => {
                                        barrier = Some(reply);
                                        break;
                                    }
                                }
                            }

                            let result = if !repository.is_current(sequence)
                                || !repository.is_current(latest_sequence)
                            {
                                Ok(())
                            } else {
                                match repository.save_lock.lock() {
                                    Ok(_guard) => match (latest_full, latest_preferences) {
                                        (
                                            Some((full_sequence, mut config)),
                                            Some((preference_sequence, source)),
                                        ) if preference_sequence > full_sequence => {
                                            config.merge_interactive_preferences_from(&source);
                                            config.save()
                                        }
                                        (Some((_, config)), _) => config.save(),
                                        (None, Some((_, source))) => {
                                            ConfigStore::load().and_then(|mut config| {
                                                config.merge_interactive_preferences_from(&source);
                                                config.save()
                                            })
                                        }
                                        (None, None) => Ok(()),
                                    },
                                    Err(_) => {
                                        Err(anyhow::anyhow!("config repository lock is poisoned"))
                                    }
                                }
                            };

                            if let Err(error) = &result {
                                tracing::warn!("failed to save config in background: {error:#}");
                            }
                            if let Some(reply) = barrier {
                                let _ = reply.send(result);
                            }
                        }
                    });
                if let Err(error) = worker {
                    tracing::warn!("failed to start config save worker: {error}");
                }
                sender
            })
            .clone()
    }

    fn persist_async(self: &Arc<Self>, source: ConfigStore) {
        let sequence = self.next_sequence();
        if let Err(error) = self
            .save_sender()
            .send((sequence, SaveRequest::Preferences(source)))
        {
            tracing::warn!("failed to queue preference save: {error}");
        }
    }

    fn save_full_async(self: &Arc<Self>, config: ConfigStore) -> anyhow::Result<()> {
        let sequence = self.next_sequence();
        self.save_sender()
            .send((sequence, SaveRequest::Full(config)))
            .map_err(|_| anyhow::anyhow!("config save worker is unavailable"))
    }

    fn flush(self: &Arc<Self>) -> anyhow::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.save_sender()
            .send((
                self.save_sequence.load(Ordering::SeqCst),
                SaveRequest::Flush(reply_tx),
            ))
            .map_err(|_| anyhow::anyhow!("config save worker is unavailable"))?;
        reply_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("config save worker stopped"))?
    }

    fn persist_sync(&self, source: &ConfigStore) -> anyhow::Result<()> {
        self.next_sequence();
        let _guard = self
            .save_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("config repository lock is poisoned"))?;
        let mut config = ConfigStore::load()?;
        config.merge_interactive_preferences_from(source);
        config.save()
    }

    fn save_full_sync(&self, config: &ConfigStore) -> anyhow::Result<()> {
        self.next_sequence();
        let _guard = self
            .save_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("config repository lock is poisoned"))?;
        config.save()
    }
}

pub(crate) fn persist_async(source: ConfigStore) {
    repository().persist_async(source);
}

pub(crate) fn persist_sync(source: &ConfigStore) -> anyhow::Result<()> {
    repository().persist_sync(source)
}

pub(crate) fn save_full(config: &ConfigStore) -> anyhow::Result<()> {
    repository().save_full_sync(config)
}

pub(crate) fn save_full_async(config: &ConfigStore) -> anyhow::Result<()> {
    repository().save_full_async(config.clone())
}

pub(crate) fn flush() -> anyhow::Result<()> {
    repository().flush()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicU64};

    use super::ConfigRepository;
    use crate::session::config::ConfigStore;

    fn test_repository() -> Arc<ConfigRepository> {
        Arc::new(ConfigRepository {
            save_sequence: AtomicU64::new(0),
            save_lock: std::sync::Mutex::new(()),
            save_tx: std::sync::OnceLock::new(),
        })
    }

    #[test]
    fn save_sequence_is_monotonic_across_concurrent_writers() {
        let repository = test_repository();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let repository = Arc::clone(&repository);
                let tx = tx.clone();
                scope.spawn(move || {
                    tx.send(repository.next_sequence()).unwrap();
                });
            }
        });
        drop(tx);

        let mut sequences: Vec<_> = rx.iter().collect();
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=8).collect::<Vec<_>>());
        assert!(repository.is_current(8));
        assert!(!repository.is_current(7));
    }

    #[test]
    fn interactive_save_preserves_domain_data_and_updates_preferences() {
        let mut domain = ConfigStore::in_memory();
        domain.set_sync_connection("https://sync.example.test".to_string(), "alice".to_string());
        domain.replace_connection_groups(vec!["production".to_string()]);

        let mut preferences = ConfigStore::in_memory();
        preferences.set_locale("zh-CN");
        preferences.set_terminal_font_size(16.0);

        domain.merge_interactive_preferences_from(&preferences);

        assert_eq!(domain.locale(), "zh-CN");
        assert_eq!(domain.terminal_font_size(), 16.0);
        assert_eq!(domain.sync_endpoint(), "https://sync.example.test");
        assert_eq!(domain.sync_username(), "alice");
        assert_eq!(domain.connection_groups(), &["production".to_string()]);
    }

    #[test]
    fn full_save_invalidates_older_async_sequences() {
        let repository = test_repository();
        let async_sequence = repository.next_sequence();
        let full_save_sequence = repository.next_sequence();

        assert!(!repository.is_current(async_sequence));
        assert!(repository.is_current(full_save_sequence));
    }
}
