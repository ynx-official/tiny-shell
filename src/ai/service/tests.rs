use super::*;
use serde_json::{Value, json};
use std::{sync::atomic::AtomicUsize, time::Duration};
use tiny_shell_mcp::{
    contracts::{Call, Result as McpResult},
    engine::{Engine, Remote, Target},
    policy::Mode,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Fake {
    target: Target,
    commands: AtomicUsize,
}
#[async_trait::async_trait]
impl Remote for Fake {
    async fn targets(&self) -> McpResult<Vec<Target>> {
        Ok(vec![self.target.clone()])
    }
    async fn execute(
        &self,
        _: &Target,
        id: &str,
        call: &Call,
        _: watch::Receiver<bool>,
    ) -> McpResult<Value> {
        if matches!(call, Call::RunCommand(_)) {
            self.commands.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"stdout":"fixture output","stderr":"","exit_code":0,"truncated":false}))
        } else {
            Ok(json!({"session_id":id}))
        }
    }
    async fn close(&self, _: &str) {}
}
pub(super) fn fixture(
    directory: PathBuf,
    p: Provider,
    engine: Option<Arc<Engine>>,
) -> Result<Arc<Service>> {
    let settings = Settings {
        providers: vec![p.clone()],
        default_provider: Some(p.id.clone()),
        ..Settings::default()
    };
    let store = Store::fixture(directory, Some([9; 32]), &p.id, "fixture-key")?;
    Ok(Arc::new(Service {
        state: Mutex::new(State {
            snapshot: Snapshot {
                settings,
                ready: true,
                ..Snapshot::default()
            },
            cancel: HashMap::new(),
            continuations: HashMap::new(),
        }),
        store: Mutex::new(Some(Arc::new(store))),
        persistence: Mutex::new(()),
        stamp: AtomicU64::new(1),
        runtime: tokio::runtime::Handle::current(),
        engine,
    }))
}
pub(super) fn provider(address: std::net::SocketAddr) -> Provider {
    Provider {
        name: "fixture".into(),
        base_url: format!("http://{address}/v1"),
        models: vec![Model {
            id: "fixture-model".into(),
            supports_tools: true,
        }],
        default_model: "fixture-model".into(),
        allow_http: true,
        ..Provider::default()
    }
}
pub(super) async fn request(socket: &mut tokio::net::TcpStream) -> Result<Value> {
    let mut bytes = Vec::new();
    loop {
        let mut buf = [0; 4096];
        let n = socket.read(&mut buf).await?;
        ensure!(n > 0, "request ended");
        bytes.extend_from_slice(&buf[..n]);
        ensure!(bytes.len() < 1024 * 1024, "request too large");
        if let Some(at) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..at]);
            let length = headers
                .lines()
                .find_map(|l| {
                    l.to_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|n| n.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= at + 4 + length {
                return Ok(serde_json::from_slice(&bytes[at + 4..at + 4 + length])?);
            }
        }
    }
}
pub(super) async fn response(
    socket: &mut tokio::net::TcpStream,
    delta: Value,
    reason: &str,
) -> Result<()> {
    let text = format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":delta}]}),
        json!({"choices":[{"delta":{},"finish_reason":reason}]})
    );
    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",text.len()).as_bytes()).await?;
    for chunk in text.as_bytes().chunks(7) {
        socket.write_all(chunk).await?;
    }
    Ok(())
}

pub(super) async fn settled(service: &Arc<Service>) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        while service
            .snapshot()
            .conversations
            .iter()
            .any(|c| c.state.active())
            || Arc::strong_count(service) > 1
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}

fn bound_target(name: &str) -> BoundTarget {
    BoundTarget {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        host: format!("{name}.invalid"),
        port: 22,
        user: "fixture".into(),
        digest: format!("digest-{name}"),
    }
}

