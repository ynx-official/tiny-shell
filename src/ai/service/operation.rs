use tokio::sync::oneshot;

pub(super) type Completion = oneshot::Sender<Result<(), String>>;

/// A single request's result, independent of the mutable settings-page notice.
pub(crate) struct OperationReceipt(oneshot::Receiver<Result<(), String>>);

impl OperationReceipt {
    pub(super) fn channel() -> (Completion, Self) {
        let (sender, receiver) = oneshot::channel();
        (sender, Self(receiver))
    }

    pub(crate) async fn wait(self) -> Result<(), String> {
        self.0
            .await
            .unwrap_or_else(|_| Err("AI_INTERRUPTED".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn receipt_waits_for_completion_and_keeps_its_own_result() {
        let (first, receipt) = OperationReceipt::channel();
        let (second, later) = OperationReceipt::channel();
        let wait = receipt.wait();
        tokio::pin!(wait);
        assert!(futures::poll!(&mut wait).is_pending());
        assert!(first.send(Err("AI_STORAGE_ERROR".into())).is_ok());
        assert!(second.send(Ok(())).is_ok());
        assert_eq!(wait.await, Err("AI_STORAGE_ERROR".into()));
        assert_eq!(later.wait().await, Ok(()));
    }

    #[tokio::test]
    async fn abandoned_operation_never_reports_success() {
        let (sender, receipt) = OperationReceipt::channel();
        drop(sender);
        assert_eq!(receipt.wait().await, Err("AI_INTERRUPTED".into()));
    }

    #[tokio::test]
    async fn settings_receipt_reports_disk_failure_and_only_succeeds_after_persistence()
    -> anyhow::Result<()> {
        use crate::ai::{Provider, Settings};
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-feedback-{}", uuid::Uuid::new_v4()));
        let service = super::super::tests::fixture(directory.clone(), Provider::default(), None)?;
        let path = directory.join("ai/providers.json");
        // A directory at the destination causes a deterministic write failure on every platform.
        std::fs::create_dir(&path)?;
        let mut execution = service.snapshot().settings.execution;
        execution.tool_calls += 1;
        let failed = service.save_execution(execution.clone())?;
        assert!(failed.wait().await.is_err());
        assert_ne!(
            service.snapshot().settings.execution.tool_calls,
            execution.tool_calls
        );
        std::fs::remove_dir(&path)?;
        let completed = service.save_execution(execution.clone())?;
        assert_eq!(completed.wait().await, Ok(()));
        let persisted: Settings = serde_json::from_slice(&std::fs::read(&path)?)?;
        assert_eq!(persisted.execution.tool_calls, execution.tool_calls);
        service.audit.close().await?;
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}
