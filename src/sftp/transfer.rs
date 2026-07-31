use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use anyhow::Result;

use crate::terminal::{BackendEvent, BackendEventSender, TransferState};

/// Cooperative transfer control shared by upload/download tasks and their UI.
pub(crate) struct TransferStateFlag(pub(crate) Arc<AtomicU8>);

impl TransferStateFlag {
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicU8::new(0)))
    }

    pub(crate) fn pause(&self) {
        self.0.store(1, Ordering::SeqCst);
    }

    pub(crate) fn resume(&self) {
        self.0.store(0, Ordering::SeqCst);
    }

    pub(crate) fn cancel(&self) {
        self.0.store(2, Ordering::SeqCst);
    }

    pub(crate) async fn yield_if_paused(
        &self,
        events: &BackendEventSender,
        tab_id: &str,
        id: &str,
        transferred: u64,
        total: Option<u64>,
    ) -> Result<()> {
        let mut was_paused = false;
        loop {
            match self.0.load(Ordering::SeqCst) {
                2 => return Err(anyhow::anyhow!("transfer cancelled")),
                1 => {
                    if !was_paused {
                        let _ = events.send(BackendEvent::TransferProgress {
                            tab_id: tab_id.to_string(),
                            id: id.to_string(),
                            transferred,
                            total,
                            state: TransferState::Paused,
                        });
                        was_paused = true;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                _ => {
                    if was_paused {
                        let _ = events.send(BackendEvent::TransferProgress {
                            tab_id: tab_id.to_string(),
                            id: id.to_string(),
                            transferred,
                            total,
                            state: TransferState::Running,
                        });
                    }
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TransferStateFlag;
    use crate::terminal::backend_event_channel;

    #[test]
    fn cancellation_is_shared_between_clones() {
        let original = TransferStateFlag::new();
        let clone = TransferStateFlag(std::sync::Arc::clone(&original.0));
        original.cancel();
        assert_eq!(clone.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cancelled_transfer_stops_at_next_cooperative_yield() {
        let flag = TransferStateFlag::new();
        let (events, _receiver) = backend_event_channel();
        flag.cancel();

        let result = flag
            .yield_if_paused(&events, "tab", "transfer", 42, Some(100))
            .await;

        assert!(result.is_err_and(|error| error.to_string().contains("cancelled")));
    }
}
