# 内置 AI 实现契约

> 状态：Review
> 最后更新：2026-10-22
> 关联：[需求](../01-requirements/ai-conversations.md)、[交互](../02-design/ai-conversations.md)、[MCP 契约](mcp-contracts.md)、[安全边界](ai-threat-model.md)、[验证](../04-quality/ai-conversations.md)

## 职责与请求路径

- `src/ai/model.rs`：Provider、模型能力、绑定目标、消息及轮次状态。
- `src/ai/model/defaults.rs`：唯一 `ModelSelection`、厂商与默认组合校验、删除默认厂商保护；保留既有序列化结构。`Conversation::new` 只从有效组合继承默认，已有会话不重新绑定。
- `src/ai/provider.rs`：OpenAI 兼容 HTTP/SSE、模型发现及固定消息测试。不开启重定向，不发送内部凭据。
- `src/ai/storage.rs` 与 `key_vault.rs`：隐私密码保护的 API Key、系统凭据保护的历史主密钥、本机认证加密、受限读取、原子保存和临时模式。配置同步、旧凭据迁移与身份绑定见[同步契约](../sync/ai-mcp-sync.md)。
- `src/ai/service.rs` 与 `service/runner.rs`：后台运行、取消、快照、消息上下文与工具闭环。`service/toolset.rs` 定义内置工具边界，`service/tools.rs` 管理会话和任务等待，`service/budget.rs` 负责预算与结果复用。UI 线程不等待网络或凭据存储。
- `src/app/ai/`、`src/app/settings/ai.rs`：原生右栏、历史和 Provider 草稿；UI 只读取快照、发出操作。
- `src/app/settings/ai/providers.rs` 与 `providers/`：厂商编辑、全局选择草稿、联动选择器和分栏渲染。`Service::save_provider` 接收一个默认组合，并在持久化锁内复核最新设置后将厂商与默认配置一同写入。2026-10-22 已确认的规则与兼容边界见[厂商设置设计](../02-design/ai-provider-settings.md)，未运行验证见[质量记录](../04-quality/ai-provider-settings.md)。
- `crates/mcp/src/engine/internal.rs`：不可通过 IPC 构造的内部能力；共享引擎继续负责 SSH、文件、审批、取消和审计。

新对话从当前终端的连接 ID 查找已保存的 SSH；主机、用户、端口及代理身份必须一致，绑定保存态的授权摘要，不把终端展开的托管私钥或仅本次登录的口令计入新的身份。主机/代理已修改、临时或 RDP 连接不隐式绑定。旧版已记录错误摘要的对话需新建，不能静默改绑历史目标。发送时固定 Provider/模型和目标快照。具备工具能力且内置开关已开、绑定目标与当前已保存连接的摘要一致时才取得 `InternalClient`、附带目标信息和工具 Schema。模型调用 → 累积完整参数 → 内置工具参数校验 → 应用建立/复用会话并填入 ID → 原有 Engine → 通知等待任务终态 → 工具结果回送模型。`request_id` 由应用生成 UUID，防止模型复用请求造成意外幂等匹配。

内部来源是 `ai:<conversation UUID>`，外部 EngineHandler 拒绝此命名空间。任务和 SSH 会话沿用 client 字段隔离；对话结束或取消后关闭自身独立 SSH 会话，人工终端不受此生命周期控制。AI 全局模式/启停代数绑定内部能力，关闭后再打开不恢复旧轮能力。Engine 按来源生成有效 Policy：AI 只有当前能力绑定的目标及 AI 模式，外部沿用原机器授权及模式。发放能力前以及执行前核对当前保存目标摘要。两种来源的权限变更仅取消自身任务、审批和会话；审计记录任务创建时的模式。

## 数据与存储

2026-10-13 对话权限覆盖：`Conversation.mode_override: Option<Mode>` 为 None 时跟随全局，旧历史缺少字段时也为 None。`service/permissions.rs` 在状态锁内同时检查活动状态及 runner 取消通道，拒绝运行中修改；成功后只保存当前对话，并记录本机权限变更审计。已有消息的对话复用加密 checkpoint，空白草稿仍不持久化，不进入全局设置或云同步。

