use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tiny_shell_mcp::{
    policy::Policy,
    storage::{Registration, Store},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
};

struct Fixture {
    directory: tempfile::TempDir,
    store: Store,
    registration: Registration,
}

impl Fixture {
    fn new(enabled: bool) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let store = Store::new(directory.path())?;
        store.save_policy(&Policy {
            enabled,
            ..Policy::default()
        })?;
        let registration = store.register("stdio-test")?;
        Ok(Self {
            directory,
            store,
            registration,
        })
    }

    fn set_enabled(&self, enabled: bool) -> Result<()> {
        let mut policy = self.store.load_policy()?;
        policy.enabled = enabled;
        policy.revision += 1;
        self.store.save_policy(&policy)
    }

    fn start(&self) -> Result<BridgeProcess> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tiny-shell-mcp"))
            .args(["--client", &self.registration.id, "--config-dir"])
            .arg(self.directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let input = child.stdin.take().context("stdin unavailable")?;
        let output = BufReader::new(child.stdout.take().context("stdout unavailable")?).lines();
        Ok(BridgeProcess {
            child,
            input,
            output,
        })
    }
}

struct BridgeProcess {
    child: Child,
    input: ChildStdin,
    output: Lines<BufReader<ChildStdout>>,
}

impl BridgeProcess {
    async fn send(&mut self, message: Value) -> Result<()> {
        self.input
            .write_all(format!("{message}\n").as_bytes())
            .await?;
        self.input.flush().await?;
        Ok(())
    }

    async fn request(&mut self, message: Value) -> Result<Value> {
        let id = message["id"].clone();
        self.send(message).await?;
        let line = tokio::time::timeout(Duration::from_secs(10), self.output.next_line())
            .await??
            .context("helper closed stdout before replying")?;
        let response: Value = serde_json::from_str(&line)?;
        assert_eq!(response["id"], id);
        Ok(response)
    }

    async fn initialize(&mut self, complete: bool) -> Result<()> {
        let response = self.request(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).await?;
        assert!(response["result"]["capabilities"]["tools"].is_object());
        if complete {
            self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                .await?;
            self.list_tools().await?;
        }
        Ok(())
    }

    async fn list_tools(&mut self) -> Result<()> {
        let response = self
            .request(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
            .await?;
        assert_eq!(
            response["result"]["tools"].as_array().map(Vec::len),
            Some(10)
        );
        Ok(())
    }

    async fn exits_with_open_input(self, success: bool) -> Result<()> {
        let Self {
            child,
            input,
            mut output,
        } = self;
        // Keep the client's pipe open: EOF must not be what allows the bridge to exit.
        let result =
            tokio::time::timeout(Duration::from_secs(5), child.wait_with_output()).await??;
        drop(input);
        assert_eq!(result.status.success(), success);
        assert!(output.next_line().await?.is_none());
        if success {
            assert!(result.stderr.is_empty());
        }
        Ok(())
    }
}

#[tokio::test]
async fn disabled_helper_exits_without_handshake_even_when_client_keeps_stdin_open() -> Result<()> {
    let fixture = Fixture::new(false)?;
    // Repeated client launch attempts must not leave resident services.
    for _ in 0..3 {
        fixture.start()?.exits_with_open_input(true).await?;
    }
    Ok(())
}

#[tokio::test]
async fn helper_without_a_policy_defaults_to_disabled() -> Result<()> {
    let fixture = Fixture::new(false)?;
    std::fs::remove_file(fixture.store.root().join("policy.json"))?;
    fixture.start()?.exits_with_open_input(true).await
}

#[tokio::test]
async fn disabling_mcp_exits_during_initialization_with_stdin_still_open() -> Result<()> {
    let fixture = Fixture::new(true)?;
    let mut bridge = fixture.start()?;
    bridge.initialize(false).await?;
    fixture.set_enabled(false)?;
    bridge.exits_with_open_input(true).await
}

#[tokio::test]
async fn disabling_mcp_exits_all_bridges_only_for_that_profile_and_allows_reenable() -> Result<()> {
    let fixture = Fixture::new(true)?;
    let other_profile = Fixture::new(true)?;
    let mut first = fixture.start()?;
    let mut second = fixture.start()?;
    let mut other = other_profile.start()?;
    first.initialize(true).await?;
    second.initialize(true).await?;
    other.initialize(true).await?;

    fixture.set_enabled(false)?;
    first.exits_with_open_input(true).await?;
    second.exits_with_open_input(true).await?;
    other.list_tools().await?;
    fixture.start()?.exits_with_open_input(true).await?;

    fixture.set_enabled(true)?;
    let mut restarted = fixture.start()?;
    restarted.initialize(true).await?;
    fixture.set_enabled(false)?;
    restarted.exits_with_open_input(true).await?;
    other_profile.set_enabled(false)?;
    other.exits_with_open_input(true).await
}

#[tokio::test]
async fn revoking_a_client_exits_its_bridge_without_disabling_other_clients() -> Result<()> {
    let mut fixture = Fixture::new(true)?;
    let mut revoked = fixture.start()?;
    revoked.initialize(true).await?;
    let revoked_id = fixture.registration.id.clone();
    fixture.registration = fixture.store.register("another-client")?;
    let mut active = fixture.start()?;
    active.initialize(true).await?;

    fixture.store.revoke(&revoked_id)?;
    revoked.exits_with_open_input(true).await?;
    active.list_tools().await?;
    fixture.set_enabled(false)?;
    active.exits_with_open_input(true).await
}

#[tokio::test]
async fn unreadable_policy_stops_an_initialized_bridge() -> Result<()> {
    let fixture = Fixture::new(true)?;
    let mut bridge = fixture.start()?;
    bridge.initialize(true).await?;
    tiny_shell_mcp::storage::atomic_bytes(&fixture.store.root().join("policy.json"), b"{")?;
    bridge.exits_with_open_input(false).await
}

#[tokio::test]
async fn helper_performs_real_stdio_handshake_and_reports_offline_app() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    store.save_policy(&Policy {
        enabled: true,
        ..Policy::default()
    })?;
    let registration = store.register("stdio-test")?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_tiny-shell-mcp"))
        .args(["--client", &registration.id, "--config-dir"])
        .arg(directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("stdin unavailable"))?;
    let mut output = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("stdout unavailable"))?,
    )
    .lines();
    for request in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_status","arguments":{}}}),
    ] {
        input.write_all(format!("{request}\n").as_bytes()).await?;
        input.flush().await?;
        if request.get("id").is_none() {
            continue;
        }
        let line = tokio::time::timeout(std::time::Duration::from_secs(10), output.next_line())
            .await??
            .ok_or_else(|| anyhow::anyhow!("helper closed stdout"))?;
        let response: Value = serde_json::from_str(&line)?;
        assert_eq!(response["id"], request["id"]);
        match response["id"].as_u64() {
            Some(1) => assert!(response["result"]["capabilities"]["tools"].is_object()),
            Some(2) => assert_eq!(
                response["result"]["tools"].as_array().map(Vec::len),
                Some(10)
            ),
            Some(3) => {
                assert_eq!(response["result"]["isError"], true);
                assert_eq!(
                    response["result"]["structuredContent"]["error"]["code"],
                    "APP_NOT_RUNNING"
                );
            }
            _ => panic!("unexpected response"),
        }
    }
    drop(input);
    tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await??;
    Ok(())
}
