use super::*;
use tiny_shell_mcp::policy::Mode;

#[tokio::test]
async fn conversation_mode_is_local_persistent_and_can_return_to_global() -> Result<()> {
    let directory = std::env::temp_dir().join(format!(
        "tiny-shell-permission-test-{}",
        uuid::Uuid::new_v4()
    ));
    let service = tests::fixture(
        directory.clone(),
        tests::provider("127.0.0.1:1".parse()?),
        None,
    )?;
    let id = service.create(None)?;
    let other = service.create(None)?;
    service.mutate(&id, |c| {
        c.messages.push(Message::text("user", "fixture"));
        Ok(())
    })?;
    service.set_conversation_mode(&id, Some(Mode::Full))?;
    tests::settled(&service).await?;
    let snapshot = service.snapshot();
    assert_eq!(
        snapshot
            .conversations
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .mode_override,
        Some(Mode::Full)
    );
    assert_eq!(
        snapshot
            .conversations
            .iter()
            .find(|c| c.id == other)
            .unwrap()
            .mode_override,
        None
    );
    let path = directory.join("ai/conversations").join(format!("{id}.enc"));
    let restore = || -> Result<Conversation> {
        Ok(serde_json::from_slice(&super::super::storage::unseal(
            &[9; 32],
            &id,
            &std::fs::read(&path)?,
        )?)?)
    };
    assert_eq!(restore()?.mode_override, Some(Mode::Full));
    service.set_conversation_mode(&id, None)?;
    tests::settled(&service).await?;
    assert_eq!(restore()?.mode_override, None);
    service.audit.close().await?;
    drop(service);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[tokio::test]
async fn every_active_phase_and_turn_cleanup_reject_permission_changes() -> Result<()> {
    let directory = std::env::temp_dir().join(format!(
        "tiny-shell-permission-test-{}",
        uuid::Uuid::new_v4()
    ));
    let service = tests::fixture(
        directory.clone(),
        tests::provider("127.0.0.1:1".parse()?),
        None,
    )?;
    let id = service.create(None)?;
    for phase in [
        TurnState::Generating,
        TurnState::RunningTool,
        TurnState::AwaitingApproval,
        TurnState::AwaitingContinuation,
    ] {
        service.mutate(&id, |c| {
            c.state = phase;
            Ok(())
        })?;
        assert_eq!(
            service
                .set_conversation_mode(&id, Some(Mode::Full))
                .unwrap_err()
                .to_string(),
            "AI_BUSY"
        );
        assert_eq!(service.snapshot().conversations[0].mode_override, None);
    }
    service.mutate(&id, |c| {
        c.state = TurnState::Idle;
        Ok(())
    })?;
    let (cancel, _) = watch::channel(false);
    service
        .state
        .lock()
        .unwrap()
        .cancel
        .insert(id.clone(), cancel);
    assert_eq!(
        service
            .set_conversation_mode(&id, Some(Mode::Full))
            .unwrap_err()
            .to_string(),
        "AI_BUSY"
    );
    service.audit.close().await?;
    drop(service);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn old_history_defaults_to_global_and_explicit_choices_round_trip() -> Result<()> {
    let conversation = Conversation::new(None, &Settings::default());
    let mut old = serde_json::to_value(&conversation)?;
    old.as_object_mut().unwrap().remove("mode_override");
    assert_eq!(
        serde_json::from_value::<Conversation>(old)?.mode_override,
        None
    );
    for mode in [
        None,
        Some(Mode::ReadOnly),
        Some(Mode::Controlled),
        Some(Mode::Full),
    ] {
        let mut c = conversation.clone();
        c.mode_override = mode;
        assert_eq!(
            serde_json::from_slice::<Conversation>(&serde_json::to_vec(&c)?)?.mode_override,
            mode
        );
    }
    Ok(())
}
