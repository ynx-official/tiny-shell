use super::tests::{fixture, provider, request, response, settled};
use super::*;
use serde_json::json;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

#[tokio::test]
async fn first_completed_turn_titles_in_background_without_blocking_the_next_turn() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let service = fixture(directory.clone(), provider(listener.local_addr()?), None)?;
    let id = service.create(None)?;
    let (title_started, ready) = oneshot::channel();
    let (second_replied, replied) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut chat, _) = listener.accept().await?;
        request(&mut chat).await?;
        response(&mut chat, json!({"content":"first answer"}), "stop").await?;
        let (mut title, _) = listener.accept().await?;
        let title_body = request(&mut title).await?;
        assert_eq!(title_body["stream"], false);
        assert!(title_body.get("tools").is_none());
        assert_eq!(title_body["model"], "fixture-model");
        assert_eq!(title_body["messages"].as_array().unwrap().len(), 2);
        assert!(title_body["max_tokens"].as_u64().unwrap() <= 128);
        title_started
            .send(())
            .map_err(|_| anyhow!("fixture stopped"))?;
        let (mut second, _) = listener.accept().await?;
        let body = request(&mut second).await?;
        assert_eq!(
            body["messages"].as_array().unwrap().last().unwrap()["content"],
            "第二个问题"
        );
        response(&mut second, json!({"content":"second answer"}), "stop").await?;
        second_replied
            .send(())
            .map_err(|_| anyhow!("fixture stopped"))?;
        released.await?;
        let body = json!({"choices":[{"message":{"role":"assistant","content":"\"检查 nginx 运行状态\""},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}).to_string();
        title.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
        Ok::<_, anyhow::Error>(())
    });
    service.send(id.clone(), "帮我检查 nginx 是否正在运行".into())?;
    tokio::time::timeout(Duration::from_secs(3), ready).await??;
    assert_eq!(service.snapshot().conversations[0].messages.len(), 2);
    assert!(!service.snapshot().conversations[0].state.active());
    service.send(id.clone(), "第二个问题".into())?;
    tokio::time::timeout(Duration::from_secs(3), replied).await??;
    release.send(()).map_err(|_| anyhow!("fixture stopped"))?;
    tokio::time::timeout(Duration::from_secs(3), server).await???;
    settled(&service).await?;
    let c = service.snapshot().conversations.remove(0);
    assert_eq!(c.title, "检查 nginx 运行状态");
    assert!(c.title.chars().count() <= 20);
    assert_eq!(c.messages.len(), 4);
    assert_eq!(c.turns.len(), 2);
    assert_eq!(c.usage_summary().requests, 3);
    assert_eq!(c.usage_summary().total, 5);
    let titles = c
        .requests
        .iter()
        .filter(|r| r.purpose == RequestPurpose::Title)
        .collect::<Vec<_>>();
    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0].turn_id, c.turns[0].id);
    let last_chat = c
        .requests
        .iter()
        .rev()
        .find(|r| r.purpose == RequestPurpose::Chat)
        .unwrap();
    assert_eq!(
        service.audit_context(&c).model_request_id.as_ref(),
        Some(&last_chat.id)
    );
    let stored = std::fs::read(directory.join("ai/conversations").join(format!("{id}.enc")))?;
    let loaded: Conversation =
        serde_json::from_slice(&crate::ai::storage::unseal(&[9; 32], &id, &stored)?)?;
    assert_eq!(loaded.title, c.title);
    assert_eq!(service.audit.summary(Some(id)).await?.total, 5);
    service.audit.close().await?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn generated_title_is_one_line_unicode_safe_and_old_history_is_preserved() -> Result<()> {
    assert_eq!(
        super::title::normalize_title("  标题：“检查 nginx 状态”  ").as_deref(),
        Some("检查 nginx 状态")
    );
    assert_eq!(
        super::title::normalize_title(&"界".repeat(40))
            .unwrap()
            .chars()
            .count(),
        20
    );
    assert_eq!(
        super::title::normalize_title(&"🙂".repeat(30))
            .unwrap()
            .chars()
            .count(),
        20
    );
    assert!(super::title::normalize_title(" \n ").is_none());
    assert!(super::title::normalize_title("解释内容\n实际标题").is_none());
    let c = Conversation::new(None, &Settings::default());
    let mut old = serde_json::to_value(c)?;
    old.as_object_mut().unwrap().remove("title_state");
    assert_eq!(
        serde_json::from_value::<Conversation>(old)?.title_state,
        TitleState::Preserved
    );
    Ok(())
}