#[tokio::test]
async fn text_only_conditions_do_not_share_ssh_metadata_or_tools() -> Result<()> {
    for (enabled, has_target, model_tools, has_engine) in [
        (false, true, true, true),
        (true, false, true, true),
        (true, true, false, true),
        (true, true, true, false),
    ] {
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let mut p = provider(listener.local_addr()?);
        p.models[0].supports_tools = model_tools;
        let bound = bound_target("private-canary");
        let remote = Arc::new(Fake {
            target: bound.remote(),
            commands: AtomicUsize::new(0),
        });
        let engine = Engine::new(
            tiny_shell_mcp::storage::Store::new(&directory)?,
            remote.clone(),
        )?;
        engine
            .set_ai_enabled(enabled)
            .map_err(|e| anyhow!(e.code))?;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let body = request(&mut socket).await?;
            response(&mut socket, json!({"content":"普通回答"}), "stop").await?;
            Ok::<_, anyhow::Error>(body)
        });
        let service = fixture(directory.clone(), p, has_engine.then_some(engine))?;
        let id = service.create(has_target.then_some(bound))?;
        service.send(id, "你好".into())?;
        let body = tokio::time::timeout(Duration::from_secs(5), server).await???;
        assert!(
            !body.to_string().contains("private-canary"),
            "text-only requests must not share target metadata"
        );
        assert!(
            body.get("tools").is_none(),
            "text-only requests must not expose SSH tools"
        );
        settled(&service).await?;
        assert!(service.snapshot().conversations[0].state == TurnState::Idle);
        assert_eq!(remote.commands.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(directory)?;
    }
    Ok(())
}

