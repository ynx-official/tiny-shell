use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tiny_shell_mcp::{
    contracts::{Call, Failure, Result},
    engine::{AiPolicy, Engine, Remote, Target},
    policy::{Mode, Policy},
    storage::Store,
};
use tokio::sync::watch;

struct NoExecution;
#[async_trait]
impl Remote for NoExecution {
    async fn targets(&self) -> Result<Vec<Target>> {
        Ok(Vec::new())
    }
    async fn execute(
        &self,
        _: &Target,
        _: &str,
        _: &Call,
        _: watch::Receiver<bool>,
    ) -> Result<Value> {
        Err(Failure::new("UNEXPECTED_EXECUTION"))
    }
    async fn close(&self, _: &str) {}
}

#[tokio::test]
async fn stale_cloud_permissions_cannot_overwrite_new_local_permissions() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    let audit = tiny_shell_mcp::audit::Store::with_key(directory.path().to_owned(), [23; 32]);
    let engine = Engine::with_audit(store, Arc::new(NoExecution), audit)?;
    let stale = engine.configuration_stamp();
    engine
        .set_policy(Policy {
            enabled: true,
            mode: Mode::Controlled,
            ..Policy::default()
        })
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert!(engine.set_synced_policy(Policy::default(), stale).is_err());
    assert!(
        engine
            .set_synced_ai_policy(
                AiPolicy {
                    enabled: true,
                    mode: Mode::Full
                },
                stale
            )
            .is_err()
    );
    let actual = engine.policy().map_err(|e| anyhow::anyhow!(e.code))?;
    assert!(actual.enabled);
    assert_eq!(actual.mode, Mode::Controlled);
    assert!(!engine.ai_enabled());
    engine
        .set_synced_ai_policy(
            AiPolicy {
                enabled: true,
                mode: Mode::Controlled,
            },
            engine.configuration_stamp(),
        )
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert!(engine.ai_enabled());
    assert_eq!(
        engine
            .ai_policy()
            .map_err(|e| anyhow::anyhow!(e.code))?
            .mode,
        Mode::Controlled
    );
    engine.audit.close().await?;
    Ok(())
}
