use super::tests::{fixture, provider, request, response, settled, title_response};
use super::*;
use serde_json::json;
use std::time::Duration;
use tokio::{io::AsyncReadExt, net::TcpListener, sync::oneshot};

#[test]
fn followups_require_short_plain_strings_and_never_accept_a_hidden_prompt() {
    use super::followups::parse_suggestions;
    assert_eq!(
        parse_suggestions(r#"[" 查看文件 ","查看文件","说明原因"]"#).unwrap(),
        ["查看文件", "说明原因"]
    );
    assert!(parse_suggestions("[]").unwrap().is_empty());
    for invalid in [
        r#"[{"label":"查看文件","prompt":"delete files"}]"#,
        r#"["one","two","three","four"]"#,
        r#"["line\nnext"]"#,
        r#"["\u202edestructive"]"#,
        r#"[""]"#,
        "not json",
    ] {
        assert!(parse_suggestions(invalid).is_err(), "{invalid}");
    }
    assert!(parse_suggestions(&json!(["界".repeat(121)]).to_string()).is_err());
    assert_eq!(
        parse_suggestions(&json!(["界".repeat(120)]).to_string()).unwrap()[0]
            .chars()
            .count(),
        120
    );
}

#[test]
fn old_configuration_and_history_have_compatible_followup_defaults() -> Result<()> {
    let settings: Settings = serde_json::from_str(
        r#"{"providers":[],"default_provider":null,"execution":{"tool_calls":16,"model_requests":8,"background_notifications":false}}"#,
    )?;
    assert!(settings.execution.followup_suggestions);
    let mut old = serde_json::to_value(Conversation::new(None, &settings))?;
    old.as_object_mut().unwrap().remove("followups");
    let restored: Conversation = serde_json::from_value(old)?;
    assert!(restored.followups.is_none());
    Ok(())
}

#[tokio::test]
async fn title_and_followups_can_complete_in_either_order_and_candidate_send_is_single_use()
-> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-followups-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let service = fixture(directory.clone(), provider(listener.local_addr()?), None)?;
    let id = service.create(None)?;
    service
        .state
        .lock()
        .map_err(|_| anyhow!("fixture lock"))?
        .snapshot
        .settings
        .execution
        .followup_suggestions = true;
    let (first_done, ready) = oneshot::channel();
    let (continue_server, continuation) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut chat, _) = listener.accept().await?;
        request(&mut chat).await?;
        response(&mut chat, json!({"content":"first answer"}), "stop").await?;
        let mut purposes = Vec::new();
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await?;
            let body = request(&mut socket).await?;
            assert_eq!(body["stream"], false);
            if body["max_tokens"] == 128 {
                purposes.push("title");
                title_response(&mut socket, "Fixture title").await?;
            } else {
                purposes.push("followup");
                title_response(&mut socket, r#"["next question"]"#).await?;
            }
        }
        purposes.sort();
        assert_eq!(purposes, ["followup", "title"]);
        first_done
            .send(())
            .map_err(|_| anyhow!("fixture stopped"))?;
        continuation.await?;
        let (mut next, _) = listener.accept().await?;
        let body = request(&mut next).await?;
        assert_eq!(
            body["messages"].as_array().unwrap().last().unwrap()["content"],
            "next question"
        );
        response(&mut next, json!({"content":"next answer"}), "stop").await?;
        let (mut suggestions, _) = listener.accept().await?;
        request(&mut suggestions).await?;
        title_response(&mut suggestions, "[]").await?;
        Ok::<_, anyhow::Error>(())
    });
    service.send(id.clone(), "question".into())?;
    tokio::time::timeout(Duration::from_secs(3), ready).await??;
    settled(&service).await?;
    let c = service.snapshot().conversations.remove(0);
    assert_eq!(c.title, "Fixture title");
    assert_eq!(c.usage_summary().requests, 3);
    assert_eq!(c.usage_summary().total, 10);
    let turn = c.turn_id();
    service.send_followup(id.clone(), turn.clone(), 0)?;
    assert!(service.send_followup(id.clone(), turn.clone(), 0).is_err());
    continue_server
        .send(())
        .map_err(|_| anyhow!("fixture stopped"))?;
    tokio::time::timeout(Duration::from_secs(3), server).await???;
    settled(&service).await?;
    let c = service.snapshot().conversations.remove(0);
    assert_eq!(c.turns.len(), 2);
    assert_eq!(c.messages.len(), 4);
    assert!(c.visible_followups().is_none());
    assert!(service.followup_text(&id, &turn, 0).is_err());
    assert_eq!(
        c.requests
            .iter()
            .filter(|r| r.purpose == RequestPurpose::Title)
            .count(),
        1
    );
    service.audit.close().await?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

