use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tiny_shell_mcp::{
    contracts::{Call, Result},
    engine::{AiPolicy, Engine, EngineHandler, Remote, Target},
    ipc::Handler,
    policy::Mode,
    storage::Store,
};
use tokio::sync::watch;

struct Fake(Vec<Target>);
#[async_trait]
impl Remote for Fake {
    async fn targets(&self) -> Result<Vec<Target>> {
        Ok(self.0.clone())
    }
    async fn execute(
        &self,
        _: &Target,
        session: &str,
        _: &Call,
        _: watch::Receiver<bool>,
    ) -> Result<Value> {
        Ok(json!({"session_id":session}))
    }
    async fn close(&self, _: &str) {}
}

#[tokio::test]
async fn ai_global_access_does_not_require_mcp_machine_grants() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let targets = (0..2)
        .map(|i| Target {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("bound-{i}"),
            group: None,
            host: "example.invalid".into(),
            port: 22,
            user: "test".into(),
            digest: format!("bound-digest-{i}"),
        })
        .collect::<Vec<_>>();
    let store = Store::new(dir.path())?;
    let external = store.register("test")?;
    let engine = fixture_engine(store, Arc::new(Fake(targets.clone())))?;
    let mut policy = engine.policy().map_err(|e| anyhow::anyhow!(e.code))?;
    policy.enabled = true;
    policy.grant(targets[0].id.clone(), targets[0].digest.clone());
    engine
        .set_policy(policy)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    engine
        .set_ai_enabled(true)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    for target in &targets {
        let ai = engine
            .internal_client(&uuid::Uuid::new_v4().to_string(), target.clone())
            .map_err(|e| anyhow::anyhow!(e.code))?;
        let listed = ai
            .call(Call::parse("list_connections", json!({})).unwrap())
            .await
            .map_err(|e| anyhow::anyhow!(e.code))?;
        assert_eq!(listed["connections"].as_array().unwrap().len(), 1);
        assert_eq!(listed["connections"][0]["id"], target.id);
        let opened = ai
            .call(
                Call::parse(
                    "open_session",
                    json!({
                        "connection_id": target.id, "request_id": uuid::Uuid::new_v4().to_string()
                    }),
                )
                .unwrap(),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.code))?;
        assert!(opened["task_id"].is_string());
    }
    let external_list = EngineHandler(engine.clone())
        .call(
            &external.id,
            Call::parse("list_connections", json!({})).unwrap(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert_eq!(external_list["connections"].as_array().unwrap().len(), 1);
    assert_eq!(external_list["connections"][0]["id"], targets[0].id);
    let denied = EngineHandler(engine)
        .call(
            &external.id,
            Call::parse(
                "open_session",
                json!({
                    "connection_id": targets[1].id, "request_id": uuid::Uuid::new_v4().to_string()
                }),
            )
            .unwrap(),
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code, "CONNECTION_NOT_ALLOWED");
    Ok(())
}

async fn task_state(
    engine: &Engine,
    id: &str,
    expected: tiny_shell_mcp::tasks::State,
) -> anyhow::Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if engine
                .reviews()
                .map_err(|e| anyhow::anyhow!(e.code))?
                .iter()
                .any(|r| r.id == id && r.state == expected)
            {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn conversation_overrides_control_execution_without_changing_global_or_sibling_modes()
-> anyhow::Result<()> {
    use tiny_shell_mcp::tasks::State;
    let directory = tempfile::tempdir()?;
    let target = Target {
        id: uuid::Uuid::new_v4().to_string(),
        name: "fixture".into(),
        group: None,
        host: "example.invalid".into(),
        port: 22,
        user: "test".into(),
        digest: "fixture".into(),
    };
    let engine = fixture_engine(
        Store::new(directory.path())?,
        Arc::new(Fake(vec![target.clone()])),
    )?;
    engine.set_ai_enabled(true).unwrap();
    let inherited = engine
        .internal_client(&uuid::Uuid::new_v4().to_string(), target.clone())
        .unwrap();
    for mode in [Mode::ReadOnly, Mode::Controlled, Mode::Full] {
        let id = uuid::Uuid::new_v4().to_string();
        let client = engine
            .checked_internal_client_with_mode(&id, target.clone(), Some(mode))
            .await
            .unwrap();
        assert_eq!(client.mode().unwrap(), mode);
        assert_eq!(inherited.mode().unwrap(), Mode::ReadOnly);
        assert_eq!(engine.ai_policy().unwrap().mode, Mode::ReadOnly);
        let opened = client.call(Call::parse("open_session", json!({"connection_id":target.id,"request_id":uuid::Uuid::new_v4().to_string()})).unwrap()).await.unwrap();
        let task = opened["task_id"].as_str().unwrap();
        task_state(&engine, task, State::Succeeded).await?;
        let session = client
            .call(Call::parse("get_task", json!({"task_id":task})).unwrap())
            .await
            .unwrap()["result"]["session_id"]
            .clone();
        let result = client.call(Call::parse("run_command", json!({"session_id":session,"command":"printf fixture","request_id":uuid::Uuid::new_v4().to_string()})).unwrap()).await;
        match mode {
            Mode::ReadOnly => assert_eq!(result.unwrap_err().code, "PERMISSION_DENIED"),
            Mode::Controlled => {
                let result = result.unwrap();
                let task = result["task_id"].as_str().unwrap();
                task_state(&engine, task, State::AwaitingUser).await?;
                // Reissuing the capability cannot change a pending approval's mode.
                assert!(
                    engine
                        .internal_client_with_mode(&id, target.clone(), Some(Mode::Full))
                        .is_err()
                );
                assert_eq!(client.mode().unwrap(), Mode::Controlled);
                engine.approve(task, true).unwrap();
                task_state(&engine, task, State::Succeeded).await?;
            }
            Mode::Full => {
                task_state(
                    &engine,
                    result.unwrap()["task_id"].as_str().unwrap(),
                    State::Succeeded,
                )
                .await?
            }
        }
        client.stop().unwrap();
        let following = engine
            .internal_client_with_mode(&id, target.clone(), None)
            .unwrap();
        assert_eq!(following.mode().unwrap(), Mode::ReadOnly);
        assert!(
            !client.is_valid(),
            "a prior handle cannot acquire the next turn's permission"
        );
        assert_eq!(client.stop().unwrap_err().code, "CLIENT_REVOKED");
        assert!(
            inherited.is_valid(),
            "another conversation's selection cannot revoke this one"
        );
    }
    let id = uuid::Uuid::new_v4().to_string();
    let overridden = engine
        .internal_client_with_mode(&id, target.clone(), Some(Mode::Full))
        .unwrap();
    engine
        .set_ai_policy(AiPolicy {
            enabled: true,
            mode: Mode::Controlled,
        })
        .unwrap();
    assert!(!overridden.is_valid());
    assert!(!inherited.is_valid());
    assert_eq!(
        engine
            .internal_client_with_mode(&id, target.clone(), Some(Mode::Full))
            .unwrap()
            .mode()
            .unwrap(),
        Mode::Full
    );
    assert_eq!(
        engine
            .internal_client(&uuid::Uuid::new_v4().to_string(), target.clone())
            .unwrap()
            .mode()
            .unwrap(),
        Mode::Controlled
    );
    engine.set_ai_enabled(false).unwrap();
    assert!(
        engine
            .internal_client_with_mode(&id, target.clone(), Some(Mode::Full))
            .is_err()
    );
    engine.set_ai_enabled(true).unwrap();
    assert!(
        !overridden.is_valid(),
        "the global enable switch must still revoke overrides"
    );
    Ok(())
}

#[tokio::test]
async fn permission_modes_and_pending_approvals_are_isolated_by_source() -> anyhow::Result<()> {
    use tiny_shell_mcp::tasks::State;
    let dir = tempfile::tempdir()?;
    let target = Target {
        id: uuid::Uuid::new_v4().to_string(),
        name: "fixture".into(),
        host: "example.invalid".into(),
        port: 22,
        user: "test".into(),
        group: None,
        digest: "fixture-digest".into(),
    };
    let store = Store::new(dir.path())?;
    let external = store.register("test")?;
    let engine = fixture_engine(store, Arc::new(Fake(vec![target.clone()])))?;
    let mut policy = engine.policy().map_err(|e| anyhow::anyhow!(e.code))?;
    policy.enabled = true;
    policy.mode = Mode::Controlled;
    policy.grant(target.id.clone(), target.digest.clone());
    engine
        .set_policy(policy)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    engine
        .set_ai_enabled(true)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let conversation = uuid::Uuid::new_v4().to_string();
    let ai = engine
        .internal_client(&conversation, target.clone())
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let handler = EngineHandler(engine.clone());
    let open = || {
        Call::parse(
            "open_session",
            json!({"connection_id":target.id,
        "request_id":uuid::Uuid::new_v4().to_string()}),
        )
        .unwrap()
    };
    let get = |id: &str| Call::parse("get_task", json!({"task_id":id})).unwrap();
    let command = |session: &Value| {
        Call::parse(
            "run_command",
            json!({"session_id":session,
        "command":"printf fixture", "request_id":uuid::Uuid::new_v4().to_string()}),
        )
        .unwrap()
    };
    let opened = ai.call(open()).await.map_err(|e| anyhow::anyhow!(e.code))?;
    let id = opened["task_id"].as_str().unwrap();
    task_state(&engine, id, State::Succeeded).await?;
    let session = ai
        .call(get(id))
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?["result"]["session_id"]
        .clone();
    assert_eq!(
        ai.call(command(&session)).await.unwrap_err().code,
        "PERMISSION_DENIED",
        "external controlled mode must not enable commands in read-only AI"
    );
    let opened = handler
        .call(&external.id, open())
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let id = opened["task_id"].as_str().unwrap();
    task_state(&engine, id, State::Succeeded).await?;
    let session = handler
        .call(&external.id, get(id))
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?["result"]["session_id"]
        .clone();
    let pending = handler
        .call(&external.id, command(&session))
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let external_pending = pending["task_id"].as_str().unwrap();
    task_state(&engine, external_pending, State::AwaitingUser).await?;
    engine
        .set_ai_policy(AiPolicy {
            enabled: true,
            mode: Mode::Controlled,
        })
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert!(
        !ai.is_valid(),
        "AI mode changes revoke prior AI capabilities"
    );
    task_state(&engine, external_pending, State::AwaitingUser).await?;
    engine
        .approve(external_pending, true)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    task_state(&engine, external_pending, State::Succeeded).await?;
    let fresh = engine
        .internal_client(&conversation, target.clone())
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let opened = fresh
        .call(open())
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let id = opened["task_id"].as_str().unwrap();
    task_state(&engine, id, State::Succeeded).await?;
    let session = fresh
        .call(get(id))
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?["result"]["session_id"]
        .clone();
    let pending = fresh
        .call(command(&session))
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let ai_pending = pending["task_id"].as_str().unwrap();
    task_state(&engine, ai_pending, State::AwaitingUser).await?;
    let mut policy = engine.policy().map_err(|e| anyhow::anyhow!(e.code))?;
    policy.mode = Mode::Full;
    policy.grants.clear();
    engine
        .set_policy(policy)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert!(fresh.is_valid());
    task_state(&engine, ai_pending, State::AwaitingUser).await?;
    engine
        .approve(ai_pending, true)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    task_state(&engine, ai_pending, State::Succeeded).await?;
    let records = engine.store.audit_tail()?;
    assert!(
        records
            .iter()
            .filter(|r| r["client_id"]
                .as_str()
                .is_some_and(|c| c.starts_with("ai:")))
            .any(|r| r["operation"] == "run_command" && r["mode"] == "controlled")
    );
    Ok(())
}

#[tokio::test]
async fn old_ai_settings_migrate_once_and_remain_independent_after_restart() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let store = Store::new(dir.path())?;
    let mut policy = store.load_policy()?;
    policy.mode = Mode::Controlled;
    store.save_policy(&policy)?;
    std::fs::write(
        store.root().join("ai-access.json"),
        br#"{"schema_version":1,"enabled":true}"#,
    )?;
    let engine = fixture_engine(store.clone(), Arc::new(Fake(Vec::new())))?;
    assert_eq!(
        engine.ai_policy().map_err(|e| anyhow::anyhow!(e.code))?,
        AiPolicy {
            enabled: true,
            mode: Mode::Controlled
        }
    );
    policy.mode = Mode::Full;
    engine
        .set_policy(policy)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let restored = fixture_engine(store, Arc::new(Fake(Vec::new())))?;
    assert_eq!(
        restored
            .ai_policy()
            .map_err(|e| anyhow::anyhow!(e.code))?
            .mode,
        Mode::Controlled
    );
    Ok(())
}

struct DelayedTargets {
    target: Target,
    started: watch::Sender<bool>,
    resume: tokio::sync::Notify,
}
#[async_trait]
impl Remote for DelayedTargets {
    async fn targets(&self) -> Result<Vec<Target>> {
        self.started.send_replace(true);
        self.resume.notified().await;
        Ok(vec![self.target.clone()])
    }
    async fn execute(
        &self,
        _: &Target,
        _: &str,
        _: &Call,
        _: watch::Receiver<bool>,
    ) -> Result<Value> {
        unreachable!("target validation must not execute SSH work")
    }
    async fn close(&self, _: &str) {}
}

#[tokio::test]
async fn target_lookup_cannot_resurrect_access_after_a_disable_enable_cycle() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let target = Target {
        id: uuid::Uuid::new_v4().to_string(),
        name: "fixture".into(),
        host: "example.invalid".into(),
        port: 22,
        user: "test".into(),
        group: None,
        digest: "fixture-digest".into(),
    };
    let (started, mut observed) = watch::channel(false);
    let remote = Arc::new(DelayedTargets {
        target: target.clone(),
        started,
        resume: tokio::sync::Notify::new(),
    });
    let engine = fixture_engine(Store::new(dir.path())?, remote.clone())?;
    engine
        .set_ai_enabled(true)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let issuing = engine.clone();
    let task = tokio::spawn(async move {
        issuing
            .checked_internal_client(&uuid::Uuid::new_v4().to_string(), target)
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), observed.changed()).await??;
    engine
        .set_ai_enabled(false)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    engine
        .set_ai_enabled(true)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    remote.resume.notify_one();
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), task).await??;
    assert!(matches!(result, Err(e) if e.code == "CLIENT_REVOKED"));
    Ok(())
}

