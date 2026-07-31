use std::{
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use crate::session::config::ConfigStore;

static SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static SAVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static SAVE_TX: OnceLock<mpsc::Sender<(u64, ConfigStore)>> = OnceLock::new();

fn save_sender() -> mpsc::Sender<(u64, ConfigStore)> {
    SAVE_TX
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel::<(u64, ConfigStore)>();
            let worker = thread::Builder::new()
                .name("tiny-shell-config-save".to_string())
                .spawn(move || {
                    while let Ok((mut sequence, mut source)) = receiver.recv() {
                        while let Ok((next_sequence, next_source)) =
                            receiver.recv_timeout(Duration::from_millis(100))
                        {
                            sequence = next_sequence;
                            source = next_source;
                        }

                        let lock = SAVE_LOCK.get_or_init(|| Mutex::new(()));
                        let Ok(_guard) = lock.lock() else {
                            tracing::warn!(
                                "config save lock is poisoned; skipping background save"
                            );
                            continue;
                        };
                        if sequence != SAVE_SEQUENCE.load(Ordering::SeqCst) {
                            continue;
                        }
                        let mut config =
                            ConfigStore::load().unwrap_or_else(|_| ConfigStore::in_memory());
                        config.merge_interactive_preferences_from(&source);
                        if sequence != SAVE_SEQUENCE.load(Ordering::SeqCst) {
                            continue;
                        }
                        if let Err(error) = config.save() {
                            tracing::warn!("failed to save preferences in background: {error:#}");
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

pub(crate) fn persist_async(source: ConfigStore) {
    let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1;
    if let Err(error) = save_sender().send((sequence, source)) {
        tracing::warn!("failed to queue preference save: {error}");
    }
}

pub(crate) fn persist_sync(source: &ConfigStore) -> anyhow::Result<()> {
    SAVE_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let lock = SAVE_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock
        .lock()
        .map_err(|_| anyhow::anyhow!("config save lock is poisoned"))?;
    let mut config = ConfigStore::load().unwrap_or_else(|_| ConfigStore::in_memory());
    config.merge_interactive_preferences_from(source);
    config.save()
}

/// Persist the complete configuration through the same serialized writer used
/// by interactive preference saves. This keeps full session writes from racing
/// with a queued preference snapshot.
pub(crate) fn save_full(config: &ConfigStore) -> anyhow::Result<()> {
    SAVE_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let lock = SAVE_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock
        .lock()
        .map_err(|_| anyhow::anyhow!("config save lock is poisoned"))?;
    config.save()
}

#[cfg(test)]
mod tests {
    use super::SAVE_SEQUENCE;
    use std::sync::atomic::Ordering;

    #[test]
    fn save_sequence_always_advances() {
        let first = SAVE_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        let second = SAVE_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        assert_eq!(second, first + 1);
    }
}