runner 将对话快照中的覆盖传给 `checked_internal_client_with_mode`。引擎 Binding 保存目标、可选模式和独立能力代数，`InternalClient::mode` 与有效 Policy 使用同一解析规则；模型工具声明、命令/写入审批及任务审计因此一致。每次发放新轮次能力分配新代数，仅替换本对话 Binding；前一轮有未终结任务或 SSH 会话时拒绝重发，旧句柄不能获得新权限或停止新轮次。全局变更仍统一撤销旧能力并取消内置任务，外部 MCP 不受影响。总开关、目标摘要和模型工具能力均不会被覆盖绕过。

`Provider` 包含 UUID、名称、Base URL、启用状态、本机 HTTP/无 Key 选项、模型数组和默认模型。模型包含 ID 与 `supports_tools`，没有 API Key 字段。Provider 操作状态/反馈与历史重命名及删除反馈分开，历史完成不会解除进行中的 Provider 请求。发现列表使用独立快照，关联 Provider UUID 和地址，不使用拼接文案传递模型数据。

`Conversation` 包含 UUID、标题、可选 BoundTarget、Provider UUID、模型、messages、activities、state、error 和更新时间。BoundTarget 仅保存连接 ID、公开名称/主机/端口/用户名与授权摘要，不保存密码、私钥或代理凭据。Message 保存角色、文本、完整 tool_calls 与 tool_call_id。

Activity 增加可选 `message_index` 与 `tool_call_id`，由 runner 创建活动时写入，关联原助手消息中的调用。两字段采用 serde 默认值，旧加密历史可直接读取，无需迁移密文格式。`src/app/ai/tool_steps.rs` 将消息与活动投影为文本/工具组，不改写模型上下文；`tool_steps/view.rs` 独立保存展开状态与审阅回调。结果中的失败/终态优先于旧活动状态，缺失或截断结果不能凭空推断成功。旧轮次未完成的步骤不会随新轮次变为运行中。

2026-10-13 聊天内审阅：`tool_steps/review.rs` 展示引擎准备的单任务快照并管理提交反馈；`src/app/ai/review.rs` 连接引擎及既有凭据编辑器。`Engine::review(id)` 只复制指定任务，避免渲染时复制其他对话的文件预览。加载与点击时均核对任务 ID、`ai:<conversation>` 来源、目标 ID/摘要及 AwaitingUser 状态；提交前核对展示快照与当前命令、预览、错误状态一致。文件预览缺失和 AUTH_REQUIRED 不允许走普通批准。按钮成功后停用；最终审批仍由 Engine::approve 原子检查待处理状态、权限代数和时限，继续使用既有审计及执行前目标/文件版本校验。预览不使用模型生成的参数摘要，也不增加 Provider 请求或持久化格式。

ToolGroupView 的根布局禁止纵向 flex 压缩，长答复由外层聊天滚动区承载。根节点同时裁剪圆角内的内容；若允许 flex 压缩，长答复会把工具组高度压至零，留下可见边框。回归使用真实 GPUI 滚动容器，覆盖运行 → 完成 → 追加长 Markdown 答复 → 展开结果。

`src/app/ai/composer.rs` 复用 InputState 及按钮，负责输入容器排布和临时反馈。TurnFeedback 在非活动轮为 Hidden；活动轮从最后一个 user 消息开始查找 assistant 的非空正文，没有正文时为 Waiting，已有正文时为 Dots。工具结果和历史旧轮不计入首次正文，后续模型请求不恢复 Waiting。AiPanel 将点附在最末助手回复容器中；末项为工具或异常时，使用无助手标识、无文案的独立点节点。只有 Waiting 使用两行助手标识/处理文案。反馈不写入 messages、不进入模型请求或持久化。三个点使用 GPUI 重复动画依次切换透明度，节点移除后自然停止，不新增后台定时器。输入使用 appearance(false) 去掉内部边框及禁用灰底，实际禁用语义和发送/停止事件保持原实现。

