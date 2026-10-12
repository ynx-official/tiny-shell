use super::tests::{fixture, provider};
use super::*;
use std::time::Duration;

async fn paused(service: &Service) -> Result<Continuation> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(pause) = service.snapshot().conversations[0].continuation.clone() {
                return pause;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(Into::into)
}

#[tokio::test]
async fn continuation_is_single_use_and_does_not_accept_a_stale_choice() -> Result<()> {
    let directory = std::env::temp_dir().join(format!("ai-pause-{}", uuid::Uuid::new_v4()));
    let service = fixture(directory.clone(), provider("127.0.0.1:1".parse()?), None)?;
    let id = service.create(None)?;
    let (cancel, mut receiver) = watch::channel(false);
    service
        .state
        .lock()
        .map_err(|_| anyhow!("lock"))?
        .cancel
        .insert(id.clone(), cancel);
    let worker = service.clone();
    let target = id.clone();
    let task = tokio::spawn(async move {
        worker
            .wait_for_continuation(&target, LimitKind::ToolCalls, 16, 16, None, &mut receiver)
            .await
    });
    let pause = paused(&service).await?;
    assert!(service.snapshot().conversations[0].state.active());
    assert!(service.resume(&id, "expired").is_err());
    assert!(service.resume(&id, &pause.token).is_ok());
    assert!(service.resume(&id, &pause.token).is_err());
    task.await??;
    assert!(service.snapshot().conversations[0].continuation.is_none());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[tokio::test]
async fn stopping_a_paused_turn_wakes_the_wait_and_cannot_be_overridden() -> Result<()> {
    let directory = std::env::temp_dir().join(format!("ai-pause-{}", uuid::Uuid::new_v4()));
    let service = fixture(directory.clone(), provider("127.0.0.1:1".parse()?), None)?;
    let id = service.create(None)?;
    let (cancel, mut receiver) = watch::channel(false);
    service
        .state
        .lock()
        .map_err(|_| anyhow!("lock"))?
        .cancel
        .insert(id.clone(), cancel);
    let worker = service.clone();
    let target = id.clone();
    let task = tokio::spawn(async move {
        worker
            .wait_for_continuation(&target, LimitKind::ModelRequests, 8, 8, None, &mut receiver)
            .await
    });
    let pause = paused(&service).await?;
    service.stop(&id);
    assert!(service.resume(&id, &pause.token).is_err());
    assert_eq!(task.await?.unwrap_err().to_string(), "AI_CANCELLED");
    assert!(service.snapshot().conversations[0].continuation.is_none());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn old_settings_keep_finite_defaults_and_invalid_budgets_are_rejected() -> Result<()> {
    let settings: Settings = serde_json::from_str(r#"{"providers":[],"default_provider":null}"#)?;
    assert_eq!(settings.execution.tool_calls, 16);
    assert_eq!(settings.execution.model_requests, 8);
    assert!(!settings.execution.background_notifications);
    let mut execution = settings.execution;
    execution.tool_calls = 0;
    assert!(execution.validate().is_err());
    execution.tool_calls = 129;
    assert!(execution.validate().is_err());
    execution.tool_calls = 16;
    execution.model_requests = 65;
    assert!(execution.validate().is_err());
    Ok(())
}

#[tokio::test]
async fn rejection_is_single_use_and_prevents_a_later_allow() -> Result<()> {
    let directory = std::env::temp_dir().join(format!("ai-pause-{}", uuid::Uuid::new_v4()));
    let service = fixture(directory.clone(), provider("127.0.0.1:1".parse()?), None)?;
    let id = service.create(None)?;
    let (cancel, mut receiver) = watch::channel(false);
    service
        .state
        .lock()
        .map_err(|_| anyhow!("lock"))?
        .cancel
        .insert(id.clone(), cancel);
    let worker = service.clone();
    let target = id.clone();
    let task = tokio::spawn(async move {
        worker
            .wait_for_continuation(&target, LimitKind::ToolCalls, 16, 16, None, &mut receiver)
            .await
    });
    let pause = paused(&service).await?;
    service.terminate_continuation(&id, &pause.token)?;
    assert!(service.terminate_continuation(&id, &pause.token).is_err());
    assert!(service.resume(&id, &pause.token).is_err());
    assert_eq!(task.await?.unwrap_err().to_string(), "AI_CANCELLED");
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[tokio::test]
async fn a_choice_is_bound_to_its_conversation() -> Result<()> {
    let directory = std::env::temp_dir().join(format!("ai-pause-{}", uuid::Uuid::new_v4()));
    let service = fixture(directory.clone(), provider("127.0.0.1:1".parse()?), None)?;
    let first = service.create(None)?;
    let second = service.create(None)?;
    let (cancel, mut receiver) = watch::channel(false);
    service
        .state
        .lock()
        .map_err(|_| anyhow!("lock"))?
        .cancel
        .insert(first.clone(), cancel);
    let worker = service.clone();
    let target = first.clone();
    let task = tokio::spawn(async move {
        worker
            .wait_for_continuation(&target, LimitKind::ToolCalls, 16, 16, None, &mut receiver)
            .await
    });
    let pause = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(pause) = service
                .attention()
                .pending
                .into_iter()
                .find(|p| p.conversation == first)
            {
                break pause.choice;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert!(service.resume(&second, &pause.token).is_err());
    assert!(
        service
            .terminate_continuation(&second, &pause.token)
            .is_err()
    );
    assert!(service.continuation_live(&first, &pause.token));
    service.resume(&first, &pause.token)?;
    task.await??;
    assert!(!service.continuation_live(&first, &pause.token));
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
