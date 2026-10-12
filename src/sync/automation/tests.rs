use super::*;
use crate::{
    ai::{Model, Provider},
    sync::{
        MergeLocal, MergedConfig, PrivacyPasswordStatus,
        protocol::{CURRENT_FORMAT_VERSION, V3SyncPayload},
        reconcile_three_way,
    },
};

fn ai() -> AiConfiguration {
    let provider = Provider {
        name: "Synthetic".into(),
        base_url: "https://model.example.test/v1".into(),
        models: vec![Model {
            id: "fixture".into(),
            supports_tools: true,
        }],
        default_model: "fixture".into(),
        ..Provider::default()
    };
    AiConfiguration {
        settings: Settings {
            providers: vec![provider.clone()],
            default_provider: Some(provider.id.clone()),
            ..Settings::default()
        },
        keys: BTreeMap::from([(provider.id, "synthetic-api-secret".into())]),
    }
}

fn config(ai: AiConfiguration) -> MergedConfig {
    MergedConfig {
        automation: AutomationConfig {
            ai: Some(ai),
            mcp: Some(McpConfiguration::capture(
                Policy::default(),
                AiPolicy::default(),
            )),
            ..AutomationConfig::default()
        },
        ..MergedConfig::default()
    }
}

fn local(config: &MergedConfig) -> MergeLocal<'_> {
    MergeLocal {
        sessions: &config.sessions,
        deleted_sessions: &config.deleted_sessions,
        connection_groups: &config.connection_groups,
        deleted_connection_groups: &config.deleted_connection_groups,
        keys: &config.managed_keys,
        commands: &config.quick_command_categories,
        base_payload: config.base_payload.as_ref(),
        automation: Some(&config.automation),
    }
}

#[test]
fn ai_keys_are_encrypted_even_when_ssh_secret_sync_is_disabled() -> Result<()> {
    let configuration = config(ai());
    let payload = V3SyncPayload::from_config(
        "device-a".into(),
        &configuration,
        false,
        "sync-password",
        None,
    )?;
    assert_eq!(payload.schema_version, CURRENT_FORMAT_VERSION);
    assert_eq!(
        payload.privacy_password_status("sync-password")?,
        PrivacyPasswordStatus::Verified
    );
    assert_eq!(
        payload.privacy_password_status("incorrect-password")?,
        PrivacyPasswordStatus::Mismatch
    );
    let bytes = crate::sync::serialize_payload(&payload)?;
    assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-api-secret"));
    let parsed = crate::sync::parse_payload(&bytes)?;
    let decoded = crate::sync::merge::decode_payload(parsed, "sync-password");
    assert!(decoded.automation.ai == configuration.automation.ai);
    let wrong = crate::sync::merge::decode_payload(payload, "wrong-password");
    assert_eq!(wrong.unavailable_secret_count, 1);
    assert!(wrong.automation.ai.is_some_and(|ai| ai.keys.is_empty()));
    Ok(())
}

#[test]
fn ciphertext_cannot_be_rebound_to_a_different_provider_or_url() -> Result<()> {
    let configuration = ai();
    let mut exported = configuration.export("sync-password", None)?;
    exported.settings.providers[0].base_url = "https://other.example.test/v1".into();
    let (resolved, unavailable) = exported.resolve(Some(&configuration), Some("sync-password"));
    assert_eq!(unavailable, 1);
    assert!(resolved.keys.is_empty());
    let mut exported = configuration.export("sync-password", None)?;
    let old_id = exported.settings.providers[0].id.clone();
    let new_id = uuid::Uuid::new_v4().to_string();
    exported.settings.providers[0].id = new_id.clone();
    let ciphertext = exported
        .keys
        .remove(&old_id)
        .ok_or_else(|| anyhow::anyhow!("missing fixture key"))?;
    exported.keys.insert(new_id, ciphertext);
    assert_eq!(exported.resolve(None, Some("sync-password")).1, 1);
    Ok(())
}

#[test]
fn unchanged_ai_configuration_reuses_ciphertext_and_password_verifier() -> Result<()> {
    let configuration = config(ai());
    let base =
        V3SyncPayload::from_config("a".into(), &configuration, false, "sync-password", None)?;
    let next = V3SyncPayload::from_config(
        "b".into(),
        &configuration,
        false,
        "sync-password",
        Some(&base),
    )?;
    assert!(base.is_content_equivalent(&next));
    let rotated = V3SyncPayload::from_config(
        "a".into(),
        &configuration,
        false,
        "new-password",
        Some(&base),
    )?;
    assert_eq!(
        rotated.privacy_password_status("new-password")?,
        PrivacyPasswordStatus::Verified
    );
    assert_eq!(
        rotated
            .ai
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing AI"))?
            .value
            .resolve(None, Some("sync-password"))
            .1,
        1
    );
    Ok(())
}

