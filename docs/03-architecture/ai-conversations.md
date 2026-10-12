# 内置 AI 实现契约

> 状态：Review
> 最后更新：2026-10-12
> 关联：[需求](../01-requirements/ai-conversations.md)、[交互](../02-design/ai-conversations.md)、[MCP 契约](mcp-contracts.md)、[安全边界](ai-threat-model.md)、[验证](../04-quality/ai-conversations.md)

## 职责与请求路径

- `src/ai/model.rs`：Provider、模型能力、绑定目标、消息及轮次状态。
- `src/ai/provider.rs`：OpenAI 兼容 HTTP/SSE、模型发现及固定消息测试。不开启重定向，不发送内部凭据。
- `src/ai/storage.rs`：系统凭据、本机认证加密、受限读取、原子保存和临时模式。
- `src/ai/service.rs` 与 `service/runner.rs`：后台运行、取消、快照、消息上下文与工具闭环。UI 线程不等待网络或凭据存储。
- `src/app/ai/`、`src/app/settings/ai.rs`：原生右栏、历史和 Provider 草稿；UI 只读取快照、发出操作。
- `crates/mcp/src/engine/internal.rs`：不可通过 IPC 构造的内部能力；共享引擎继续负责 SSH、文件、审批、取消和审计。

新对话从当前终端的连接 ID 查找已保存的 SSH；主机、用户、端口及代理身份必须一致，绑定保存态的授权摘要，不把终端展开的托管私钥或仅本次登录的口令计入新的身份。主机/代理已修改、临时或 RDP 连接不隐式绑定。旧版已记录错误摘要的对话需新建，不能静默改绑历史目标。发送时固定 Provider/模型和目标快照。具备工具能力且内置开关已开、绑定目标与当前已保存连接的摘要一致时才取得 `InternalClient`、附带目标信息和工具 Schema。模型调用 → 累积完整参数 → `Call::parse` → 原有 Engine → 查询任务终态 → 工具结果回送模型。`request_id` 由应用生成 UUID，防止模型复用请求造成意外幂等匹配。

内部来源是 `ai:<conversation UUID>`，外部 EngineHandler 拒绝此命名空间。任务和 SSH 会话沿用 client 字段隔离；对话结束或取消后关闭自身独立 SSH 会话，人工终端不受此生命周期控制。AI 全局模式/启停代数绑定内部能力，关闭后再打开不恢复旧轮能力。Engine 按来源生成有效 Policy：AI 只有当前能力绑定的目标及 AI 模式，外部沿用原机器授权及模式。发放能力前以及执行前核对当前保存目标摘要。两种来源的权限变更仅取消自身任务、审批和会话；审计记录任务创建时的模式。

## 数据与存储

`Provider` 包含 UUID、名称、Base URL、启用状态、本机 HTTP/无 Key 选项、模型数组和默认模型。模型包含 ID 与 `supports_tools`，没有 API Key 字段。Provider 操作状态/反馈与历史重命名及删除反馈分开，历史完成不会解除进行中的 Provider 请求。发现列表使用独立快照，关联 Provider UUID 和地址，不使用拼接文案传递模型数据。

`Conversation` 包含 UUID、标题、可选 BoundTarget、Provider UUID、模型、messages、activities、state、error 和更新时间。BoundTarget 仅保存连接 ID、公开名称/主机/端口/用户名与授权摘要，不保存密码、私钥或代理凭据。Message 保存角色、文本、完整 tool_calls 与 tool_call_id。

Activity 增加可选 `message_index` 与 `tool_call_id`，由 runner 创建活动时写入，关联原助手消息中的调用。两字段采用 serde 默认值，旧加密历史可直接读取，无需迁移密文格式。`src/app/ai/tool_steps.rs` 将消息与活动投影为文本/工具组，不改写模型上下文；`tool_steps/view.rs` 独立保存展开状态与审阅回调。结果中的失败/终态优先于旧活动状态，缺失或截断结果不能凭空推断成功。旧轮次未完成的步骤不会随新轮次变为运行中。