`src/app/ai/scroll.rs` 统一聊天滚动策略。2026-10-16 因长记录卡顿，将全量布局与 ScrollHandle 跟随改为 GPUI ListState/FollowMode::Tail，具体渲染契约见下节。活动轮（生成、工具执行、等待审批及预算暂停）刷新时恢复底部跟随；非活动轮在底部时随迟到排版保持锚定，向上滚动立即解除跟随，回到底部重新跟随。切换对话与发送重新锚定尾部。面板标题、用量与输入容器保留自身高度，只有 transcript 承载聊天滚动。

### 长记录渲染（2026-10-16）

- `service/panel_snapshot.rs` 提供当前对话专用快照，保留设置、存储提示等公共状态，只复制所选对话。历史列表继续使用完整快照以保留全文搜索。AiPanel 以服务 stamp、当前会话及历史/聊天模式缓存 `Rc<Snapshot>`，输入、布局与动画刷新不重新复制历史；真实服务数据变化才刷新。现阶段一次服务变更仍会复制当前对话，未改持久化模型。
- `panel/transcript.rs` 保存 ListState、消息/工具组投影和渲染缓存。列表按可变高度测量可见范围与 200px 邻域，不启用 measure_all；正文、工具组和尾部提示分别作为列表项。快照未变化时复用投影，变化时找出受影响后缀并重新测量；条目数量变化通过 splice 更新，保留未受影响的历史阅读锚点。宽度变化由 GPUI 重新测量，异步正文排版由 Tail 模式保持尾部。
- 安全处理后的 Markdown 以消息位置和完整原文匹配，等长内容替换也失效；使用 SharedString 避免每帧复制按钮与 TextView 文本。LRU 最多保存 64 项且原文与处理文本合计不超过 4 MiB，超限只淘汰缓存，不截断显示、复制或历史。图片/HTML 的资源阻断仍调用原 `markdown::renderable`。
- 已访问工具组的实体由当前 Transcript 持有，切换对话释放；仅可见组更新和渲染，避免滚出视口即丢失展开状态。续聊建议按会话及轮次保留一个实体，其点击仍经过原服务校验。审批作用域、过期校验及执行边界保持原实现。
- 工具投影预先按消息位置、调用 ID 与操作名建立活动索引，旧活动按操作名和原顺序消费，保留精确关联优先的规则；最后一个用户轮只定位一次，消除每个工具调用重复扫描活动和后续消息的开销。

这项优化针对多消息历史；单条超长正文及单个包含大量步骤的工具组仍作为一个列表项布局，未拆分正文块。尚无运行时帧率、耗时或内存基准，不据此声明性能提升比例；验证与手动验收见[质量记录](../04-quality/ai-conversations.md)。

配置目录沿用 `~/.config/tiny-shell`：

| 位置 | 内容 |
| --- | --- |
| `ai/providers.json` | Provider 元数据；不含 Key。 |
| `ai/conversations/<UUID>.enc` | `TSAI1` 标记、24 字节随机 nonce、XChaCha20-Poly1305 密文；AAD 为对话 UUID，单文件读取上限 16 MiB。 |
| `ai/keys.enc` | 云同步隐私密码加密的服务商 Key，绑定服务商 ID 与地址。 |
| 系统凭据存储 | 随机 32 字节历史密钥；服务名包含配置目录规范路径的 SHA-256。旧 Provider Key 的迁移见同步契约。 |
| `mcp/ai-access.json` | 内置访问开关和全局模式，schema_version=2；v1 一次性迁移，保留原开关并复制当时共享模式，后续独立；原 `mcp/policy.json` 格式不变。 |

Provider 与历史保存在独立目录，连接归档不包含它们；AI/MCP 配置由应用协调进入云同步，规则以[同步契约](../sync/ai-mcp-sync.md)为准。持久化串行锁隔离保存与删除，I/O 前复制快照并释放状态锁，避免删除后由迟到检查点重新创建历史。未配置隐私密码时新 Key 仅在内存中使用；无历史主密钥时不落盘聊天，不生成替代密钥覆盖已有密文。

