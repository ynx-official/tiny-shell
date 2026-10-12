# MCP 源码运行桥接回归

> 状态：Review
> 最后更新：2026-10-12
> 关联：[MCP 开发契约](../03-architecture/mcp-contracts.md)、[构建与打包](../05-operations/build-and-package.md)、[实施与验收](mcp-implementation.md)

## 问题与修正

用户通过普通 `cargo run` 启动根应用，在 Codex CLI 注册行复制配置时报“未找到 MCP 桥接程序”。现场运行的是 `target/debug/tiny-shell.exe`，同目录没有 `tiny-shell-mcp.exe`；其他 Cargo target 目录中的 helper 不会被当前程序采用。根应用依赖共享库不等于 Cargo 会构建共享包的二进制目标。

复制配置现在优先使用同目录独立 helper；缺少时使用当前主程序及 `--mcp --client <注册 UUID>`。主程序在 GUI 和日志初始化前直接运行与 helper 共用的 stdio 入口，保留同样的注册验证、撤销检查和认证 IPC。AppImage 配置始终引用外层入口。配置依旧是已说明的通用 mcpServers JSON，本次没有增加品牌格式模板或写入第三方配置。

当前旧窗口的同目录 helper 已补齐，无需强制退出用户的窗口即可恢复原有复制路径。永久修复在重新构建的主程序中生效，以后普通 `cargo run` 不再要求独立 helper。

## 回归证据

- 先抽取原有配置生成逻辑，再运行同一组回归断言。源码运行及 AppImage 缺少 helper 两项均因 `MCP helper is missing` 失败；完整安装包路径通过。
- 真实子进程回归把主应用复制到包含空格及中文名字的隔离目录，只放主 exe，不放 helper。旧实现以退出码 1 关闭 stdout；修复后完成 initialize、十个工具发现和 get_status 的离线错误返回，并正常退出。
- 已撤销的注册和未知 CLI 参数会在协议启动前拒绝，stdout 保持为空；不输出注册秘密。
- 两个入口的现场只读冒烟：首次检查时主应用已关闭，独立 helper 返回 APP_NOT_RUNNING；随后实际重新执行 `cargo run --locked`，新构建的 `target/debug/tiny-shell-mcp.exe` 与新主程序 `--mcp` 均连接该应用，完成协议初始化、十个工具发现及 get_status，返回应用版本 1.6.10 和原有 full 模式。两个桥接子进程退出码均为 0，stderr 为空。没有打开远端会话、执行 SSH 命令或读写远端文件，没有修改权限与客户端注册。

## 验证结果

- `cargo test --workspace --locked --all-targets -- --include-ignored`：552 通过、0 失败、0 忽略，包含本次六项回归与拒绝场景。
- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --locked --all-targets -- -D warnings`：通过。
- 普通 `cargo run --locked`：重新构建并启动更新后的主应用，本机真实桥接调用通过。
- `cargo build --workspace --locked --release`：Windows 构建通过，主程序与独立 helper 均生成；发布主程序 `--mcp` 也完成同款现场初始化、十个工具发现和状态查询，正常退出且 stderr 为空。

测试代码与脱敏现场结果分别见 `tests/mcp_embedded_stdio.rs`、共享包的 launch 回归和忽略目录中的 `target/mcp-cargo-run-smoke.json`。完整测试、Clippy 与 release 输出位于 `target/mcp-cargo-run-tests.log`、`target/mcp-cargo-run-clippy.log`、`target/mcp-cargo-run-release.log`。

本次没有执行原生 UI 点击或其他操作系统的 stdio 运行验证，不把路径单元测试算作 AppImage 原生验证。已记录的启动期间 GPUI 借用告警不属于本次桥接缺失修复，仍见[现场冒烟的限制](mcp-live-smoke.md)。主应用保留运行，仅关闭本次验证的桥接子进程。没有改变应用版本、提交、推送或发布。

[返回文档索引](../README.md)
