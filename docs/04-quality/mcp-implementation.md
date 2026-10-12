# MCP 实施与验收

> 状态：Review  
> 最后更新：2026-10-12  
> 关联：[产品设计](../02-design/mcp-integration.md)、[开发契约](../03-architecture/mcp-contracts.md)

本文记录 2026-10-10 首轮实施基线。2026-10-12 的设置交互、三页签和客户端配置适配已继续实现，最新验证与仍未完成的原生验收见 [MCP 设置交互验证](mcp-settings-interaction.md)；下文的单页和通用 JSON 限制仅指首轮基线。

## 执行顺序

1. SDK 锁定、共享契约、权限策略和失败测试。
2. 私有注册存储、认证 IPC、stdio helper；真实进程握手测试。
3. 应用协调器、独立 SSH/SFTP 会话和十个工具。
4. 原生 MCP 设置、授权与审阅、双语、元数据历史。
5. 打包入口、完整质量门禁与隔离 SSH/SFTP 联调。

## 必须验证的行为

- 三模式 × 启用/禁用 × 授权/未授权权限矩阵；只读禁止任意命令，拒绝分支无远端 I/O。
- 连接新建/目标修改/重命名/分组/删除；新目标不能复用旧授权。
- 审批拒绝/关闭/超时/双击/策略变化，跨窗口批准一次执行。
- UUID/未知字段/路径/超时/内容上限验证；输出截断仍排空通道。
- 客户端 A 不能访问 B 会话、任务、取消与去重记录。
- 去重同请求/参数变化/重连；重启失效；不确定结果不自动重试。
- 超大/截断帧、错误 HMAC、重放、错误 generation、撤销注册、IPC 排他占用。
- SDK stdio initialize、tools/list、tools/call；应用离线和禁用返回结构化错误；stdout 纯协议。
- SSH 独立无 PTY 执行、非零退出/取消/超时；未存凭据的 UI 输入；主机密钥变更。
- 文本编码/换行/权限、排他新建、版本冲突、符号链接、保存后校验/恢复。
- 日志不含命令/正文/凭据，七天/大小清理；秘密文件权限；不进入同步/归档。
- 设置启用/模式/连接/注册/复制/撤销/审阅/停止全部/清空；中英、主题、720×520/980×700。

使用隔离临时目录与本地测试 SSH 服务；不访问用户真实保存服务器。单元测试先确认失败再实施。真实 IPC/stdio 不以 mock 代替，远端服务可在严格执行边界使用 fake 完成状态机测试，并另做实际 SSH 联调。

## 首轮实现范围

已实现独立 stdio helper、十个工具契约、当前用户私有注册与认证 IPC、三模式授权、连接目标摘要、客户端独立 AI SSH 会话、任务查询/审批/取消/去重、文本读写与版本冲突、元数据历史及清理。设置页已接入启用、模式、连接勾选、注册/复制/撤销、任务审阅、凭据输入、停止全部和清空记录，维护中英文。

本轮采用现有 SettingPage 的单页分组先交付可用入口；设计中的独立三页签、连接搜索、客户端检测与品牌配置模板仍属后续界面切片，不把已批准的产品设计静默改成已实现。通用 mcpServers JSON 已提供；Codex CLI 等客户端需要按自己的格式配置相同 command/args，尚未进行品牌客户端实测。

2026-10-12 已修复普通 `cargo run` 缺少独立 helper 时无法复制配置的问题：主程序可直接运行共享 `--mcp` stdio 入口，完整安装包继续优先使用 helper。更新后的测试与现场验证以[源码运行桥接回归](mcp-cargo-run-bridge.md)为准，下方保留 2026-10-10 首轮实施的历史快照。

## 实际验证

- 权限和任务终态先以失败测试确认，再实施；收尾审查发现“撤销 A 会影响 B”的问题，回归测试在旧行为下失败，按客户端取消后通过。断言未削弱。
- Rust 全工作区：`cargo test --workspace --locked --all-targets -- --include-ignored`，**546 通过、0 失败、0 忽略**，包含手动性能基准与新 SSH/SFTP 联调。
- 质量门禁：全 workspace 格式检查和严格 Clippy 已通过；原有八项 Python 发布脚本测试及 v1.6.10 发布资料一致性校验通过。
- 真正的子进程 stdio initialize / tools/list / tools/call 已通过；发布版 helper 也独立冒烟验证十个工具、离线错误和纯协议 stdout。
- 真正的 Windows 命名管道握手、端点排他占用、nonce/HMAC 绑定、超量帧、注册撤销与损坏配置 fail closed 已验证。错误 HMAC 的密码学验证已覆盖，不宣称做过另一操作系统用户的 ACL 穿透测试。
- 隔离 TCP loopback SSH/SFTP：独立非 PTY 命令、stdout/stderr、退出码、超时、目录、CRLF/权限保留、排他创建、旧版本拒绝和符号链接写入拒绝均通过。测试使用内存文件服务和临时 known_hosts，不连接真实保存服务器。
- Windows release 同时构建两个程序；主应用与 helper 在同一 release 目录。最终 `cargo build --workspace --locked --release --no-default-features` 通过（Windows CI 同款功能配置）。
- 打包脚本：Windows PowerShell 和 macOS/Linux Bash 语法检查通过，发布流水线和各平台脚本已添加 helper；本机缺少 Inno Setup，未创建安装器。macOS/Linux 未做原生打包运行；跨平台标准库下载持续无进展，已停止该可选下载，不把静态检查视为跨平台构建。

## 当前限制与未验证项

- `cwd` 尚不支持；连接配置没有经过确认的远程 Shell 类型，返回 UNSUPPORTED_SHELL。目录切换可以纳入完整 command 后审阅。
- 初版使用单页分组，尚未做原生 GPUI 实机视觉和键盘流程检查；720×520/980×700、主题、字号适配不能宣称通过。
- 命令断线/取消不保证远端进程已结束；文件读取和替换不是对所有远端进程的全局 CAS。
- 历史仅为元数据，任务正文在内存；应用运行时按最早记录到期清理，关闭期间不运行后台服务，下次启动清理已过期记录。
- 本次不发布安装包，不改应用版本、不提交、不推送、不创建标签。代码仍是 v1.6.10 工作区中的首轮实现；最终产品/多平台发布验收尚未完成。

## 覆盖率与产物

`cargo llvm-cov -p tiny-shell-mcp --locked --all-targets`：共享 MCP 包行覆盖率 **73.03%**，函数覆盖率 **70.62%**。报告包括该包测试代码，不包含 GPUI 设置和主应用 SSH/SFTP 服务，不是整项功能的生产代码覆盖率。Branch / MC/DC 未启用，不将零计数当作 0% 覆盖率。

本机可复核产物均在忽略的 target 目录：

- `target/rust-1.96.1/release/tiny-shell.exe` 与 `tiny-shell-mcp.exe`。
- `target/mcp-tests.log`、`target/mcp-clippy.log`、`target/mcp-release.log`。
- `target/mcp-coverage-summary.json`、`target/mcp-coverage.log`。
- `target/mcp-revoke-regression-failing.log`：旧的全局撤销行为稳定触发 B 会话失效，修复后的同一断言通过。

正式产品设计与开发契约已完善；本报告保持 Review，表示原生界面和多平台/品牌客户端发布验收尚未完成。
