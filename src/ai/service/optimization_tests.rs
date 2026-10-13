use super::tests::{fixture, provider, request, response, settled};
use super::*;
use serde_json::{Value, json};
use std::{sync::atomic::AtomicUsize, time::Duration};
use tiny_shell_mcp::{
    contracts::{Call, Failure, Result as McpResult},
    engine::{AiPolicy, Remote, Target},
    policy::Mode,
};
use tokio::net::TcpListener;

#[derive(Clone, Copy)]
enum Outcome {
    Success,
    ReadFailure,
    UnknownCommand,
}
struct RemoteFixture {
    target: Target,
    outcome: Outcome,
    opens: AtomicUsize,
    reads: AtomicUsize,
    commands: AtomicUsize,
    closes: AtomicUsize,
    sessions: Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl Remote for RemoteFixture {
    async fn targets(&self) -> McpResult<Vec<Target>> {
        Ok(vec![self.target.clone()])
    }
    async fn execute(
        &self,
        _: &Target,
        session: &str,
        call: &Call,
        _: watch::Receiver<bool>,
    ) -> McpResult<Value> {
        match call {
            Call::OpenSession(_) => {
                self.opens.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"session_id":session}))
            }
            Call::ReadFile(_) => {
                self.sessions.lock().unwrap().push(session.into());
                self.reads.fetch_add(1, Ordering::SeqCst);
                if matches!(self.outcome, Outcome::ReadFailure) {
                    Err(Failure::new("FILE_ERROR"))
                } else {
                    Ok(
                        json!({"content":"fixture text","version":"fixture-version","size":12,"encoding":"UTF-8"}),
                    )
                }
            }
            Call::RunCommand(_) => {
                self.sessions.lock().unwrap().push(session.into());
                self.commands.fetch_add(1, Ordering::SeqCst);
                if matches!(self.outcome, Outcome::UnknownCommand) {
                    Err(Failure::new("OUTCOME_UNKNOWN"))
                } else {
                    Ok(json!({"stdout":"fixture output","stderr":"","exit_code":0}))
                }
            }
            _ => Err(Failure::new("INVALID_ARGUMENT")),
        }
    }
    async fn close(&self, _: &str) {
        self.closes.fetch_add(1, Ordering::SeqCst);
    }
}
fn tool(index: usize, name: &str, args: Value) -> Value {
    json!({"index":index,"id":format!("call-{index}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})
}
async fn batch(
    calls: Vec<Value>,
    outcome: Outcome,
    turns: usize,
) -> Result<(Snapshot, Arc<RemoteFixture>, Vec<Value>)> {
    batch_with_choice(calls, outcome, turns, false, ExecutionSettings::default()).await
}
async fn batch_with_choice(
    calls: Vec<Value>,
    outcome: Outcome,
    turns: usize,
    terminate: bool,
    execution: ExecutionSettings,
) -> Result<(Snapshot, Arc<RemoteFixture>, Vec<Value>)> {
    let model_allowance = execution.model_requests;
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let p = provider(listener.local_addr()?);
    let bound = BoundTarget {
        id: uuid::Uuid::new_v4().to_string(),
        name: "fixture".into(),
        host: "fixture.invalid".into(),
        port: 22,
        user: "fixture".into(),
        digest: "fixture-digest".into(),
    };
    let remote = Arc::new(RemoteFixture {
        target: bound.remote(),
        outcome,
        opens: AtomicUsize::new(0),
        reads: AtomicUsize::new(0),
        commands: AtomicUsize::new(0),
        closes: AtomicUsize::new(0),
        sessions: Mutex::new(Vec::new()),
    });
    let engine = fixture_engine(
        tiny_shell_mcp::storage::Store::new(&directory)?,
        remote.clone(),
    )?;
    engine
        .set_ai_policy(AiPolicy {
            enabled: true,
            mode: Mode::Full,
        })
        .map_err(|e| anyhow!(e.code))?;
    let server = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for turn in 0..turns {
            let (mut socket, _) = listener.accept().await?;
            bodies.push(request(&mut socket).await?);
            response(&mut socket, json!({"tool_calls":calls}), "tool_calls").await?;
            if terminate {
                break;
            }
            let (mut socket, _) = listener.accept().await?;
            bodies.push(request(&mut socket).await?);
            response(&mut socket, json!({"content":"fixture summary"}), "stop").await?;
            if turn == 0 {
                let (mut title, _) = listener.accept().await?;
                let body = request(&mut title).await?;
                assert_eq!(body["stream"], false);
                assert!(body.get("tools").is_none());
                super::tests::title_response(&mut title, "检查测试配置").await?;
            }
        }
        Ok::<_, anyhow::Error>(bodies)
    });
    let service = fixture(directory.clone(), p, Some(engine))?;
    service
        .state
        .lock()
        .map_err(|_| anyhow!("fixture lock"))?
        .snapshot
        .settings
        .execution = ExecutionSettings {
        followup_suggestions: false,
        ..execution
    };
    let id = service.create(Some(bound))?;
    for _ in 0..turns {
        service.send(id.clone(), "检查 fixture".into())?;
        let pauses = tokio::time::timeout(Duration::from_secs(5), async {
            let mut choices = HashSet::new();
            loop {
                let snapshot = service.snapshot();
                let c = &snapshot.conversations[0];
                if let Some(pause) = &c.continuation
                    && choices.insert(pause.token.clone())
                {
                    let completed = c.messages.iter().filter(|m| m.role == "tool").count();
                    for _ in 0..20 {
                        tokio::task::yield_now().await;
                    }
                    assert_eq!(
                        service.snapshot().conversations[0]
                            .messages
                            .iter()
                            .filter(|m| m.role == "tool")
                            .count(),
                        completed,
                        "waiting must perform no further tools"
                    );
                    if terminate {
                        service.terminate_continuation(&id, &pause.token)?;
                        assert!(service.resume(&id, &pause.token).is_err());
                    } else {
                        service.resume(&id, &pause.token)?;
                        assert!(
                            service.resume(&id, &pause.token).is_err(),
                            "a double click must not grant another allowance"
                        );
                    }
                }
                if !c.state.active() {
                    break;
                }
                tokio::task::yield_now().await;
            }
            Ok::<_, anyhow::Error>(choices.len())
        })
        .await??;
        if model_allowance == 1 {
            assert!(
                pauses > 0,
                "the second model request must wait for an explicit continuation"
            );
        }
        settled(&service).await?;
        assert!(
            service.snapshot().conversations[0].state
                == if terminate {
                    TurnState::Interrupted
                } else {
                    TurnState::Idle
                }
        );
    }
    let bodies = tokio::time::timeout(Duration::from_secs(5), server).await???;
    tokio::time::timeout(Duration::from_secs(2), async {
        while remote.closes.load(Ordering::SeqCst) != remote.opens.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let snapshot = service.snapshot();
    service.audit.close().await?;
    std::fs::remove_dir_all(directory)?;
    Ok((snapshot, remote, bodies))
}
fn outputs(snapshot: &Snapshot) -> Vec<Value> {
    snapshot.conversations[0]
        .messages
        .iter()
        .filter(|m| m.role == "tool")
        .map(|m| serde_json::from_str(&m.content).unwrap())
        .collect()
}

#[tokio::test]
async fn application_reuses_one_session_per_turn_and_does_not_replay_history_ids() -> Result<()> {
    let (snapshot, remote, bodies) = batch(
        vec![
            tool(0, "read_file", json!({"path":"/first"})),
            tool(1, "read_file", json!({"path":"/second"})),
        ],
        Outcome::Success,
        2,
    )
    .await?;
    assert_eq!(
        bodies.len(),
        4,
        "each turn needs only operation and summary model requests"
    );
    assert_eq!(remote.opens.load(Ordering::SeqCst), 2);
    assert_eq!(remote.closes.load(Ordering::SeqCst), 2);
    assert_eq!(remote.reads.load(Ordering::SeqCst), 4);
    let sessions = remote.sessions.lock().unwrap();
    assert_eq!(sessions[0], sessions[1]);
    assert_eq!(sessions[2], sessions[3]);
    assert_ne!(sessions[0], sessions[2]);
    for output in outputs(&snapshot) {
        assert_eq!(output["version"], "fixture-version");
        assert!(output.get("id").is_none());
        assert!(!output.to_string().contains("session_id"));
    }
    Ok(())
}

#[tokio::test]
async fn identical_reads_are_reused_and_a_command_invalidates_the_read_cache() -> Result<()> {
    let calls = vec![
        tool(0, "read_file", json!({"path":"/file"})),
        tool(1, "read_file", json!({"path":"/file"})),
        tool(2, "run_command", json!({"command":"fixture"})),
        tool(3, "read_file", json!({"path":"/file"})),
    ];
    let (snapshot, remote, _) = batch(calls, Outcome::Success, 1).await?;
    assert_eq!(remote.opens.load(Ordering::SeqCst), 1);
    assert_eq!(remote.reads.load(Ordering::SeqCst), 2);
    assert_eq!(remote.commands.load(Ordering::SeqCst), 1);
    assert_eq!(outputs(&snapshot).len(), 4);
    Ok(())
}

#[tokio::test]
async fn continuing_a_tool_budget_resumes_the_batch_without_replaying_operations() -> Result<()> {
    let calls = (0..18)
        .map(|i| tool(i, "read_file", json!({"path":format!("/{i}")})))
        .collect();
    let (snapshot, remote, bodies) = batch(calls, Outcome::Success, 1).await?;
    assert_eq!(remote.reads.load(Ordering::SeqCst), 18);
    assert_eq!(remote.opens.load(Ordering::SeqCst), 1);
    assert_eq!(remote.closes.load(Ordering::SeqCst), 1);
    let results = outputs(&snapshot);
    assert_eq!(
        results.len(),
        18,
        "all provider calls require corresponding results"
    );
    for output in &results[16..] {
        assert!(output.get("error").is_none());
    }
    assert_eq!(bodies.len(), 2);
    assert!(bodies[1].get("tools").is_some());
    Ok(())
}

#[tokio::test]
async fn terminating_a_tool_budget_preserves_results_and_pairs_unexecuted_calls() -> Result<()> {
    let calls = (0..18)
        .map(|i| tool(i, "read_file", json!({"path":format!("/{i}")})))
        .collect();
    let (snapshot, remote, bodies) = batch_with_choice(
        calls,
        Outcome::Success,
        1,
        true,
        ExecutionSettings::default(),
    )
    .await?;
    assert_eq!(remote.reads.load(Ordering::SeqCst), 16);
    assert_eq!(remote.closes.load(Ordering::SeqCst), 1);
    assert_eq!(
        bodies.len(),
        1,
        "termination must not spend another model request for a summary"
    );
    let results = outputs(&snapshot);
    assert_eq!(results.len(), 18);
    assert!(results[16..].iter().all(|r| r["state"] == "not_executed"));
    Ok(())
}

#[tokio::test]
async fn model_budget_can_pause_before_the_final_reply_without_replaying_tools() -> Result<()> {
    let execution = ExecutionSettings {
        model_requests: 1,
        ..ExecutionSettings::default()
    };
    let (snapshot, remote, bodies) = batch_with_choice(
        vec![tool(0, "read_file", json!({"path":"/file"}))],
        Outcome::Success,
        1,
        false,
        execution,
    )
    .await?;
    assert_eq!(remote.reads.load(Ordering::SeqCst), 1);
    assert_eq!(bodies.len(), 2);
    assert_eq!(
        snapshot.conversations[0]
            .messages
            .last()
            .map(|m| m.content.as_str()),
        Some("fixture summary")
    );
    Ok(())
}

#[tokio::test]
async fn unchanged_failures_and_unknown_mutations_stop_following_calls() -> Result<()> {
    for (outcome, name, args, executed) in [
        (
            Outcome::ReadFailure,
            "read_file",
            json!({"path":"/missing"}),
            3,
        ),
        (
            Outcome::UnknownCommand,
            "run_command",
            json!({"command":"fixture"}),
            1,
        ),
    ] {
        let calls = (0..4).map(|i| tool(i, name, args.clone())).collect();
        let (snapshot, remote, bodies) = batch(calls, outcome, 1).await?;
        assert_eq!(
            remote.reads.load(Ordering::SeqCst) + remote.commands.load(Ordering::SeqCst),
            executed
        );
        let results = outputs(&snapshot);
        assert_eq!(results.len(), 4);
        for output in &results[executed..] {
            assert_eq!(output["state"], "not_executed");
        }
        if matches!(outcome, Outcome::UnknownCommand) {
            assert_eq!(results[0]["state"], "outcome_unknown");
            assert_eq!(results[1]["error"], "AI_OUTCOME_UNKNOWN");
        }
        assert!(bodies[1].get("tools").is_none());
    }
    Ok(())
}

#[tokio::test]
async fn model_supplied_session_identifiers_are_rejected_before_opening_ssh() -> Result<()> {
    let (snapshot, remote, _) = batch(
        vec![tool(
            0,
            "read_file",
            json!({"path":"/file","session_id":uuid::Uuid::new_v4().to_string()}),
        )],
        Outcome::Success,
        1,
    )
    .await?;
    assert_eq!(remote.opens.load(Ordering::SeqCst), 0);
    assert_eq!(remote.reads.load(Ordering::SeqCst), 0);
    assert_eq!(outputs(&snapshot)[0]["error"], "INVALID_ARGUMENT");
    Ok(())
}

#[tokio::test]
async fn stopping_during_approval_cancels_the_wait_without_executing_the_command() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let p = provider(listener.local_addr()?);
    let bound = BoundTarget {
        id: uuid::Uuid::new_v4().to_string(),
        name: "fixture".into(),
        host: "fixture.invalid".into(),
        port: 22,
        user: "fixture".into(),
        digest: "fixture-digest".into(),
    };
    let remote = Arc::new(RemoteFixture {
        target: bound.remote(),
        outcome: Outcome::Success,
        opens: AtomicUsize::new(0),
        reads: AtomicUsize::new(0),
        commands: AtomicUsize::new(0),
        closes: AtomicUsize::new(0),
        sessions: Mutex::new(Vec::new()),
    });
    let engine = fixture_engine(
        tiny_shell_mcp::storage::Store::new(&directory)?,
        remote.clone(),
    )?;
    engine
        .set_ai_policy(AiPolicy {
            enabled: true,
            mode: Mode::Controlled,
        })
        .map_err(|e| anyhow!(e.code))?;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        request(&mut socket).await?;
        response(
            &mut socket,
            json!({"tool_calls":[tool(0,"run_command",json!({"command":"fixture"}))]}),
            "tool_calls",
        )
        .await
    });
    let service = fixture(directory.clone(), p, Some(engine.clone()))?;
    let id = service.create(Some(bound))?;
    service.send(id.clone(), "检查 fixture".into())?;
    tokio::time::timeout(Duration::from_secs(3), async {
        while service.snapshot().conversations[0].state != TurnState::AwaitingApproval {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    service.stop(&id);
    settled(&service).await?;
    server.await??;
    assert!(service.snapshot().conversations[0].state == TurnState::Interrupted);
    assert_eq!(remote.commands.load(Ordering::SeqCst), 0);
    assert!(
        engine
            .reviews()
            .map_err(|e| anyhow!(e.code))?
            .iter()
            .any(|task| task.state == tiny_shell_mcp::tasks::State::Cancelled)
    );
    service.audit.close().await?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

fn fixture_engine(
    store: tiny_shell_mcp::storage::Store,
    remote: std::sync::Arc<dyn tiny_shell_mcp::engine::Remote>,
) -> anyhow::Result<std::sync::Arc<tiny_shell_mcp::engine::Engine>> {
    let directory = store.root().parent().unwrap_or(store.root()).to_path_buf();
    tiny_shell_mcp::engine::Engine::with_audit(
        store,
        remote,
        tiny_shell_mcp::audit::Store::with_key(directory, [23; 32]),
    )
}
