# 内置 AI 实现契约

> 状态：Review
> 最后更新：2026-10-12
> 关联：[需求](../01-requirements/ai-conversations.md)、[交互](../02-design/ai-conversations.md)、[MCP 契约](mcp-contracts.md)、[安全边界](ai-threat-model.md)、[验证](../04-quality/ai-conversations.md)

## 职责与请求路径

- `src/ai/model.rs`：Provider、模型能力、绑定目标、消息及轮次状态。
- `src/ai/provider.rs`：OpenAI 兼容 HTTP/SSE、模型发现及固定消息测试。不开启重定向，不发送内部凭据。
- `src/ai/storage.rs`：系统凭据、本机认证加密、受限读取、原子保存和临时模式。
- `src/ai/service.rs` 与 `service/runner.rs`：后台运行、取消、快照、消息上下文与工具闭环。`service/toolset.rs` 定义内置工具边界，`service/tools.rs` 管理会话和任务等待，`service/budget.rs` 负责预算与结果复用。UI 线程不等待网络或凭据存储。
- `src/app/ai/`、`src/app/settings/ai.rs`：原生右栏、历史和 Provider 草稿；UI 只读取快照、发出操作。
- `crates/mcp/src/engine/internal.rs`：不可通过 IPC 构造的内部能力；共享引擎继续负责 SSH、文件、审批、取消和审计。

新对话从当前终端的连接 ID 查找已保存的 SSH；主机、用户、端口及代理身份必须一致，绑定保存态的授权摘要，不把终端展开的托管私钥或仅本次登录的口令计入新的身份。主机/代理已修改、临时或 RDP 连接不隐式绑定。旧版已记录错误摘要的对话需新建，不能静默改绑历史目标。发送时固定 Provider/模型和目标快照。具备工具能力且内置开关已开、绑定目标与当前已保存连接的摘要一致时才取得 `InternalClient`、附带目标信息和工具 Schema。模型调用 → 累积完整参数 → 内置工具参数校验 → 应用建立/复用会话并填入 ID → 原有 Engine → 通知等待任务终态 → 工具结果回送模型。`request_id` 由应用生成 UUID，防止模型复用请求造成意外幂等匹配。

内部来源是 `ai:<conversation UUID>`，外部 EngineHandler 拒绝此命名空间。任务和 SSH 会话沿用 client 字段隔离；对话结束或取消后关闭自身独立 SSH 会话，人工终端不受此生命周期控制。AI 全局模式/启停代数绑定内部能力，关闭后再打开不恢复旧轮能力。Engine 按来源生成有效 Policy：AI 只有当前能力绑定的目标及 AI 模式，外部沿用原机器授权及模式。发放能力前以及执行前核对当前保存目标摘要。两种来源的权限变更仅取消自身任务、审批和会话；审计记录任务创建时的模式。

## 数据与存储

`Provider` 包含 UUID、名称、Base URL、启用状态、本机 HTTP/无 Key 选项、模型数组和默认模型。模型包含 ID 与 `supports_tools`，没有 API Key 字段。Provider 操作状态/反馈与历史重命名及删除反馈分开，历史完成不会解除进行中的 Provider 请求。发现列表使用独立快照，关联 Provider UUID 和地址，不使用拼接文案传递模型数据。

`Conversation` 包含 UUID、标题、可选 BoundTarget、Provider UUID、模型、messages、activities、state、error 和更新时间。BoundTarget 仅保存连接 ID、公开名称/主机/端口/用户名与授权摘要，不保存密码、私钥或代理凭据。Message 保存角色、文本、完整 tool_calls 与 tool_call_id。

Activity 增加可选 `message_index` 与 `tool_call_id`，由 runner 创建活动时写入，关联原助手消息中的调用。两字段采用 serde 默认值，旧加密历史可直接读取，无需迁移密文格式。`src/app/ai/tool_steps.rs` 将消息与活动投影为文本/工具组，不改写模型上下文；`tool_steps/view.rs` 独立保存展开状态与审阅回调。结果中的失败/终态优先于旧活动状态，缺失或截断结果不能凭空推断成功。旧轮次未完成的步骤不会随新轮次变为运行中。

ToolGroupView 的根布局禁止纵向 flex 压缩，长答复由外层聊天滚动区承载。根节点同时裁剪圆角内的内容；若允许 flex 压缩，长答复会把工具组高度压至零，留下可见边框。回归使用真实 GPUI 滚动容器，覆盖运行 → 完成 → 追加长 Markdown 答复 → 展开结果。

