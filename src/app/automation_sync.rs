//! The application coordinates independent AI/MCP stores; protocol code never
//! touches native credentials, an IPC pairing token, history, or audit records.
use crate::sync::{
    SyncFailure, SyncResult,
    automation::{AutomationConfig, McpConfiguration},
};
use anyhow::{Result, anyhow};

pub(crate) fn capture(password: &str) -> Result<AutomationConfig> {
    let ai = super::ai::service().map_err(|error| anyhow!(error))?;
    let ai_stamp = ai.configuration_stamp();
    let ai_configuration = ai.export_sync(password)?;
    anyhow::ensure!(ai_stamp == ai.configuration_stamp(), "AI_BUSY");
    let mut mcp_stamp = None;
    let mcp = super::mcp::service()
        .ok()
        .map(|service| -> Result<_> {
            let stamp = service.engine.configuration_stamp();
            let configuration = McpConfiguration::capture(
                service.engine.policy().map_err(|e| anyhow!(e.code))?,
                service.engine.ai_policy().map_err(|e| anyhow!(e.code))?,
            );
            anyhow::ensure!(
                stamp == service.engine.configuration_stamp(),
                "POLICY_CHANGED"
            );
            mcp_stamp = Some(stamp);
            Ok(configuration)
        })
        .transpose()?;
    Ok(AutomationConfig {
        ai: Some(ai_configuration),
        mcp,
        expected_ai_stamp: Some(ai_stamp),
        expected_mcp_stamp: mcp_stamp,
    })
}

pub(crate) async fn capture_background(password: &str) -> Result<AutomationConfig> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || capture(&password))
        .await
        .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
}

fn apply(configuration: &AutomationConfig, password: &str) -> Result<()> {
    if let Some(ai) = &configuration.ai {
        super::ai::service()
            .map_err(|error| anyhow!(error))?
            .import_sync(ai, password, configuration.expected_ai_stamp)?;
    }
    if let Some(mcp) = &configuration.mcp {
        let service = super::mcp::service().map_err(|error| anyhow!(error))?;
        let _enter = service.runtime.enter();
        let mut stamp = configuration
            .expected_mcp_stamp
            .unwrap_or_else(|| service.engine.configuration_stamp());
        anyhow::ensure!(
            stamp == service.engine.configuration_stamp(),
            "POLICY_CHANGED"
        );
        let current = McpConfiguration::capture(
            service.engine.policy().map_err(|e| anyhow!(e.code))?,
            service.engine.ai_policy().map_err(|e| anyhow!(e.code))?,
        );
        if current.policy().enabled != mcp.enabled
            || current.mode != mcp.mode
            || current.scope != mcp.scope
            || current.grants != mcp.grants
            || current.known_targets != mcp.known_targets
        {
            service
                .engine
                .set_synced_policy(mcp.policy(), stamp)
                .map_err(|e| anyhow!(e.code))?;
            stamp = stamp.wrapping_add(1);
        }
        if current.ai_policy() != mcp.ai_policy() {
            service
                .engine
                .set_synced_ai_policy(mcp.ai_policy(), stamp)
                .map_err(|e| anyhow!(e.code))?;
        }
    }
    Ok(())
}

pub(crate) async fn finalize(mut result: SyncResult) -> SyncResult {
    let finalized = tokio::task::spawn_blocking(move || {
        let action = match &mut result {
            SyncResult::Uploaded {
                merged: Some(merged),
                privacy_password,
                ..
            } => apply(
                &merged.automation,
                privacy_password.as_deref().unwrap_or_default(),
            ),
            SyncResult::Downloaded {
                automation,
                ai_provider_count,
                privacy_password,
                password_status,
                payload,
                ..
            } => {
                let mut configuration = automation.clone();
                if matches!(
                    password_status,
                    crate::sync::PrivacyPasswordStatus::Missing
                        | crate::sync::PrivacyPasswordStatus::Mismatch
                ) || payload
                    .ai
                    .as_ref()
                    .is_some_and(|ai| ai.value.resolve(None, privacy_password.as_deref()).1 > 0)
                {
                    configuration.ai = None;
                }
                let applied = apply(
                    &configuration,
                    privacy_password.as_deref().unwrap_or_default(),
                );
                if applied.is_ok() {
                    *ai_provider_count = configuration
                        .ai
                        .as_ref()
                        .map(|ai| ai.settings.providers.len());
                }
                applied
            }
            SyncResult::PrivacyPasswordReset { new_password, .. } => super::ai::service()
                .map_err(|error| anyhow!(error))
                .and_then(|service| service.rotate_sync_password(new_password)),
            _ => Ok(()),
        };
        action.map(|()| result)
    })
    .await;
    match finalized {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => SyncResult::Failed(SyncFailure::other(None, error)),
        Err(_) => SyncResult::Failed(SyncFailure::other(None, "AI_STORAGE_ERROR")),
    }
}