#[tokio::test]
async fn first_message_fixes_target_and_rejects_a_second_active_turn() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let p = provider(listener.local_addr()?);
    let service = fixture(directory.clone(), p.clone(), None)?;
    let first = bound_target("first");
    let second = bound_target("second");
    let id = service.create(Some(first.clone()))?;
    service.configure_conversation(
        &id,
        Some(p.id.clone()),
        p.default_model.clone(),
        Some(second.clone()),
    )?;
    assert_eq!(
        service.snapshot().conversations[0]
            .target
            .as_ref()
            .map(|t| &t.id),
        Some(&second.id)
    );
    let (started, ready) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        request(&mut socket).await?;
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n").await?;
        let _ = started.send(());
        let mut byte = [0];
        ensure!(
            tokio::time::timeout(Duration::from_secs(5), socket.read(&mut byte)).await?? == 0,
            "stop must close this request"
        );
        Ok::<_, anyhow::Error>(())
    });
    service.send(id.clone(), "第一条消息".into())?;
    tokio::time::timeout(Duration::from_secs(5), ready).await??;
    assert_eq!(
        service
            .send(id.clone(), "重复发送".into())
            .unwrap_err()
            .to_string(),
        "AI_BUSY"
    );
    assert_eq!(
        service
            .configure_conversation(
                &id,
                Some(p.id.clone()),
                p.default_model.clone(),
                Some(first.clone())
            )
            .unwrap_err()
            .to_string(),
        "AI_BUSY"
    );
    service.stop(&id);
    server.await??;
    settled(&service).await?;
    service.configure_conversation(
        &id,
        Some(p.id.clone()),
        p.default_model.clone(),
        Some(first.clone()),
    )?;
    let snapshot = service.snapshot();
    assert_eq!(
        snapshot.conversations[0].target.as_ref().map(|t| &t.id),
        Some(&second.id),
        "changing selection after the first message cannot rebind the chat"
    );
    assert_eq!(
        snapshot.conversations[0]
            .messages
            .iter()
            .filter(|m| m.role == "user")
            .count(),
        1
    );
    let other = service.create(Some(first.clone()))?;
    assert_eq!(
        service
            .snapshot()
            .conversations
            .iter()
            .find(|c| c.id == other)
            .and_then(|c| c.target.as_ref())
            .map(|t| &t.id),
        Some(&first.id)
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[tokio::test]
async fn history_feedback_does_not_finish_a_pending_provider_operation() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let p = provider(listener.local_addr()?);
    let service = fixture(directory.clone(), p.clone(), None)?;
    let id = service.create(None)?;
    let (started, ready) = tokio::sync::oneshot::channel();
    let resume = Arc::new(tokio::sync::Notify::new());
    let server_resume = resume.clone();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        request(&mut socket).await?;
        let _ = started.send(());
        server_resume.notified().await;
        let body = json!({"choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}]}).to_string();
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
        Ok::<_, anyhow::Error>(())
    });
    service.test_provider(p, None)?;
    tokio::time::timeout(Duration::from_secs(5), ready).await??;
    let before = service.stamp();
    service.rename(id, String::new());
    tokio::time::timeout(Duration::from_secs(5), async {
        while service.stamp() == before {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(
        service.snapshot().busy,
        "history completion must not unlock the still-pending provider test"
    );
    assert!(
        service.snapshot().notice.is_none(),
        "history errors must not replace provider feedback"
    );
    assert_eq!(
        service.snapshot().history_notice.as_deref(),
        Some("AI_INVALID_NAME")
    );
    resume.notify_one();
    server.await??;
    settled(&service).await?;
    assert_eq!(service.snapshot().notice.as_deref(), Some("AI_TEST_OK"));
    assert_eq!(
        service.snapshot().history_notice.as_deref(),
        Some("AI_INVALID_NAME")
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
#[tokio::test]
async fn unsaved_endpoint_never_receives_a_saved_key() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut p = provider(listener.local_addr()?);
    let service = fixture(directory.clone(), p.clone(), None)?;
    p.base_url.push_str("/different");
    assert!(service.test_provider(p, None).is_err());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[tokio::test]
async fn stop_cancels_a_provider_that_stalls_in_its_json_body() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let service = fixture(directory.clone(), provider(listener.local_addr()?), None)?;
    let (started, ready) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        request(&mut socket).await?;
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n").await?;
        let _ = started.send(());
        let mut byte = [0];
        let ended = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut byte)).await??;
        ensure!(ended == 0, "cancel must close the provider response");
        ensure!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "stop must not automatically retry"
        );
        Ok::<_, anyhow::Error>(())
    });
    let id = service.create(None)?;
    service.send(id.clone(), "等待回答".into())?;
    tokio::time::timeout(Duration::from_secs(3), ready).await??;
    service.stop(&id);
    tokio::time::timeout(Duration::from_secs(3), async {
        while service.snapshot().conversations[0].state.active() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    server.await??;
    assert!(service.snapshot().conversations[0].state == TurnState::Interrupted);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[tokio::test]
async fn stale_target_metadata_is_not_sent_to_the_provider() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let p = provider(listener.local_addr()?);
    let target = Target {
        id: uuid::Uuid::new_v4().to_string(),
        name: "private-canary".into(),
        host: "private-canary.invalid".into(),
        port: 22,
        user: "private-canary-user".into(),
        group: None,
        digest: "private-canary-digest".into(),
    };
    let remote = Arc::new(Fake {
        target: target.clone(),
        commands: AtomicUsize::new(0),
    });
    let engine = Engine::new(
        tiny_shell_mcp::storage::Store::new(&directory)?,
        remote.clone(),
    )?;
    engine.set_ai_enabled(true).map_err(|e| anyhow!(e.code))?;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let body = request(&mut socket).await?;
        response(&mut socket, json!({"content":"普通回答"}), "stop").await?;
        Ok::<_, anyhow::Error>(body)
    });
    let service = fixture(directory.clone(), p, Some(engine))?;
    let id = service.create(Some(BoundTarget {
        id: target.id,
        name: target.name,
        host: target.host,
        port: target.port,
        user: target.user,
        digest: "stale-digest".into(),
    }))?;
    service.send(id, "你好".into())?;
    let body = tokio::time::timeout(Duration::from_secs(5), server).await???;
    assert!(
        !body.to_string().contains("private-canary"),
        "stale SSH details must remain local"
    );
    assert!(
        body.get("tools").is_none(),
        "stale target must expose no tools"
    );
    assert_eq!(remote.commands.load(Ordering::SeqCst), 0);
    tokio::time::timeout(Duration::from_secs(5), async {
        while service.snapshot().conversations[0].state.active() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
#[tokio::test]
async fn streaming_agent_uses_global_ai_approval_and_encrypted_history() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let p = provider(listener.local_addr()?);
    let target = Target {
        id: uuid::Uuid::new_v4().to_string(),
        name: "fixture".into(),
        host: "example.invalid".into(),
        port: 22,
        user: "test".into(),
        group: None,
        digest: "fixture-target-digest".into(),
    };
    let remote = Arc::new(Fake {
        target: target.clone(),
        commands: AtomicUsize::new(0),
    });
    let engine = Engine::new(
        tiny_shell_mcp::storage::Store::new(&directory)?,
        remote.clone(),
    )?;
    let mut policy = engine.policy().map_err(|e| anyhow!(e.code))?;
    policy.mode = Mode::ReadOnly;
    engine.set_policy(policy).map_err(|e| anyhow!(e.code))?;
    engine
        .set_ai_policy(tiny_shell_mcp::engine::AiPolicy {
            enabled: true,
            mode: Mode::Controlled,
        })
        .map_err(|e| anyhow!(e.code))?;
    let server = tokio::spawn(async move {
        for round in 0..2 {
            let (mut socket, _) = listener.accept().await?;
            let request = request(&mut socket).await?;
            ensure!(request["model"] == "fixture-model", "model missing");
            ensure!(request["stream"] == true, "stream missing");
            let names = request["tools"]
                .as_array()
                .ok_or_else(|| anyhow!("tools missing"))?;
            ensure!(names.len() == 4, "only operation tools should be exposed");
            ensure!(
                names
                    .iter()
                    .all(|tool| tool["function"]["parameters"]["properties"]
                        .get("session_id")
                        .is_none()),
                "session ID must be application-owned"
            );
            let delta = if round == 0 {
                json!({"tool_calls":[{"index":0,"id":"call-command","type":"function","function":{"name":"run_command","arguments":json!({"command":"printf fixture"}).to_string()}}]})
            } else {
                let output = request["messages"]
                    .as_array()
                    .and_then(|messages| {
                        messages
                            .iter()
                            .rev()
                            .find(|message| message["role"] == "tool")
                    })
                    .and_then(|message| message["content"].as_str())
                    .ok_or_else(|| anyhow!("tool result missing"))?;
                let output: Value = serde_json::from_str(output)?;
                ensure!(output["state"] == "succeeded", "completed result expected");
                ensure!(
                    output["output"][0]["text"] == "fixture output",
                    "command output missing"
                );
                json!({"content":"检查完成"})
            };
            response(
                &mut socket,
                delta,
                if round == 1 { "stop" } else { "tool_calls" },
            )
            .await?;
        }
        Ok::<(), anyhow::Error>(())
    });
    let service = fixture(directory.clone(), p, Some(engine.clone()))?;
    let bound = BoundTarget {
        id: target.id,
        name: target.name,
        host: target.host,
        port: target.port,
        user: target.user,
        digest: target.digest,
    };
    let id = service.create(Some(bound))?;
    service.send(id.clone(), "检查配置".into())?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if service.snapshot().conversations[0].state == TurnState::AwaitingApproval {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(remote.commands.load(Ordering::SeqCst), 0);
    let review = engine
        .reviews()
        .map_err(|e| anyhow!(e.code))?
        .into_iter()
        .find(|r| r.state == tiny_shell_mcp::tasks::State::AwaitingUser)
        .ok_or_else(|| anyhow!("missing approval"))?;
    let pending = service.snapshot();
    let pending = pending.conversations[0].activities.last().unwrap();
    assert_eq!(pending.message_index, Some(1));
    assert_eq!(pending.tool_call_id.as_deref(), Some("call-command"));
    assert_eq!(pending.task_id.as_deref(), Some(review.id.as_str()));
    // Toggling external access must not invalidate this internal approval.
    let mut policy = engine.policy().map_err(|e| anyhow!(e.code))?;
    policy.enabled = true;
    engine.set_policy(policy).map_err(|e| anyhow!(e.code))?;
    policy = engine.policy().map_err(|e| anyhow!(e.code))?;
    policy.enabled = false;
    policy.mode = Mode::Full;
    policy.grants.clear();
    engine.set_policy(policy).map_err(|e| anyhow!(e.code))?;
    engine
        .approve(&review.id, true)
        .map_err(|e| anyhow!(e.code))?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if !service.snapshot().conversations[0].state.active() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    server.await??;
    let snapshot = service.snapshot();
    assert!(snapshot.conversations[0].state == TurnState::Idle);
    assert_eq!(remote.commands.load(Ordering::SeqCst), 1);
    assert_eq!(
        snapshot.conversations[0]
            .messages
            .last()
            .map(|m| m.content.as_str()),
        Some("检查完成")
    );
    service.checkpoint(&id)?;
    let bytes = std::fs::read(directory.join("ai/conversations").join(format!("{id}.enc")))?;
    assert!(!String::from_utf8_lossy(&bytes).contains("fixture output"));
    let plaintext = super::super::storage::unseal(&[9; 32], &id, &bytes)?;
    assert!(!String::from_utf8_lossy(&plaintext).contains("fixture-key"));
    let restored: Conversation = serde_json::from_slice(&plaintext)?;
    assert_eq!(restored.messages.len(), 4);
    assert_eq!(restored.activities.len(), 1);
    for activity in &restored.activities {
        let message = &restored.messages[activity.message_index.unwrap()];
        assert!(
            message
                .tool_calls
                .iter()
                .any(|call| call["id"].as_str() == activity.tool_call_id.as_deref())
        );
    }
    service.delete(vec![id.clone()]);
    tokio::time::timeout(Duration::from_secs(3), async {
        while !service.snapshot().conversations.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(
        !directory
            .join("ai/conversations")
            .join(format!("{id}.enc"))
            .exists()
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
