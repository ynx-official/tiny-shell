use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tiny_shell_mcp::{
    contracts::{Call, Failure, Result},
    engine::{Engine, EngineHandler, Remote, Target},
    ipc::Handler,
    policy::Mode,
    storage::Store,
    tasks::State,
};
use tokio::sync::watch;

struct Fake {
    target: Target,
    executions: AtomicUsize,
}
#[async_trait]
impl Remote for Fake {
    async fn targets(&self) -> Result<Vec<Target>> {
        Ok(vec![self.target.clone()])
    }
    async fn execute(
        &self,
        _target: &Target,
        session: &str,
        call: &Call,
        _cancel: watch::Receiver<bool>,
    ) -> Result<Value> {
        if matches!(call, Call::OpenSession(_)) {
            return Ok(json!({"session_id":session}));
        }
        if matches!(call, Call::RunCommand(_)) {
            self.executions.fetch_add(1, Ordering::SeqCst);
            return Ok(json!({"stdout":"你好 world","stderr":"","exit_code":0,"truncated":false}));
        }
        Ok(json!({}))
    }
    async fn close(&self, _session: &str) {}
}
async fn wait(engine: &Arc<Engine>, id: &str, state: State) -> anyhow::Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if engine
                .reviews()
                .is_ok_and(|r| r.iter().any(|r| r.id == id && r.state == state))
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}
fn parse(tool: &str, args: Value) -> anyhow::Result<Call> {
    Call::parse(tool, args).map_err(|e| anyhow::anyhow!(e.code))
}
fn error(e: Failure) -> anyhow::Error {
    anyhow::anyhow!(e.code)
}
#[tokio::test]
async fn controlled_approval_deduplication_and_client_isolation() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    let a = store.register("a")?;
    let b = store.register("b")?;
    let target = Target {
        id: uuid::Uuid::new_v4().to_string(),
        name: "test".into(),
        group: None,
        host: "127.0.0.1".into(),
        port: 22,
        user: "test".into(),
        digest: "test-digest".into(),
    };
    let remote = Arc::new(Fake {
        target: target.clone(),
        executions: AtomicUsize::new(0),
    });
    let engine = Engine::new(store, remote.clone())?;
    let mut policy = engine.policy().map_err(error)?;
    policy.enabled = true;
    policy.mode = Mode::Controlled;
    policy.grant(target.id.clone(), target.digest.clone());
    engine.set_policy(policy).map_err(error)?;
    let handler = EngineHandler(engine.clone());
    let open = parse(
        "open_session",
        json!({"connection_id":target.id,"request_id":uuid::Uuid::new_v4().to_string()}),
    )?;
    let ticket = handler.call(&a.id, open.clone()).await.map_err(error)?;
    let id = ticket["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing ticket"))?;
    wait(&engine, id, State::Succeeded).await?;
    let task = handler
        .call(&a.id, parse("get_task", json!({"task_id":id}))?)
        .await
        .map_err(error)?;
    let session = task["result"]["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing session"))?;
    let request_id = uuid::Uuid::new_v4().to_string();
    let command = parse(
        "run_command",
        json!({"session_id":session,"command":"touch file","request_id":request_id}),
    )?;
    let ticket = handler.call(&a.id, command.clone()).await.map_err(error)?;
    let id = ticket["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing ticket"))?;
    wait(&engine, id, State::AwaitingUser).await?;
    assert_eq!(remote.executions.load(Ordering::SeqCst), 0);
    assert_eq!(
        handler.call(&a.id, command.clone()).await.map_err(error)?,
        ticket
    );
    assert!(
        handler
            .call(&b.id, parse("get_task", json!({"task_id":id}))?)
            .await
            .is_err()
    );
    assert!(
        handler
            .call(&b.id, parse("cancel_task", json!({"task_id":id}))?)
            .await
            .is_err()
    );
    assert!(handler.call(&b.id,parse("run_command",json!({"session_id":session,"command":"ls","request_id":uuid::Uuid::new_v4().to_string()}))?).await.is_err());
    engine.approve(id, true).map_err(error)?;
    assert!(engine.approve(id, true).is_err());
    wait(&engine, id, State::Succeeded).await?;
    assert_eq!(remote.executions.load(Ordering::SeqCst), 1);
    assert_eq!(
        handler.call(&a.id, command.clone()).await.map_err(error)?,
        ticket
    );
    assert!(
        handler
            .call(
                &a.id,
                parse(
                    "run_command",
                    json!({"session_id":session,"command":"rm file","request_id":request_id})
                )?
            )
            .await
            .is_err()
    );
    let command = parse(
        "run_command",
        json!({"session_id":session,"command":"other","request_id":uuid::Uuid::new_v4().to_string()}),
    )?;
    let rejected = handler.call(&a.id, command).await.map_err(error)?;
    let id = rejected["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing ticket"))?;
    wait(&engine, id, State::AwaitingUser).await?;
    engine.approve(id, false).map_err(error)?;
    assert!(engine.approve(id, true).is_err());
    assert_eq!(remote.executions.load(Ordering::SeqCst), 1);
    let mut policy = engine.policy().map_err(error)?;
    policy.mode = Mode::ReadOnly;
    engine.set_policy(policy).map_err(error)?;
    // Policy changes invalidate existing sessions as well as approvals.
    assert!(handler.call(&a.id,parse("run_command",json!({"session_id":session,"command":"ls","request_id":uuid::Uuid::new_v4().to_string()}))?).await.is_err());
    assert_eq!(remote.executions.load(Ordering::SeqCst), 1);
    let opened = handler
        .call(
            &a.id,
            parse(
                "open_session",
                json!({"connection_id":target.id,"request_id":uuid::Uuid::new_v4().to_string()}),
            )?,
        )
        .await
        .map_err(error)?;
    let id = opened["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("ticket"))?;
    wait(&engine, id, State::Succeeded).await?;
    let opened = handler
        .call(&a.id, parse("get_task", json!({"task_id":id}))?)
        .await
        .map_err(error)?;
    let session = opened["result"]["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session"))?;
    let denied=handler.call(&a.id,parse("run_command",json!({"session_id":session,"command":"ls","request_id":uuid::Uuid::new_v4().to_string()}))?).await.err().ok_or_else(||anyhow::anyhow!("read-only accepted command"))?;
    assert_eq!(denied.code, "PERMISSION_DENIED");
    assert_eq!(remote.executions.load(Ordering::SeqCst), 1);
    let audit = std::fs::read_to_string(engine.store.root().join("audit.jsonl"))?;
    assert!(!audit.contains("touch file"));
    assert!(!audit.contains("你好"));
    assert!(!audit.contains(&a.secret));
    engine.store.clear_audit()?;
    assert_eq!(std::fs::read(engine.store.root().join("audit.jsonl"))?, b"");
    let opened = handler
        .call(
            &b.id,
            parse(
                "open_session",
                json!({"connection_id":target.id,"request_id":uuid::Uuid::new_v4().to_string()}),
            )?,
        )
        .await
        .map_err(error)?;
    let id = opened["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("ticket"))?;
    wait(&engine, id, State::Succeeded).await?;
    let opened = handler
        .call(&b.id, parse("get_task", json!({"task_id":id}))?)
        .await
        .map_err(error)?;
    let session = opened["result"]["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session"))?;
    engine.revoke_client(&a.id).map_err(error)?;
    assert!(
        handler
            .call(&a.id, parse("get_status", json!({}))?)
            .await
            .is_err()
    );
    assert!(
        handler
            .call(
                &b.id,
                parse("read_file", json!({"session_id":session,"path":"/text"}))?
            )
            .await
            .is_ok()
    );
    Ok(())
}