## 首轮后台命名（2026-10-13）

`service/title.rs` 在新会话首轮成功结束时启动独立任务，冻结该轮 Provider、模型及轮 ID。只发送首条用户任务的前 4000 个字符；不附加 SSH 目标、工具输出或聊天历史，不提供 tools。请求为非流式，输出预算 128 tokens、整体超时 30 秒；结果规范化为不超过 20 个 Unicode 字符的单行标题，原始响应仍按既有脱敏规则留审计。

`TitleState` 区分 AwaitingFirstTurn、Generating、Generated、Failed 和 Preserved；旧数据缺字段默认 Preserved，不补发请求。标题任务使用独立取消通道，不占聊天活动状态。手动改名同步改为 Preserved 并取消请求；删除和 stop_all 同样取消。结果只写仍为 Generating 的存活会话，检查点与删除沿用同一持久化锁，不能复活已删文件。重启时，即使聊天已 Idle，也将未完成命名标为失败、请求标为 interrupted，保留已有用量且不重试。

模型请求新增带兼容默认值的 `RequestPurpose`（chat/title/followup/provider_test）。命名用量归属原会话和首轮，独立于 SSH 执行预算；工具审计只选择当前轮的 chat 请求，防止晚到辅助结果串入下一轮。请求用途保存在既有密文正文，无数据库迁移。

## 轮后续聊建议（2026-10-13）

`service/followups.rs` 在 runner 正常结束时取得已完成轮的最新消息快照，并冻结该轮 Provider、模型和轮 ID。只发送本轮用户请求前 4000 个 Unicode 字符及最终助手正文前 6000 个字符，不另加 SSH 元数据、工具原文或其他轮历史。Provider 辅助请求无 tools、非流式，输出上限 512 tokens、整体超时 20 秒，失败不自动重试。解析只接受 JSON 字符串数组，最多 3 项，每项最多 120 个 Unicode 字符；空文本、多行、控制及双向显示字符、对象形式的隐藏 prompt 均拒绝，重复项去重，空数组合法。

`Conversation.followups` 为兼容默认 None 的轮次元数据，包含 turn_id、request_id、Generating/Ready/Failed 及完整候选文本，不写入 messages，也不进入后续模型聊天上下文。随本机加密历史持久化，旧历史不补生成；重启将未完成生成标为 Failed，并按既有逻辑结算中断请求。`execution.followup_suggestions` 默认 true，随既有配置同步。关闭并保存或同步关闭时取消生成，UI 隐藏已存候选；重新开启只影响后续成功轮，不发历史请求。

每个后台请求使用独立取消通道，开始新轮、停止、删除、stop_all 均取消；结束写回时在同一状态锁内核对会话、轮次、请求 ID 和开关，旧任务不能清除新任务的取消通道。持久化与删除复用既有串行锁，已删除会话不能由迟到结果重建。候选 Token 使用 `RequestPurpose::Followup` 计入原会话和原轮，辅助请求独立于主执行预算，不增加工具权限。

`app/ai/followups.rs` 是独立轻量 GPUI 组件；`panel/followups.rs` 将点击转给服务。添加文本先校验候选仍属于当前会话和轮次；直接发送在 `runner::start_turn` 同一锁内验证候选并开始下一轮，复用原 Provider/权限入口、活动轮互斥和审计。仅添加操作编辑 InputState；直接发送不清空原草稿。输入框服务器与候选操作 SVG 通过 `app/assets.rs` 静态嵌入，不需要网络资源。

2026-10-13 用户确认“两条直显”整理：`FollowupView` 的展开更多只控制当前轮可见项，默认两条，不修改候选存储、发送文本或请求次数；整组折叠独立保存当前视图选择。Provider 提示按有用程度排序，优先单一动作的完整短请求，保留必要标识，硬校验仍为 120 字符。`app/ai/usage.rs` 仅负责输入框里的 k/M/B 展示及不完整标记，精确统计和点击审计路径复用原实现。`composer::context` 接收目标与用量元素，权限状态集中于底部 selector，并保留不可用原因和原有授权逻辑；无数据迁移。

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

