use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
    process::Command,
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self> {
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-embedded-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory)?;
        Ok(Self(directory))
    }

    fn executable(&self) -> Result<PathBuf> {
        let executable = self.0.join(if cfg!(windows) {
            "Tiny Shell 开发.exe"
        } else {
            "Tiny Shell 开发"
        });
        std::fs::copy(env!("CARGO_BIN_EXE_tiny-shell"), &executable)?;
        Ok(executable)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // This UUID directory was created by this fixture, never supplied by a user.
        if let Ok(directory) = std::fs::canonicalize(&self.0)
            && let Ok(parent) = std::fs::canonicalize(std::env::temp_dir())
            && directory.parent() == Some(parent.as_path())
            && directory.file_name() == self.0.file_name()
        {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

#[tokio::test]
async fn application_serves_stdio_without_a_sibling_helper_or_gui() -> Result<()> {
    let fixture = Fixture::new()?;
    let executable = fixture.executable()?;
    let config_directory = fixture.0.join("configuration");
    let store = tiny_shell_mcp::storage::Store::new(&config_directory)?;
    store.save_policy(&tiny_shell_mcp::policy::Policy {
        enabled: true,
        ..Default::default()
    })?;
    let registration = store.register("stdio-test")?;
    let mut child = Command::new(&executable)
        .args(["--mcp", "--client", &registration.id, "--config-dir"])
        .arg(&config_directory)
        .env_remove("APPIMAGE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = child.stdin.take().context("stdin unavailable")?;
    let mut output = BufReader::new(child.stdout.take().context("stdout unavailable")?).lines();
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_status","arguments":{}}}),
    ];
    for request in requests {
        input.write_all(format!("{request}\n").as_bytes()).await?;
        input.flush().await?;
        if request.get("id").is_none() {
            continue;
        }
        let line = tokio::time::timeout(Duration::from_secs(10), output.next_line()).await??;
        let Some(line) = line else {
            let status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await??;
            anyhow::bail!("embedded bridge closed stdout before replying: {status}");
        };
        let response: Value = serde_json::from_str(&line)?;
        assert_eq!(response["id"], request["id"]);
        match response["id"].as_u64() {
            Some(1) => assert!(response["result"]["capabilities"]["tools"].is_object()),
            Some(2) => assert_eq!(
                response["result"]["tools"].as_array().map(Vec::len),
                Some(10)
            ),
            Some(3) => assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                "APP_NOT_RUNNING"
            ),
            _ => anyhow::bail!("unexpected response"),
        }
    }
    drop(input);
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output()).await??;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[tokio::test]
async fn application_stdio_exits_without_starting_transport_when_mcp_is_disabled() -> Result<()> {
    let fixture = Fixture::new()?;
    let executable = fixture.executable()?;
    let config_directory = fixture.0.join("configuration");
    let store = tiny_shell_mcp::storage::Store::new(&config_directory)?;
    let registration = store.register("disabled-test")?;
    let mut child = Command::new(executable)
        .args(["--mcp", "--client", &registration.id, "--config-dir"])
        .arg(config_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let input = child.stdin.take().context("stdin unavailable")?;
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output()).await??;
    drop(input);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[tokio::test]
async fn application_stdio_exits_on_disable_while_client_keeps_stdin_open() -> Result<()> {
    for initialized in [false, true] {
        let fixture = Fixture::new()?;
        let executable = fixture.executable()?;
        let config_directory = fixture.0.join("configuration");
        let store = tiny_shell_mcp::storage::Store::new(&config_directory)?;
        let mut policy = tiny_shell_mcp::policy::Policy {
            enabled: true,
            ..Default::default()
        };
        store.save_policy(&policy)?;
        let registration = store.register("disable-test")?;
        let mut child = Command::new(executable)
            .args(["--mcp", "--client", &registration.id, "--config-dir"])
            .arg(config_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut input = child.stdin.take().context("stdin unavailable")?;
        let mut output = BufReader::new(child.stdout.take().context("stdout unavailable")?).lines();
        let request = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}});
        input.write_all(format!("{request}\n").as_bytes()).await?;
        input.flush().await?;
        let line = tokio::time::timeout(Duration::from_secs(10), output.next_line())
            .await??
            .context("embedded bridge closed stdout before initialization")?;
        let response: Value = serde_json::from_str(&line)?;
        assert!(response["result"]["capabilities"]["tools"].is_object());
        if initialized {
            let notification = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
            let request = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
            input
                .write_all(format!("{notification}\n{request}\n").as_bytes())
                .await?;
            input.flush().await?;
            let line = tokio::time::timeout(Duration::from_secs(10), output.next_line())
                .await??
                .context("embedded bridge closed stdout before tools/list")?;
            let response: Value = serde_json::from_str(&line)?;
            assert_eq!(
                response["result"]["tools"].as_array().map(Vec::len),
                Some(10)
            );
        }
        policy.enabled = false;
        store.save_policy(&policy)?;
        // Do not send another request or close stdin to trigger the shutdown.
        let result =
            tokio::time::timeout(Duration::from_secs(5), child.wait_with_output()).await??;
        drop(input);
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        assert!(output.next_line().await?.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn application_stdio_rejects_revoked_registration_before_starting_transport() -> Result<()> {
    let fixture = Fixture::new()?;
    let executable = fixture.executable()?;
    let config_directory = fixture.0.join("configuration");
    let store = tiny_shell_mcp::storage::Store::new(&config_directory)?;
    let registration = store.register("revoked-test")?;
    store.revoke(&registration.id)?;
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new(executable)
            .args(["--mcp", "--client", &registration.id, "--config-dir"])
            .arg(config_directory)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&registration.secret));
    Ok(())
}

#[tokio::test]
async fn application_stdio_rejects_unknown_arguments_without_opening_a_window() -> Result<()> {
    let fixture = Fixture::new()?;
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new(fixture.executable()?)
            .args(["--mcp", "--unexpected"])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    Ok(())
}
