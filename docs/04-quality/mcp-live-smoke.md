# TinyShell MCP 真实应用基础冒烟

> 状态：Review  
> 最后更新：2026-10-11  
> 测试日期：2026-10-10～2026-10-11（Asia/Shanghai）  
> 关联：[开发契约](../03-architecture/mcp-contracts.md)、[实施与验收](mcp-implementation.md)

## 目标与边界

用户明确授权对 TinyShell 中已勾选的“香港/777”SSH 连接进行实际 MCP 基础测试。本次通过项目发布版 `tiny-shell-mcp.exe --client <已注册客户端 UUID>` 的 stdio 协议进入真实应用桥接；没有使用 FileTerm、直接 SSH 或其他连接器替代 TinyShell。

使用已有的 Codex CLI 注册，不修改客户端、授权策略、凭据或连接配置。实际执行了不修改数据的命令、目录枚举、公共系统文本读取，并在 `/tmp` 使用唯一命名的测试文件验证创建、读回、版本更新和冲突，随后只清理本次创建的确切文件。没有使用 sudo、重启远端服务或操作业务文件。

**2026-10-11 完全访问模式下的真实 SSH/SFTP 基础冒烟通过**，测试文件已清理，MCP 会话已关闭。2026-10-10 的桥接离线诊断保留在本报告中；只读模式、受控模式与任务取消仍未实测。

## 环境与授权模式

| 项目 | 实际观察 |
| --- | --- |
| 平台 | Windows，本机发布版 helper 连接由 `cargo run --locked` 启动的 debug 主应用 |
| helper 路径 | `target/rust-1.96.1/release/tiny-shell-mcp.exe` |
| 主应用路径 | `target/rust-1.96.1/debug/tiny-shell.exe` |
| 运行时应用版本 | `get_status` 返回 `1.6.10`；独立 helper 版本为 `0.1.0` |
| 配置目录 | 当前用户默认 TinyShell 配置目录 |
| 客户端 | 已启用的 Codex CLI 注册；只读取注册的 id、label、enabled |
| 策略 | enabled=true、mode=full、scope=specified、revision=7 |
| 目标范围 | `list_connections` 返回唯一已授权目标，匹配用户选定的 777 连接；无下一页 |
| 应用状态 | 10 日发布版隐藏进程桥接不可用；11 日 debug 应用正常显示且响应，具有主窗口句柄和 MCP 命名管道，真实调用成功 |

没有读取或输出客户端 secret、凭据及原始 `sessions.json`。主机地址、账号、连接摘要和原始应用日志不进入本报告。

## 实际结果

2026-10-11 00:30～00:32（Asia/Shanghai），通过已有注册完成以下真实调用。异步任务均提交独有 `request_id`，通过 `get_task` 查到终态；没有仅凭任务票据判断成功，没有设置当前不支持的 `cwd`。

| 检查 | 结果 | 实际证据 |
| --- | --- | --- |
| `initialize` | 通过 | 协商 `2025-11-25`；serverInfo 为 `tiny-shell-mcp`，helper 版本 `0.1.0`；提供 tools capability |
| `notifications/initialized` | 通过 | 初始化后可继续发现和调用工具 |
| `tools/list` | 通过 | 返回 10 个预期工具，见下方清单 |
| stdio 协议与退出 | 通过 | stdout 均为可解析的 JSON-RPC；驱动记录的 12 次 helper 运行退出码均为 0，stderr 字节数均为 0 |
| 结构化结果 | 通过 | `structuredContent` 与 text content 中的 JSON 相同；`isError` 与 envelope 的 `ok` 一致 |
| `get_status` | 通过 | `ok=true`，应用版本 `1.6.10`、模式 `full`；文本限额 262144 字节、输出限额 1048576 字节 |
| `list_connections` 与目标唯一匹配 | 通过 | 返回 1 个已授权目标，名称匹配 777 且 ID 与已有唯一授权一致，`next_offset=null`；报告省略实际主机地址和账号 |
| `open_session` | 通过 | 任务 `running → succeeded`，取得独立 `session_id` |
| `get_task` | 通过 | 7 项异步任务均查到终态：6 项 `succeeded`，1 项旧版本更新按预期 `failed/FILE_CONFLICT` |
| 不修改数据的 `run_command` | 通过 | 仅打印测试标记并检查 `/tmp` 为目录；退出码 0，stdout 为预期两行，无截断 |
| `list_directory` 与分页 | 通过 | `/`、`/etc`、`/tmp` 均成功；每页限额 5，均查询下一页；`/tmp` 第二页 2 项且 `next_offset=null`；只记录数量和类型 |
| 安全文本 `read_file` | 通过 | 读取公共 `/etc/os-release`，UTF-8、LF、393 字节，返回完整内容和版本；报告不复制正文 |
| `write_file` 排他创建 | 通过 | `/tmp` 下唯一命名 `.txt`，create 任务成功，返回版本和 43 字节大小 |
| 测试文本读回 | 通过 | 读回内容与本次合成的英文/中文文本完全一致，UTF-8、LF、43 字节，版本与创建返回一致 |
| 带版本更新与读回 | 通过 | 使用首次读回的 `expected_version` 更新；任务成功，新版本改变；读回内容与第二版合成文本完全一致 |
| 旧版本冲突与内容保护 | 通过（预期拒绝） | 使用旧版本再次更新，任务 `failed/FILE_CONFLICT`；随后读回确认第二版内容及版本未变 |
| 测试文件清理 | 通过 | 先读回确认是本次合成内容，仅删除确切路径；删除后 `test ! -e` 成功、退出码 0；后续 `read_file` 返回 `FILE_ERROR` |
| `close_session` 与失效检查 | 通过 | 任务 `succeeded`，`result.closed=true`；使用旧 session 调用目录工具返回 `SESSION_NOT_FOUND` |
| 只读拒绝检查 | 未执行（模式不适用） | 实际策略为 full，不修改策略以构造 read_only 测试 |
| 受控确认检查 | 未执行（模式不适用） | 实际策略为 full，不修改策略以构造 controlled 测试 |
| `cancel_task` | 未执行 | 本次基础测试没有需要取消的长任务；不将工具发现算作取消行为通过 |
| 未保存凭据输入与主机密钥变更 | 未执行 | 本次连接直接成功，没有进入 `awaiting_user`；没有制造凭据缺失或修改 known_hosts |