2026-10-12 设置导航修订：`src/app/settings/ai.rs` 使用三个具名 SettingGroup，复用组件库与快捷键一致的侧栏子菜单/分组定位。Provider、BudgetForm、NotificationForm 使用独立 keyed state，分别位于“模型服务 / 执行预算 / 通知提醒”。预算与通知保存前读取最新 ExecutionSettings，仅覆盖本组字段，然后调用原 save_execution；共享 busy 与持久化串行锁避免并发保存，不新增配置字段。SettingPage 在设置顶层仍为索引 4，MCP 为索引 5，既有对话跳转入口不变。

`Budget` 区分可扩展的调用 ceiling 与不可绕过的 safety_halted。继续不清空缓存、累计调用数、相同结果计数或 uncertain 指纹。runner 在下一次模型请求或批内下一次工具执行之前检查预算，保存带随机 token 的 Continuation，并通过独立 oneshot 等待；与取消 watch、内部能力撤销一起 select。Service 在锁内核对 token 与取消状态，只接受一次选择。未选择不发请求、不执行工具；恢复使用同一消息序列、待执行批次及已打开的独立 SSH 会话。批内暂停终止时为剩余调用保存 not_executed 配对结果，不向 Provider 请求总结。重启加载活动状态时清空 continuation 并标记中断，不恢复 runner。

`src/app/ai/attention.rs` 负责所属窗口、去重、已有弹窗避让、过期弹窗移除和通知正文点击导航。只读取轻量 RunAttention，避免每次主窗口渲染复制全部历史。弹窗使用既有 modal token 及 ConfirmationDialog；窗口/实体更新由 defer 调度，避免 GPUI 重入。2026-10-12 按用户要求增加原生通知允许/拒绝按钮，通知按钮由平台回调直接调用线程安全的 Service::resume/terminate_continuation，不进入正文导航队列；Service 锁内校验对话/token/取消状态，允许或拒绝只接受一次。旧通知及未知 action 不追加预算、不取消其他轮次，正文导航仍需 live 校验。

`src/app/ai/notifications.rs` 隔离 Windows/macOS 平台适配与异步回调。Windows 通过 `tauri-winrt-notification 0.8.1` 使用 WinRT Toast，本用户注册 TinyShell.Desktop 身份，安装脚本同步快捷方式 AUMID 和卸载清理；不借用 PowerShell 等应用身份。macOS 通过 `mac-usernotifications 0.3.1` 使用 UNUserNotificationCenter，首次主动开启/测试请求授权，后台发送只查询授权状态；直接执行非 .app 二进制时反馈包要求。系统调用运行在后台，不阻塞 UI；macOS 通知响应最长观察一小时，超时或清除不会恢复轮次，应用内暂停无超时。Linux 不引入通知依赖，保留应用内选择。

平台适配结果、未运行项目及重点手动验收见[预算与通知验证](../04-quality/ai-execution-budget.md)。

Windows `add_button` 的 arguments 与 macOS Action::button 的 identifier 使用固定动作 ID；解析只接受两个已登记 ID，通知正文映射 Open，其他数据忽略。macOS 通过 response.is_default_action 区分正文与按钮，close_reason 非空时不执行动作；继续按钮设置 authenticationRequired。按钮处理成功后沿用 Service stamp 更新 UI，清除失效弹窗，不显式拉回窗口再做二次选择。测试通知没有对话身份，按钮无业务副作用。无需增加依赖或改变配置/历史格式。

JSON 回退与 SSE 共用完整性检查：禁止截断完成原因、重复 tool_call_id、非对象参数及不完整 JSON。用户主动测试、刷新或发送会调用 Provider；新会话首轮成功后额外异步请求一次标题，开启续聊建议时每轮成功结束后额外请求一次候选。打开历史或重启不补发命名或候选请求。正式协议依据为 [Chat Completions](https://platform.openai.com/docs/api-reference/chat/create)，凭据适配为 [keyring 4.2.0](https://docs.rs/keyring/4.2.0/keyring/)。