ToolGroupView 的根布局禁止纵向 flex 压缩，长答复由外层聊天滚动区承载。根节点同时裁剪圆角内的内容；若允许 flex 压缩，长答复会把工具组高度压至零，留下可见边框。回归使用真实 GPUI 滚动容器，覆盖运行 → 完成 → 追加长 Markdown 答复 → 展开结果。

`src/app/ai/composer.rs` 复用 InputState 及按钮，负责输入容器排布和临时反馈。TurnFeedback 在非活动轮为 Hidden；活动轮从最后一个 user 消息开始查找 assistant 的非空正文，没有正文时为 Waiting，已有正文时为 Dots。工具结果和历史旧轮不计入首次正文，后续模型请求不恢复 Waiting。AiPanel 将点附在最末助手回复容器中；末项为工具或异常时，使用无助手标识、无文案的独立点节点。只有 Waiting 使用两行助手标识/处理文案。反馈不写入 messages、不进入模型请求或持久化。三个点使用 GPUI 重复动画依次切换透明度，节点移除后自然停止，不新增后台定时器。输入使用 appearance(false) 去掉内部边框及禁用灰底，实际禁用语义和发送/停止事件保持原实现。

配置目录沿用 `~/.config/tiny-shell`：

| 位置 | 内容 |
| --- | --- |
| `ai/providers.json` | Provider 元数据；不含 Key。 |
| `ai/conversations/<UUID>.enc` | `TSAI1` 标记、24 字节随机 nonce、XChaCha20-Poly1305 密文；AAD 为对话 UUID，单文件读取上限 16 MiB。 |
| 系统凭据存储 | Provider Key 与随机 32 字节历史密钥；服务名包含配置目录规范路径的 SHA-256。 |
| `mcp/ai-access.json` | 内置访问开关和全局模式，schema_version=2；v1 一次性迁移，保留原开关并复制当时共享模式，后续独立；原 `mcp/policy.json` 格式不变。 |

Provider 与历史保存在独立目录，不进入 ConfigStore 的同步/归档对象。持久化串行锁隔离保存与删除，I/O 前复制快照并释放状态锁，避免删除后由迟到检查点重新创建历史。凭据不可用时 Key 可仅在内存中使用；无历史主密钥时不落盘聊天，不生成替代密钥覆盖已有密文。

## 轮次、取消与恢复

状态：Idle → Generating → RunningTool/AwaitingApproval → Generating → Idle；取消为 Interrupted，异常为 Failed。单对话取消通道在整个轮次完成后移除；面板隐藏不改变通道。显式停止、紧急停止和最后一个工作区窗口关闭请求取消。

重启只加载历史快照，活动轮标记 Interrupted，不启动 runner。继续对话时，对未配齐结果的 tool_calls 添加结果未知占位消息，保持模型协议完整，绝不重放操作。保存失败单独显示存储错误，不覆盖 Provider 操作状态。

网络连接超时 15 秒、请求总超时 300 秒、SSE 每次读取超时 60 秒。输出及普通 JSON 响应限制 1 MiB，单工具参数限制 256 KiB、单响应最多 32 个工具调用，每轮最多八次模型/工具交互。既有上下文选择最近最多 16 个完整用户轮，目标预算 256 KiB；单个较大轮保留，工具回送超过 64 KiB 时显式标记截断。历史正文不因上下文裁剪而删除。MCP 的文本、任务、会话及审批限制保持原契约。

JSON 回退与 SSE 共用完整性检查：禁止截断完成原因、重复 tool_call_id、非对象参数及不完整 JSON。只有用户主动测试、刷新或发送才调用 Provider。正式协议依据为 [Chat Completions](https://platform.openai.com/docs/api-reference/chat/create)，凭据适配为 [keyring 4.2.0](https://docs.rs/keyring/4.2.0/keyring/)。