发现的工具：`get_status`、`list_connections`、`open_session`、`close_session`、`run_command`、`get_task`、`cancel_task`、`list_directory`、`read_file`、`write_file`。

测试文件的确切路径为 `/tmp/tiny-shell-mcp-smoke-1c701f79-6829-4ee8-9163-b09d19bb95d9.txt`，已删除。本次只创建这一个远端文件；应用授权策略在测试后仍为 `enabled=true / full / specified / revision=7`。

## 启动前诊断与恢复

2026-10-10 23:41 前，启动前调用及发布版隐藏进程启动后的两轮调用均返回 `{ok:false,error:{code:"APP_NOT_RUNNING",message:"Start TinyShell and enable MCP."}}`。当时 Windows 主应用进程存在，但没有主窗口句柄；按规范化配置目录计算的预期管道不存在，系统中也没有任何 `tiny-shell-mcp-` 管道。主应用和 helper 的默认配置目录均来自当前用户 `.config/tiny-shell`，没有发现配置路径不一致的证据。

该轮只能证明 stdio helper 工作且应用桥接未就绪，不能据此判断 SSH、凭据或远端 SFTP 失败，也没有发生远端连接。2026-10-11 父对话改为按用户要求执行 `cargo run --locked`；本测试随后通过真实 `get_status` 和 `list_connections` 确认桥接恢复，再开始远端测试。没有启动重复实例，没有通过改变权限或直接 SSH 绕过桥接。

## 验证范围与后续确认

本次完全访问模式没有待用户确认的 MCP 任务，7 项任务均已终结。用户后续若要验收只读或受控模式，应在 TinyShell 中自行选择相应模式后另做验证；受控执行与缺失凭据输入必须使用应用正常界面，不绕过审批。

本报告保持 Review：没有完成全部权限矩阵、取消/超时、异常退出、跨客户端隔离、原生界面、品牌客户端配置和其他操作系统的真实验收。父对话观察到启动期间 GPUI `RefCell already borrowed` 共 42 次，后续窗口响应且输出未继续出现该告警；本次没有修复这一启动告警。其未阻断本次真实 MCP 操作，基础调用通过也不能代替原生界面专项验收。

测试结束保留用户要求运行的主应用；仅关闭本次创建的 MCP 会话和 helper 子进程。

本测试对话仅维护测试驱动、记录及文档索引，没有修改功能代码，没有另行运行 Rust 构建、Clippy 或单元测试；父对话的启动构建和此前实施验证不作为本次远端通过证据。文档状态、日期、相对链接和差异格式已检查。保留原有脏工作区，没有提交、推送或切换分支。

脱敏驱动、任务/会话状态和实际记录保留在忽略的 `target/mcp-live-smoke-driver.py`、`target/mcp-live-smoke-state.json`、`target/mcp-live-smoke-sanitized.jsonl`，供本机复核；不含客户端 secret 或认证凭据，日志对目标名称中的主机地址做脱敏。报告不复制远端内容或原始应用日志。

[返回文档索引](../README.md)