`src/app/ai/composer.rs` 复用 InputState 及按钮，负责输入容器排布和临时反馈。TurnFeedback 在非活动轮为 Hidden；活动轮从最后一个 user 消息开始查找 assistant 的非空正文，没有正文时为 Waiting，已有正文时为 Dots。工具结果和历史旧轮不计入首次正文，后续模型请求不恢复 Waiting。AiPanel 将点附在最末助手回复容器中；末项为工具或异常时，使用无助手标识、无文案的独立点节点。只有 Waiting 使用两行助手标识/处理文案。反馈不写入 messages、不进入模型请求或持久化。三个点使用 GPUI 重复动画依次切换透明度，节点移除后自然停止，不新增后台定时器。输入使用 appearance(false) 去掉内部边框及禁用灰底，实际禁用语义和发送/停止事件保持原实现。

`src/app/ai/scroll.rs` 统一聊天滚动策略。Generating 状态每次渲染都请求 ScrollHandle::scroll_to_bottom，不依赖消息长度变化或旧视口是否接近底部；GPUI 在本帧布局完成后按新的滚动范围定位，覆盖较大 SSE 分片和正文未变但 Markdown 重新排版的情况。其他状态保留原有规则：切换对话定位到底部，内容变化且原位置接近底部时跟随，否则保留历史阅读位置。无需后台定时器或新增持久化状态。

配置目录沿用 `~/.config/tiny-shell`：

| 位置 | 内容 |
| --- | --- |
| `ai/providers.json` | Provider 元数据；不含 Key。 |
| `ai/conversations/<UUID>.enc` | `TSAI1` 标记、24 字节随机 nonce、XChaCha20-Poly1305 密文；AAD 为对话 UUID，单文件读取上限 16 MiB。 |
| 系统凭据存储 | Provider Key 与随机 32 字节历史密钥；服务名包含配置目录规范路径的 SHA-256。 |
| `mcp/ai-access.json` | 内置访问开关和全局模式，schema_version=2；v1 一次性迁移，保留原开关并复制当时共享模式，后续独立；原 `mcp/policy.json` 格式不变。 |

Provider 与历史保存在独立目录，不进入 ConfigStore 的同步/归档对象。持久化串行锁隔离保存与删除，I/O 前复制快照并释放状态锁，避免删除后由迟到检查点重新创建历史。凭据不可用时 Key 可仅在内存中使用；无历史主密钥时不落盘聊天，不生成替代密钥覆盖已有密文。

## 轮次、取消与恢复

状态：Idle → Generating → RunningTool/AwaitingApproval → Generating → Idle；预算边界可进入 AwaitingContinuation，选择继续后恢复同一 runner；取消为 Interrupted，异常为 Failed。单对话取消通道在整个轮次完成后移除；面板隐藏不改变通道。显式停止、紧急停止和最后一个工作区窗口关闭请求取消。

重启只加载历史快照，活动轮标记 Interrupted，不启动 runner。继续对话时，对未配齐结果的 tool_calls 添加结果未知占位消息，保持模型协议完整，绝不重放操作。保存失败单独显示存储错误，不覆盖 Provider 操作状态。

网络连接超时 15 秒、请求总超时 300 秒、SSE 每次读取超时 60 秒。输出及普通 JSON 响应限制 1 MiB，单工具参数限制 256 KiB、单响应最多 32 个工具调用；单轮模型请求预算默认为八次，可配置并由用户逐次追加。既有上下文选择最近最多 16 个完整用户轮，目标预算 256 KiB；单个较大轮保留，工具回送超过 64 KiB 时缩短正文并保留合法 JSON、版本、编码、分页及退出码等元数据，明确标记 model_result_truncated。历史正文不因上下文裁剪而删除。MCP 的文本、任务、会话及审批限制保持原契约。

## 调用链优化（2026-10-12）

用户确认的范围为工具描述、状态等待、会话管理和重复调用控制。此次不新增业务工具或更改审批权限，工程阈值最初采用单用户轮 16 个逻辑工具调用、同参数同结果累计三次停止、只读结果一秒缓存及最多四个缓存条目；后续已批准预算修订将 16 次固定终止改为可配置的暂停/追加预算，其他保护不变。