#[tokio::test]
async fn manual_rename_delete_and_stop_all_cancel_a_pending_title_without_reviving_chat()
-> Result<()> {
    for action in ["rename", "delete", "stop_all"] {
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let service = fixture(directory.clone(), provider(listener.local_addr()?), None)?;
        let id = service.create(None)?;
        let (started, ready) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut chat, _) = listener.accept().await?;
            request(&mut chat).await?;
            response(&mut chat, json!({"content":"answer"}), "stop").await?;
            let (mut title, _) = listener.accept().await?;
            let body = request(&mut title).await?;
            assert_eq!(body["stream"], false);
            started.send(()).map_err(|_| anyhow!("fixture stopped"))?;
            let mut byte = [0];
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(3), title.read(&mut byte)).await??,
                0,
                "cancelling a title must close its HTTP request"
            );
            Ok::<_, anyhow::Error>(())
        });
        service.send(id.clone(), "检查 nginx 状态".into())?;
        tokio::time::timeout(Duration::from_secs(3), ready).await??;
        let original = service.snapshot().conversations[0].title.clone();
        match action {
            // Even manually confirming the same text transfers ownership to the user.
            "rename" => service.rename(id.clone(), original.clone()),
            "delete" => service.delete(vec![id.clone()]),
            _ => service.stop_all(),
        }
        server.await??;
        settled(&service).await?;
        let path = directory.join("ai/conversations").join(format!("{id}.enc"));
        if action == "delete" {
            assert!(service.snapshot().conversations.is_empty());
            assert!(!path.exists());
        } else {
            let c = service.snapshot().conversations.remove(0);
            assert_eq!(c.title, original);
            assert!(!c.state.active());
            assert!(c.error.is_none());
            assert_eq!(
                c.title_state,
                if action == "rename" {
                    TitleState::Preserved
                } else {
                    TitleState::Failed
                }
            );
            assert_eq!(
                c.requests
                    .iter()
                    .find(|r| r.purpose == RequestPurpose::Title)
                    .unwrap()
                    .state,
                "cancelled"
            );
            let loaded: Conversation = serde_json::from_slice(&crate::ai::storage::unseal(
                &[9; 32],
                &id,
                &std::fs::read(path)?,
            )?)?;
            assert_eq!(loaded.title, original);
            assert_eq!(loaded.title_state, c.title_state);
        }
        service.audit.close().await?;
        std::fs::remove_dir_all(directory)?;
    }
    Ok(())
}

#[tokio::test]
async fn preserved_titles_and_failed_first_turns_do_not_request_a_title() -> Result<()> {
    for scenario in ["manual", "legacy", "failed"] {
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let service = fixture(directory.clone(), provider(listener.local_addr()?), None)?;
        let id = service.create(None)?;
        if scenario == "manual" {
            service.rename(id.clone(), "自定义标题".into());
            settled(&service).await?;
        } else if scenario == "legacy" {
            service.mutate(&id, |c| {
                let mut data = serde_json::to_value(&*c)?;
                data.as_object_mut().unwrap().remove("title_state");
                data["title"] = "旧标题".into();
                *c = serde_json::from_value(data)?;
                Ok(())
            })?;
        }
        let failed = scenario == "failed";
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            request(&mut socket).await?;
            if failed {
                socket.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
            } else {
                response(&mut socket, json!({"content":"answer"}), "stop").await?;
            }
            assert!(
                tokio::time::timeout(Duration::from_millis(100), listener.accept())
                    .await
                    .is_err()
            );
            Ok::<_, anyhow::Error>(())
        });
        service.send(id, "检查 nginx 状态".into())?;
        settled(&service).await?;
        server.await??;
        let c = service.snapshot().conversations.remove(0);
        assert_eq!(c.requests.len(), 1);
        assert_eq!(
            c.title,
            match scenario {
                "manual" => "自定义标题",
                "legacy" => "旧标题",
                _ => "检查 nginx 状态",
            }
        );
        service.audit.close().await?;
        std::fs::remove_dir_all(directory)?;
    }
    Ok(())
}

#[tokio::test]
async fn title_failure_keeps_a_short_fallback_and_never_retries_on_the_next_turn() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let service = fixture(directory.clone(), provider(listener.local_addr()?), None)?;
    let id = service.create(None)?;
    let server = tokio::spawn(async move {
        for index in 0..3 {
            let (mut socket, _) = listener.accept().await?;
            let body = request(&mut socket).await?;
            if index == 1 {
                assert_eq!(body["stream"], false);
                socket.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
            } else {
                assert_eq!(body["stream"], true);
                response(&mut socket, json!({"content":"normal answer"}), "stop").await?;
            }
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
        Ok::<_, anyhow::Error>(())
    });
    let task = "请检查当前 nginx 服务的运行状态及监听端口和配置是否正常";
    service.send(id.clone(), task.into())?;
    settled(&service).await?;
    let c = service.snapshot().conversations.remove(0);
    assert_eq!(c.title, task.chars().take(20).collect::<String>());
    assert_eq!(c.title_state, TitleState::Failed);
    assert!(c.error.is_none());
    service.send(id.clone(), "继续回答".into())?;
    settled(&service).await?;
    server.await??;
    assert_eq!(service.snapshot().conversations[0].requests.len(), 3);
    service.audit.close().await?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
