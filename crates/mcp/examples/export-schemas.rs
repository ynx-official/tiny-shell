fn main() -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&tiny_shell_mcp::contracts::tools())?
    );
    Ok(())
}