内置只读模式提供 read_file/list_directory；受控与完全访问模式另提供 run_command/write_file。描述承诺返回完成结果，模型不负责状态查询、连接选择和会话生命周期。Schema 隐藏 connection_id/session_id/task_id/request_id/cwd；尝试提供这些字段或非当前模式操作会在建立 SSH 前拒绝。cwd 沿用登录目录，命令仍可自行显式切换目录。

每个用户轮第一次有效操作惰性建立一个绑定目标的独立会话，后续操作复用；正常结束、停止或异常统一关闭。会话不跨轮保存，旧历史的生命周期 ID 不接受为新操作参数。保持内部 client 隔离、执行前目标校验、受控逐次审批、版本冲突和审计边界。只有最初状态快照立即查询，后续使用 get_task(wait_ms=10000)；取消和内部能力撤销直接唤醒等待，无固定 200ms 轮询。

缓存仅保存成功的 read_file/list_directory 结果；同参数在最初结果的一秒窗口内可复用，命令或写入尝试前清空，变更操作的成功结果也不缓存。缓存命中仍计入逻辑预算。相同操作/参数反复得到相同结果三次，或任一操作出现 outcome_unknown/timed_out/interrupted/cancel_requested 等不确定结果，停止本轮后续操作；不自动用新 request_id 重放。预算耗尽/停止后的批内调用仍有匹配的 not_executed 结果，模型请求保留完整 tool_call_id 配对。

触发重复结果/结果未知的安全停止后，不再提供工具，要求总结有证据的结果、失败及未执行步骤。模型若仍发工具调用，返回未执行结果而不触发远端 I/O。单纯达到工具/模型预算则暂停等待选择，继续规则见下节。UI 保留显式 Unknown/TimedOut/Skipped 状态，错误对象不能把它们覆盖成普通失败。原优化阶段证据见[调用链优化验证](../04-quality/ai-tool-call-optimization.md)。

## 可恢复预算与平台提醒（2026-10-12）

`Settings.execution` 采用 serde 默认值，保存在既有 `ai/providers.json` 中；与 Provider 草稿分开保存，复用持久化串行锁。发送时取得不可变预算快照（工具 1–128，默认 16；模型 1–64，默认 8），背景通知偏好则按最新已保存设置读取。

`Budget` 区分可扩展的调用 ceiling 与不可绕过的 safety_halted。继续不清空缓存、累计调用数、相同结果计数或 uncertain 指纹。runner 在下一次模型请求或批内下一次工具执行之前检查预算，保存带随机 token 的 Continuation，并通过独立 oneshot 等待；与取消 watch、内部能力撤销一起 select。Service 在锁内核对 token 与取消状态，只接受一次选择。未选择不发请求、不执行工具；恢复使用同一消息序列、待执行批次及已打开的独立 SSH 会话。批内暂停终止时为剩余调用保存 not_executed 配对结果，不向 Provider 请求总结。重启加载活动状态时清空 continuation 并标记中断，不恢复 runner。

`src/app/ai/attention.rs` 负责所属窗口、去重、已有弹窗避让、过期弹窗移除和通知点击导航。只读取轻量 RunAttention，避免每次主窗口渲染复制全部历史。弹窗使用既有 modal token 及 ConfirmationDialog；窗口/实体更新由 defer 调度，避免 GPUI 重入。点击旧通知仅校验并导航，不能追加预算。

`src/app/ai/notifications.rs` 隔离 Windows/macOS 平台适配与异步回调。Windows 通过 `tauri-winrt-notification 0.8.1` 使用 WinRT Toast，本用户注册 TinyShell.Desktop 身份，安装脚本同步快捷方式 AUMID 和卸载清理；不借用 PowerShell 等应用身份。macOS 通过 `mac-usernotifications 0.3.1` 使用 UNUserNotificationCenter，首次主动开启/测试请求授权，后台发送只查询授权状态；直接执行非 .app 二进制时反馈包要求。系统调用运行在后台，不阻塞 UI；macOS 通知响应最长观察一小时，超时或清除不会恢复轮次，应用内暂停无超时。Linux 不引入通知依赖，保留应用内选择。

平台适配结果、未运行项目及重点手动验收见[预算与通知验证](../04-quality/ai-execution-budget.md)。

JSON 回退与 SSE 共用完整性检查：禁止截断完成原因、重复 tool_call_id、非对象参数及不完整 JSON。只有用户主动测试、刷新或发送才调用 Provider。正式协议依据为 [Chat Completions](https://platform.openai.com/docs/api-reference/chat/create)，凭据适配为 [keyring 4.2.0](https://docs.rs/keyring/4.2.0/keyring/)。