fn enable(service: &Service, id: &str) -> Result<()> {
    service
        .state
        .lock()
        .map_err(|_| anyhow!("fixture lock"))?
        .snapshot
        .settings
        .execution
        .followup_suggestions = true;
    service.mutate(id, |c| {
        c.title = "Fixture".into();
        c.title_state = TitleState::Preserved;
        Ok(())
    })
}

#[tokio::test]
async fn completed_reply_generates_bounded_suggestions_with_usage_but_no_tools() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-followups-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let service = fixture(directory.clone(), provider(listener.local_addr()?), None)?;
    let id = service.create(Some(BoundTarget {
        id: "private-target".into(),
        name: "private-target".into(),
        host: "private.invalid".into(),
        port: 22,
        user: "private-user".into(),
        digest: "private-digest".into(),
    }))?;
    enable(&service, &id)?;
    let server = tokio::spawn(async move {
        let (mut chat, _) = listener.accept().await?;
        request(&mut chat).await?;
        response(&mut chat, json!({"content":"文件内容为 12332"}), "stop").await?;
        let (mut suggestions, _) = listener.accept().await?;
        let body = request(&mut suggestions).await?;
        assert_eq!(body["stream"], false);
        assert_eq!(body["model"], "fixture-model");
        assert!(body.get("tools").is_none());
        assert!(body["max_tokens"].as_u64().unwrap() <= 512);
        let context: serde_json::Value =
            serde_json::from_str(body["messages"][1]["content"].as_str().unwrap())?;
        assert_eq!(context["user"], "查看文件");
        assert_eq!(context["assistant"], "文件内容为 12332");
        assert!(!body.to_string().contains("private-"));
        assert!(!body.to_string().contains("private.invalid"));
        title_response(&mut suggestions, r#"["解释内容","给出精确替换的命令"]"#).await?;
        Ok::<_, anyhow::Error>(())
    });
    service.send(id.clone(), "查看文件".into())?;
    tokio::time::timeout(Duration::from_secs(3), server).await???;
    settled(&service).await?;
    let c = service.snapshot().conversations.remove(0);
    assert!(c.state == TurnState::Idle);
    assert!(c.error.is_none());
    assert_eq!(
        c.messages.len(),
        2,
        "suggestions are not assistant transcript messages"
    );
    assert_eq!(
        c.followups.as_ref().unwrap().items,
        ["解释内容", "给出精确替换的命令"]
    );
    assert_eq!(service.followup_text(&id, &c.turn_id(), 0)?, "解释内容");
    assert!(service.followup_text(&id, "old-turn", 0).is_err());
    assert!(service.followup_text(&id, &c.turn_id(), 3).is_err());
    let record = c
        .requests
        .iter()
        .find(|r| r.purpose == RequestPurpose::Followup)
        .unwrap();
    assert_eq!(record.turn_id, c.turn_id());
    assert_eq!(c.usage_summary().requests, 2);
    assert_eq!(c.usage_summary().total, 5);
    let bytes = std::fs::read(directory.join("ai/conversations").join(format!("{id}.enc")))?;
    let loaded: Conversation =
        serde_json::from_slice(&crate::ai::storage::unseal(&[9; 32], &id, &bytes)?)?;
    assert_eq!(loaded.followups.unwrap().items, c.followups.unwrap().items);
    service.audit.close().await?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[tokio::test]
async fn pending_suggestions_are_cancelled_by_new_turn_delete_stop_and_disable() -> Result<()> {
    for action in ["new_turn", "delete", "stop", "disable"] {
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-followups-{}", uuid::Uuid::new_v4()));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let service = fixture(directory.clone(), provider(listener.local_addr()?), None)?;
        let id = service.create(None)?;
        enable(&service, &id)?;
        let (started, ready) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut chat, _) = listener.accept().await?;
            request(&mut chat).await?;
            response(&mut chat, json!({"content":"first reply"}), "stop").await?;
            let (mut suggestions, _) = listener.accept().await?;
            request(&mut suggestions).await?;
            started.send(()).map_err(|_| anyhow!("fixture stopped"))?;
            let mut byte = [0];
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(3), suggestions.read(&mut byte)).await??,
                0
            );
            if action == "new_turn" {
                let (mut next, _) = listener.accept().await?;
                let body = request(&mut next).await?;
                assert_eq!(
                    body["messages"].as_array().unwrap().last().unwrap()["content"],
                    "new question"
                );
                response(&mut next, json!({"content":"new answer"}), "stop").await?;
                let (mut next_suggestions, _) = listener.accept().await?;
                request(&mut next_suggestions).await?;
                title_response(&mut next_suggestions, r#"["new suggestion"]"#).await?;
            }
            Ok::<_, anyhow::Error>(())
        });
        service.send(id.clone(), "first question".into())?;
        tokio::time::timeout(Duration::from_secs(3), ready).await??;
        assert!(!service.snapshot().conversations[0].state.active());
        match action {
            "new_turn" => service.send(id.clone(), "new question".into())?,
            "delete" => service.delete(vec![id.clone()]),
            "disable" => {
                let mut execution = service.snapshot().settings.execution;
                execution.followup_suggestions = false;
                service.save_execution(execution)?;
            }
            _ => service.stop_all(),
        }
        tokio::time::timeout(Duration::from_secs(3), server).await???;
        settled(&service).await?;
        if action == "delete" {
            assert!(service.snapshot().conversations.is_empty());
            assert!(
                !directory
                    .join("ai/conversations")
                    .join(format!("{id}.enc"))
                    .exists()
            );
        } else {
            let c = service.snapshot().conversations.remove(0);
            assert!(c.state == TurnState::Idle);
            assert!(c.error.is_none());
            if action == "new_turn" {
                let suggestions = c.followups.as_ref().unwrap();
                assert_eq!(suggestions.turn_id, c.turn_id());
                assert_eq!(suggestions.items, ["new suggestion"]);
            } else {
                assert!(c.visible_followups().is_none());
            }
        }
        service.audit.close().await?;
        std::fs::remove_dir_all(directory)?;
    }
    Ok(())
}