#[test]
fn ai_and_mcp_changes_merge_independently_and_same_ai_edits_conflict() -> Result<()> {
    let base_config = config(ai());
    let base = V3SyncPayload::from_config("a".into(), &base_config, false, "sync-password", None)?;
    let mut changed_local = base_config.clone();
    changed_local
        .automation
        .ai
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("missing AI"))?
        .settings
        .execution
        .tool_calls = 32;
    changed_local.automation.expected_ai_stamp = Some(9);
    let mut changed_remote = base_config.clone();
    changed_remote
        .automation
        .mcp
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("missing MCP"))?
        .mode = Mode::Controlled;
    let remote = V3SyncPayload::from_config(
        "b".into(),
        &changed_remote,
        false,
        "sync-password",
        Some(&base),
    )?;
    let merged = reconcile_three_way(local(&changed_local), &base, remote, "sync-password")?;
    assert!(merged.conflicts.is_empty());
    assert_eq!(
        merged
            .merged
            .automation
            .ai
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing AI"))?
            .settings
            .execution
            .tool_calls,
        32
    );
    assert_eq!(
        merged
            .merged
            .automation
            .mcp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing MCP"))?
            .mode,
        Mode::Controlled
    );
    assert_eq!(merged.merged.automation.expected_ai_stamp, Some(9));
    changed_remote
        .automation
        .ai
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("missing AI"))?
        .settings
        .execution
        .tool_calls = 48;
    let remote = V3SyncPayload::from_config(
        "b".into(),
        &changed_remote,
        false,
        "sync-password",
        Some(&base),
    )?;
    let mut merged = reconcile_three_way(local(&changed_local), &base, remote, "sync-password")?;
    assert_eq!(merged.conflicts.len(), 1);
    assert_eq!(
        merged.conflicts[0].kind(),
        crate::sync::SyncEntityKind::AiSettings
    );
    assert!(!format!("{:?}", merged.conflicts[0]).contains("synthetic-api-secret"));
    merged.resolve_all(crate::sync::ConflictResolution::Remote)?;
    assert_eq!(merged.merged.automation.expected_ai_stamp, Some(9));
    Ok(())
}

#[test]
fn legacy_payloads_cannot_delete_new_domains_and_unknown_versions_are_rejected() -> Result<()> {
    let configuration = config(ai());
    let base =
        V3SyncPayload::from_config("a".into(), &configuration, false, "sync-password", None)?;
    let mut legacy = base.clone();
    legacy.schema_version = 3;
    legacy.ai = None;
    legacy.mcp = None;
    let parsed = crate::sync::parse_payload(&crate::sync::serialize_payload(&legacy)?)?;
    let merged = reconcile_three_way(local(&configuration), &base, parsed, "sync-password")?;
    assert!(merged.conflicts.is_empty());
    assert!(merged.merged.automation.ai == configuration.automation.ai);
    legacy.schema_version = 5;
    assert!(crate::sync::parse_payload(&crate::sync::serialize_payload(&legacy)?).is_err());
    Ok(())
}

#[test]
fn first_sync_restores_remote_configuration_to_an_empty_device() -> Result<()> {
    let mut configuration = config(ai());
    configuration
        .automation
        .mcp
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("missing MCP"))?
        .enabled = true;
    let remote =
        V3SyncPayload::from_config("a".into(), &configuration, false, "sync-password", None)?;
    let empty = config(AiConfiguration {
        settings: Settings::default(),
        keys: BTreeMap::new(),
    });
    let merged =
        crate::sync::merge_payload_for_upload_with_deleted(local(&empty), remote, "sync-password");
    assert!(merged.automation.ai == configuration.automation.ai);
    assert!(merged.automation.mcp == configuration.automation.mcp);
    Ok(())
}

#[test]
fn no_password_exports_only_settings_and_unavailable_keys_preserve_local_values() -> Result<()> {
    let configuration = ai();
    let public = configuration.export("", None)?;
    assert!(public.keys.values().all(|key| *key == SyncSecret::Omitted));
    let (resolved, unavailable) = public.resolve(None, None);
    assert!(resolved.settings == configuration.settings);
    assert!(resolved.keys.is_empty());
    assert_eq!(unavailable, 0);
    let mut encrypted = configuration.export("sync-password", None)?;
    let (resolved, unavailable) = encrypted.resolve(Some(&configuration), None);
    assert!(resolved.keys == configuration.keys);
    assert_eq!(unavailable, 1);
    let id = configuration.settings.providers[0].id.clone();
    encrypted.keys.insert(
        id.clone(),
        SyncSecret::LegacyPlaintext("unsafe-secret".into()),
    );
    let (resolved, unavailable) = encrypted.resolve(Some(&configuration), Some("sync-password"));
    assert!(resolved.keys == configuration.keys);
    assert_eq!(unavailable, 1);
    encrypted.keys.insert(id.clone(), SyncSecret::Empty);
    let (resolved, unavailable) = encrypted.resolve(Some(&configuration), Some("sync-password"));
    assert_eq!(resolved.keys.get(&id).map(String::as_str), Some(""));
    assert_eq!(unavailable, 0);
    Ok(())
}

#[test]
fn clearing_keys_and_removing_all_providers_are_explicit_edits() -> Result<()> {
    let configuration = config(ai());
    let base =
        V3SyncPayload::from_config("a".into(), &configuration, false, "sync-password", None)?;
    let mut cleared = configuration.clone();
    let ai = cleared
        .automation
        .ai
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("missing AI"))?;
    let id = ai.settings.providers[0].id.clone();
    ai.keys.insert(id.clone(), String::new());
    let merged = reconcile_three_way(local(&cleared), &base, base.clone(), "sync-password")?;
    assert!(merged.conflicts.is_empty());
    let payload = V3SyncPayload::from_config(
        "a".into(),
        &merged.merged,
        false,
        "sync-password",
        Some(&base),
    )?;
    let decoded = crate::sync::merge::decode_payload(payload, "sync-password");
    assert_eq!(
        decoded
            .automation
            .ai
            .as_ref()
            .and_then(|ai| ai.keys.get(&id))
            .map(String::as_str),
        Some("")
    );
    cleared.automation.ai = Some(AiConfiguration {
        settings: Settings::default(),
        keys: BTreeMap::new(),
    });
    let merged = reconcile_three_way(local(&cleared), &base, base.clone(), "sync-password")?;
    assert!(merged.conflicts.is_empty());
    assert!(
        merged
            .merged
            .automation
            .ai
            .is_some_and(|ai| ai.settings.providers.is_empty())
    );
    Ok(())
}
