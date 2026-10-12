use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tiny_shell_mcp::{
    contracts::{Call, Result},
    engine::{AiPolicy, Engine, InternalClient, Remote, Target},
    policy::Mode,
    storage::Store,
    tasks::State,
};
use tokio::sync::{Notify, watch};

struct RemoteFixture {
    target: Target,
    target_reads: AtomicUsize,
    release: Notify,
}
#[async_trait]
impl Remote for RemoteFixture {
    async fn targets(&self) -> Result<Vec<Target>> {
        self.target_reads.fetch_add(1, Ordering::SeqCst);
        Ok(vec![self.target.clone()])
    }
    async fn execute(
        &self,
        _: &Target,
        session: &str,
        call: &Call,
        _: watch::Receiver<bool>,
    ) -> Result<Value> {
        if matches!(call, Call::OpenSession(_)) {
            return Ok(json!({"session_id":session}));
        }
        self.release.notified().await;
        Ok(json!({"stdout":"finished","stderr":"","exit_code":0}))
    }
    async fn close(&self, _: &str) {}
}
fn call(name: &str, args: Value) -> anyhow::Result<Call> {
    Call::parse(name, args).map_err(|e| anyhow::anyhow!(e.code))
}
async fn state(engine: &Engine, task: &str, expected: State) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if engine
                .reviews()
                .is_ok_and(|r| r.iter().any(|r| r.id == task && r.state == expected))
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}
async fn fixture(
    dir: &std::path::Path,
) -> anyhow::Result<(Arc<Engine>, InternalClient, Arc<RemoteFixture>, String)> {
    let target = Target {
        id: uuid::Uuid::new_v4().to_string(),
        name: "fixture".into(),
        group: None,
        host: "example.invalid".into(),
        port: 22,
        user: "test".into(),
        digest: "fixture".into(),
    };
    let remote = Arc::new(RemoteFixture {
        target: target.clone(),
        target_reads: AtomicUsize::new(0),
        release: Notify::new(),
    });
    let engine = fixture_engine(Store::new(dir)?, remote.clone())?;
    engine
        .set_ai_policy(AiPolicy {
            enabled: true,
            mode: Mode::Full,
        })
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let client = engine
        .internal_client(&uuid::Uuid::new_v4().to_string(), target.clone())
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let opened = client
        .call(call(
            "open_session",
            json!({"connection_id":target.id,"request_id":uuid::Uuid::new_v4().to_string()}),
        )?)
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let task = opened["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("task missing"))?;
    state(&engine, task, State::Succeeded).await?;
    let snapshot = client
        .call(call("get_task", json!({"task_id":task}))?)
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let session = snapshot["result"]["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session missing"))?
        .to_owned();
    Ok((engine, client, remote, session))
}

#[tokio::test]
async fn task_queries_do_not_reload_remote_connections() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let (engine, client, remote, session) = fixture(directory.path()).await?;
    let task = client.call(call("run_command", json!({"session_id":session,"request_id":uuid::Uuid::new_v4().to_string(),"command":"fixture"}))?).await.map_err(|e| anyhow::anyhow!(e.code))?;
    let id = task["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("task missing"))?;
    state(&engine, id, State::Running).await?;
    let before = remote.target_reads.load(Ordering::SeqCst);
    for _ in 0..10 {
        client
            .call(call("get_task", json!({"task_id":id}))?)
            .await
            .map_err(|e| anyhow::anyhow!(e.code))?;
    }
    assert_eq!(
        remote.target_reads.load(Ordering::SeqCst),
        before,
        "task status must not reload the connection store"
    );
    remote.release.notify_one();
    Ok(())
}

