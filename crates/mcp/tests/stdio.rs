use serde_json::{Value, json};
use std::process::Stdio;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

#[tokio::test]
async fn helper_performs_real_stdio_handshake_and_reports_offline_app() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let registration =
        tiny_shell_mcp::storage::Store::new(directory.path())?.register("stdio-test")?;
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