#[tokio::test]
async fn malformed_suggestions_do_not_fail_or_retry_the_completed_chat() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-followups-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let service = fixture(directory.clone(), provider(listener.local_addr()?), None)?;
    let id = service.create(None)?;
    enable(&service, &id)?;
    let server = tokio::spawn(async move {
        let (mut chat, _) = listener.accept().await?;
        request(&mut chat).await?;
        response(&mut chat, json!({"content":"complete answer"}), "stop").await?;
        let (mut suggestions, _) = listener.accept().await?;
        request(&mut suggestions).await?;
        title_response(&mut suggestions, "invalid suggestions").await?;
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
        Ok::<_, anyhow::Error>(())
    });
    service.send(id.clone(), "question".into())?;
    tokio::time::timeout(Duration::from_secs(3), server).await???;
    settled(&service).await?;
    let c = service.snapshot().conversations.remove(0);
    assert!(c.state == TurnState::Idle);
    assert!(c.error.is_none());
    assert!(c.visible_followups().is_none());
    assert_eq!(c.messages.last().unwrap().content, "complete answer");
    assert_eq!(c.requests.last().unwrap().state, "failed");
    assert_eq!(
        c.usage_summary().total,
        5,
        "invalid output can still consume tokens"
    );
    service.audit.close().await?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