#[tokio::test]
async fn bounded_wait_wakes_on_completion_and_preserves_output() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let (engine, client, remote, session) = fixture(directory.path()).await?;
    let ticket = client.call(call("run_command", json!({"session_id":session,"request_id":uuid::Uuid::new_v4().to_string(),"command":"fixture"}))?).await.map_err(|e| anyhow::anyhow!(e.code))?;
    let id = ticket["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("task missing"))?
        .to_owned();
    state(&engine, &id, State::Running).await?;
    let waiting = client.clone();
    let query = call("get_task", json!({"task_id":id,"wait_ms":10000}))?;
    let job = tokio::spawn(async move { waiting.call(query).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        !job.is_finished(),
        "long poll returned an unchanged running state"
    );
    remote.release.notify_one();
    let result = tokio::time::timeout(Duration::from_secs(1), job)
        .await??
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert_eq!(result["state"], "succeeded");
    assert_eq!(result["output"][0]["text"], "finished");
    Ok(())
}

#[tokio::test]
async fn waiting_rejects_foreign_tasks_and_wakes_on_revocation() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let (engine, client, remote, session) = fixture(directory.path()).await?;
    let ticket = client.call(call("run_command", json!({"session_id":session,"request_id":uuid::Uuid::new_v4().to_string(),"command":"fixture"}))?).await.map_err(|e| anyhow::anyhow!(e.code))?;
    let id = ticket["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("task missing"))?
        .to_owned();
    state(&engine, &id, State::Running).await?;
    let other = engine
        .internal_client(&uuid::Uuid::new_v4().to_string(), remote.target.clone())
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert_eq!(
        other
            .call(call("get_task", json!({"task_id":id,"wait_ms":10000}))?)
            .await
            .unwrap_err()
            .code,
        "TASK_NOT_FOUND"
    );
    let query = call("get_task", json!({"task_id":id,"wait_ms":10000}))?;
    let job = tokio::spawn(async move { client.call(query).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    engine
        .set_ai_enabled(false)
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let failure = tokio::time::timeout(Duration::from_secs(1), job)
        .await??
        .unwrap_err();
    assert_eq!(failure.code, "CLIENT_REVOKED");
    remote.release.notify_one();
    Ok(())
}

#[tokio::test]
async fn wait_duration_is_bounded_and_completed_tasks_return_immediately() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let (engine, client, remote, session) = fixture(directory.path()).await?;
    let ticket = client.call(call("run_command", json!({"session_id":session,"request_id":uuid::Uuid::new_v4().to_string(),"command":"fixture"}))?).await.map_err(|e| anyhow::anyhow!(e.code))?;
    let id = ticket["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("task missing"))?;
    state(&engine, id, State::Running).await?;
    let started = tokio::time::Instant::now();
    let query = call("get_task", json!({"task_id":id,"wait_ms":30}))?;
    let result = tokio::time::timeout(Duration::from_secs(1), client.call(query))
        .await?
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert_eq!(result["state"], "running");
    assert!(started.elapsed() >= Duration::from_millis(30));
    remote.release.notify_one();
    state(&engine, id, State::Succeeded).await?;
    let query = call("get_task", json!({"task_id":id,"wait_ms":10000}))?;
    let result = tokio::time::timeout(Duration::from_millis(200), client.call(query))
        .await?
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert_eq!(result["state"], "succeeded");
    assert!(Call::parse("get_task", json!({"task_id":id,"wait_ms":10001})).is_err());
    assert!(Call::parse("get_task", json!({"task_id":id,"wait_ms":-1})).is_err());
    Ok(())
}

#[tokio::test]
async fn waiting_observes_cancellation_without_waiting_for_the_remote_result() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let (engine, client, remote, session) = fixture(directory.path()).await?;
    let ticket = client.call(call("run_command", json!({"session_id":session,"request_id":uuid::Uuid::new_v4().to_string(),"command":"fixture"}))?).await.map_err(|e| anyhow::anyhow!(e.code))?;
    let id = ticket["task_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("task missing"))?
        .to_owned();
    state(&engine, &id, State::Running).await?;
    let waiting = client.clone();
    let query = call("get_task", json!({"task_id":id,"wait_ms":10000}))?;
    let job = tokio::spawn(async move { waiting.call(query).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    client
        .call(call("cancel_task", json!({"task_id":id}))?)
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?;
    let result = tokio::time::timeout(Duration::from_secs(1), job)
        .await??
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert_eq!(result["state"], "cancel_requested");
    remote.release.notify_one();
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