#[tokio::test]
async fn internal_access_is_independent_bound_and_cannot_be_forged() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let targets: Vec<Target> = (0..2)
        .map(|i| Target {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("target-{i}"),
            group: None,
            host: "example.invalid".into(),
            port: 22,
            user: "test".into(),
            digest: format!("digest-{i}"),
        })
        .collect();
    let store = Store::new(dir.path())?;
    let external = store.register("test")?;
    let engine = fixture_engine(store, Arc::new(Fake(targets.clone())))?;
    let mut policy = engine.policy().map_err(|e| anyhow::anyhow!(e.code))?;
    policy.mode = Mode::Full;
    for t in &targets {
        policy.grant(t.id.clone(), t.digest.clone());
    }
    engine
        .set_policy(policy)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let conversation = uuid::Uuid::new_v4().to_string();
    assert!(
        engine
            .internal_client(&conversation, targets[0].clone())
            .is_err()
    );
    let list = || Call::parse("list_connections", json!({})).unwrap();
    engine
        .set_ai_enabled(true)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let ai = engine
        .internal_client(&conversation, targets[0].clone())
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let result = ai.call(list()).await.map_err(|e| anyhow::anyhow!(e.code))?;
    assert_eq!(result["connections"].as_array().unwrap().len(), 1);
    assert_eq!(result["connections"][0]["id"], targets[0].id);
    assert!(
        EngineHandler(engine.clone())
            .call(&external.id, list())
            .await
            .is_err()
    );
    assert!(
        EngineHandler(engine.clone())
            .call(&format!("ai:{conversation}"), list())
            .await
            .is_err()
    );
    let other = Call::parse(
        "open_session",
        json!({"connection_id":targets[1].id,"request_id":uuid::Uuid::new_v4().to_string()}),
    )
    .map_err(|e| anyhow::anyhow!(e.code))?;
    assert_eq!(
        ai.call(other).await.unwrap_err().code,
        "CONNECTION_NOT_ALLOWED"
    );
    let mut policy = engine.policy().map_err(|e| anyhow::anyhow!(e.code))?;
    policy.enabled = true;
    engine
        .set_policy(policy)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    engine
        .set_ai_enabled(false)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert!(ai.call(list()).await.is_err());
    engine
        .set_ai_enabled(true)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert!(
        ai.call(list()).await.is_err(),
        "reenabling must not resurrect a prior turn capability"
    );
    let fresh = engine
        .internal_client(&conversation, targets[0].clone())
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert!(fresh.is_valid());
    let mut policy = engine.policy().map_err(|e| anyhow::anyhow!(e.code))?;
    policy.grants.remove(&targets[0].id);
    engine
        .set_policy(policy)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert!(
        fresh.is_valid(),
        "MCP grants cannot revoke global AI access"
    );
    assert!(
        engine
            .internal_client(&conversation, targets[0].clone())
            .is_ok()
    );
    assert!(
        EngineHandler(engine.clone())
            .call(&external.id, list())
            .await
            .is_ok()
    );
    assert!(dir.path().join("mcp/ai-access.json").exists());
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
