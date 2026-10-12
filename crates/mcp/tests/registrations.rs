use tiny_shell_mcp::{
    launch::format_client_config,
    storage::{RegistrationOutcome, Store},
};

#[test]
fn rename_persists_the_name_without_changing_access_or_configuration() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    let client = store.register("Codex CLI")?;
    let config = serde_json::json!({"mcpServers":{"tiny-shell":{"command":"tiny-shell","args":["--mcp","--client",client.id]}}}).to_string();
    let before = format_client_config(&client.label, &config)?;
    store.rename_client(&client.id, "  工作电脑 Codex  ")?;

    let reloaded = Store::new(directory.path())?.client(&client.id)?;
    assert_eq!(reloaded.name(), "工作电脑 Codex");
    assert_eq!(reloaded.label, client.label);
    assert_eq!(reloaded.id, client.id);
    assert_eq!(reloaded.secret, client.secret);
    assert_eq!(reloaded.enabled, client.enabled);
    assert_eq!(format_client_config(&reloaded.label, &config)?, before);
    assert!(!store.load_policy()?.enabled);
    Ok(())
}

#[test]
fn names_reject_empty_control_and_overlong_input_without_changing_storage() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    let client = store.register("MCP Client")?;
    let path = store
        .root()
        .join("clients")
        .join(format!("{}.json", client.id));
    let before = std::fs::read(&path)?;
    for invalid in [
        "".to_owned(),
        "   ".to_owned(),
        "a\nb".to_owned(),
        "a\0b".to_owned(),
        "名".repeat(129),
    ] {
        assert!(store.rename_client(&client.id, &invalid).is_err());
        assert_eq!(std::fs::read(&path)?, before);
    }
    store.rename_client(&client.id, &"名".repeat(128))?;
    assert_eq!(store.client(&client.id)?.name(), "名".repeat(128));
    Ok(())
}

#[test]
fn legacy_registrations_keep_their_template_name() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    let client = store.register("OpenCode")?;
    let path = store
        .root()
        .join("clients")
        .join(format!("{}.json", client.id));
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    assert!(value.get("display_name").is_none());
    assert_eq!(store.client(&client.id)?.name(), "OpenCode");
    assert!(store.client(&client.id)?.display_name.is_none());
    Ok(())
}

#[test]
fn renaming_does_not_restore_revoked_access() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    let client = store.register("Claude Code")?;
    store.revoke(&client.id)?;
    store.rename_client(&client.id, "旧电脑")?;
    let renamed = store.client(&client.id)?;
    assert_eq!(renamed.name(), "旧电脑");
    assert!(!renamed.enabled);
    assert_eq!(renamed.secret, client.secret);
    Ok(())
}

#[test]
fn returning_to_the_template_name_removes_the_alias() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    let client = store.register("MCP Client")?;
    store.rename_client(&client.id, "工作区")?;
    assert!(store.client(&client.id)?.display_name.is_some());
    store.rename_client(&client.id, "MCP Client")?;
    assert!(store.client(&client.id)?.display_name.is_none());
    assert_eq!(store.client(&client.id)?.name(), "MCP Client");
    Ok(())
}

#[test]
fn renamed_instances_coexist_and_repeated_add_reuses_only_the_default() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    assert!(matches!(
        store.register_or_reuse("Codex CLI")?,
        RegistrationOutcome::Created(_)
    ));
    assert!(matches!(
        store.register_or_reuse("Codex CLI")?,
        RegistrationOutcome::Existing(_)
    ));
    let first = store.clients()?.remove(0);
    store.rename_client(&first.id, "工作电脑")?;
    let RegistrationOutcome::Created(second) = store.register_or_reuse("Codex CLI")? else {
        anyhow::bail!("a renamed registration must allow another instance");
    };
    assert_ne!(first.id, second.id);
    assert!(matches!(
        store.register_or_reuse("Codex CLI")?,
        RegistrationOutcome::Existing(_)
    ));
    assert_eq!(store.clients()?.len(), 2);
    assert_eq!(store.client(&first.id)?.name(), "工作电脑");
    assert_eq!(store.client(&second.id)?.name(), "Codex CLI");
    store.revoke(&second.id)?;
    assert!(matches!(
        store.register_or_reuse("Codex CLI")?,
        RegistrationOutcome::Created(_)
    ));
    assert_eq!(store.clients()?.iter().filter(|c| c.enabled).count(), 2);
    Ok(())
}

#[test]
fn concurrent_rename_and_revoke_preserve_revocation() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    for _ in 0..16 {
        let client = store.register("MCP Client")?;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let rename_store = store.clone();
        let rename_id = client.id.clone();
        let rename_barrier = barrier.clone();
        let renamer = std::thread::spawn(move || {
            rename_barrier.wait();
            rename_store.rename_client(&rename_id, "工作电脑")
        });
        let revoke_store = store.clone();
        let revoke_id = client.id.clone();
        let revoke_barrier = barrier.clone();
        let revoker = std::thread::spawn(move || {
            revoke_barrier.wait();
            revoke_store.revoke(&revoke_id)
        });
        barrier.wait();
        renamer
            .join()
            .map_err(|_| anyhow::anyhow!("rename thread panicked"))??;
        revoker
            .join()
            .map_err(|_| anyhow::anyhow!("revoke thread panicked"))??;
        let reloaded = store.client(&client.id)?;
        assert_eq!(reloaded.name(), "工作电脑");
        assert!(!reloaded.enabled);
    }
    Ok(())
}
