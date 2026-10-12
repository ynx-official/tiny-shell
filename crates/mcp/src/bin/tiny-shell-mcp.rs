fn main() -> anyhow::Result<()> {
    tiny_shell_mcp::stdio::run(std::env::args().skip(1))
}
