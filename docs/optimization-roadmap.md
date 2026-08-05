# TinyShell 优化台账

> 建立日期：2026-08-05  
> 分析基线：`v1.3.2`（提交 `d53a815`）  
> 目的：持续记录可验证的结构优化，明确哪些已经完成、哪些仍待实施。

## 状态约定

- `[ ]` 待办：尚未完成，必须附带明确范围和验收条件。
- `[x]` 已完成：当前源码或 Git 历史中存在可核验的实现证据。
- 本文只记录工程优化，不混入尚未确认的产品功能。
- 状态发生变化时，应同时更新任务说明和证据，不能只修改复选框。

## 当前结论

近期版本已经开始拆分 `src/app/` 中的大型职责，并完成了后台事件合并、异步任务代际控制、连接表单聚合、SFTP 工作区状态聚合等工作。当前最明显的下一阶段问题，是这些子状态虽然已经形成类型，但仍集中挂载在 `TinyShell` 上，导致终端标签、窗格布局、主页导航、SFTP、同步、更新和窗口状态可以被大量模块直接修改。

优先级判断如下：

1. 先收敛终端工作区状态和状态转换入口。
2. 再减少 `Render::render` 中的状态修正与一次性 UI 副作用。
3. 最后处理字符串参数、国际化遗漏等局部问题。

## 下一项具体待办

### [x] APP-101 提取 `TerminalWorkspaceState`，收敛终端工作区状态转换

**优先级：P0**

**完成证据**

- `src/app/terminal_workspace.rs` 新增 `TerminalWorkspaceState`，聚合标签、标签组、系统信息页、窗格树和搜索状态。
- `TinyShell` 现在只持有一个 `WindowState`，工作区字段通过 `Deref` 提供兼容访问，避免继续复制状态。
- `src/app/ui.rs` 不再在 `Render::render` 中修正活动标签；工作区查询方法已委托给状态对象。
- 新增工作区与窗格删除语义测试，`cargo check --locked`、Clippy 和全量测试通过。

**目标职责**

新增 `src/app/terminal_workspace.rs`，由 `TerminalWorkspaceState` 负责：

- 保存终端标签、标签组、系统信息页和窗格树。
- 维护活动标签、活动组、活动系统信息页和聚焦窗格路径的一致性。
- 提供按 ID 查询、激活、插入、关闭、分割、移动标签所需的明确接口。
- 将不依赖 GPUI、后端 I/O 或窗口对象的状态转换实现为可单元测试的纯逻辑。

`TinyShell` 继续负责应用编排、GPUI 事件、窗口交互、后端启动和持久化，不应继续公开修改工作区内部集合。

**建议实施顺序**

1. 定义 `TerminalWorkspaceState`，先迁移上述字段和只读查询方法，不改变行为。
2. 将“活动标签必须存在”“活动组与窗格根同步”等约束收口为状态对象方法。
3. 迁移关闭标签、切换标签、窗格分割和跨窗口标签移动等状态转换；后端创建、SFTP 创建和 GPUI 通知仍留在应用编排层。
4. 删除迁移后对内部集合的直接写入，调用方只通过明确方法改变状态。
5. 为纯状态转换补充单元测试，覆盖空工作区、关闭活动标签、关闭最后一个窗格、切换标签组和非法 ID。

**验收条件**

- `TinyShell` 不再分别持有本任务列出的终端工作区字段，只持有一个 `TerminalWorkspaceState`。
- 活动标签修正不再发生在 `Render::render` 中，而是在引起状态变化的操作完成时立即维护。
- `TerminalWorkspaceState` 不依赖 `Window`、`Context<TinyShell>`、Tokio runtime、SFTP handle 或终端后端创建逻辑。
- 工作区集合的外部调用方不能直接执行 `push`、`retain`、索引赋值等修改操作。
- 新增的纯逻辑测试覆盖主要不变量，且格式检查、Clippy 和全部测试通过。

**非目标**

- 本任务不重写终端渲染器。
- 本任务不改变标签、窗格或跨窗口拖拽的用户行为。
- 本任务不同时重构 SFTP、同步、更新和设置状态，避免扩大改动面。

## 后续待办

### [x] APP-102 将渲染期副作用迁移到显式状态转换

`Render::render` 现在只负责派生 UI；SFTP 输入同步、树滚动同步和终端滚动偏移消费已移到 `on_prepaint` 的显式状态更新边界。一次性传输对话框使用 `pending_dialog: Option<DialogKind>` 表达并在 prepaint 消费，上传流程不再通过渲染期布尔状态触发对话框。

**验收证据**

- `src/app/ui.rs::render` 不再执行状态同步或消费滚动偏移。
- `src/sftp/ops.rs` 不再写入不存在的 `show_transfers_dialog` 字段。
- 重复渲染不会重复执行这些状态变更。

### [x] APP-103 用领域枚举替代窗格方向字符串

`PaneDirection::{Left, Right, Up, Down}` 已成为窗格分割和焦点移动的统一参数类型。UI action 与终端输入在边界处直接构造枚举，领域逻辑不再接收方向字符串。

**验收证据**

- `split_current_pane`、`focus_adjacent_pane` 及相关递归函数均使用 `PaneDirection`。
- 四个方向的 UI action、快捷键和嵌套窗格路径已完成编译验证。

### [x] APP-104 补齐用户可见输入提示的国际化

连接表单的可选名称、私钥路径、内联私钥和 SSH passphrase 提示已统一使用 `locales/zh-CN.yml` 与 `locales/en.yml`。示例端口、用户和路径仍作为输入数据默认值保留。

**验收证据**

- `src/app/mod.rs` 的连接输入 placeholder 均通过 `t!` 获取。
- 中英文资源已补齐对应键，Clippy 与全量测试通过。

### [x] APP-105 强化 `PaneLayout` 的不变量与返回语义

`PaneLayout::Empty` 已成为空窗格的显式表示。`remove_tab` 只在真正删除目标时返回 `true`，递归删除后会清理空节点并折叠单子节点。

**验收证据**

- `src/app/mod.rs` 覆盖空节点、单叶、组合节点和嵌套布局处理。
- `src/app/terminal_workspace.rs` 新增删除不存在标签、删除最后叶子、嵌套折叠及无空 ID 测试。
- 全量测试结果为 221 passed、0 failed、1 ignored。

## 新识别待办

### [ ] APP-106 移除工作区状态的可变透传，集中维护状态不变量

**优先级：P0**

**现状证据**

- `src/app/terminal_workspace.rs` 中 `TerminalWorkspaceState` 的集合、活动 ID 和 `pane_root` 字段仍全部为 `pub(crate)`。
- `WindowState` 对 `TerminalWorkspaceState` 实现 `DerefMut`，`TinyShell` 又对 `WindowState` 实现 `DerefMut`，调用方可以从应用对象直接修改工作区内部字段。
- `src/app/connection_actions.rs`、`src/app/session_actions.rs` 等模块仍直接执行 `tabs.push`、`tab_groups.push`、`pane_root = ...`、`active_tab = ...`，一次操作需要由调用方手工维护多个字段的一致性。

**风险**

新增标签关闭、跨窗口移动或窗格操作时，容易遗漏活动标签、活动组、窗格根和焦点路径中的任一同步步骤；状态对象虽然已提取，但尚未真正形成约束边界。

**建议范围**

- 移除 `WindowState` 和 `TinyShell` 面向工作区的 `DerefMut` 兼容层，保留显式只读访问或命名访问器。
- 为新增标签组、激活标签、关闭标签、替换窗格根、同步当前组布局等操作提供 `TerminalWorkspaceState` 方法。
- 方法返回状态变化结果或领域事件，由 `TinyShell` 在外层执行后端注册、持久化和 `cx.notify()`，不把 GPUI/I/O 引入工作区模块。

**验收条件**

- `src/app/` 中除 `terminal_workspace.rs` 外，不再直接修改 `tabs`、`tab_groups`、`pane_root`、`active_tab`、`active_group` 和 `focused_pane_path`。
- 关闭活动标签、删除最后一个标签、切换标签组、分割窗格和跨窗口移动均通过命名状态转换完成。
- 单元测试覆盖每项转换后的活动 ID、布局和焦点路径不变量。
- 格式检查、Clippy 和全部测试通过，用户可见行为不变。

### [ ] APP-107 统一对话框生命周期和串行切换入口

**优先级：P1**

**现状证据**

- `TinyShell` 同时持有 `active_dialog: Option<DialogKind>`，`WindowState` 另持有 `pending_dialog: Option<DialogKind>`，同一领域存在“已打开”和“待打开”两套状态源。
- `src/app/dialogs/quick_commands.rs`、`src/app/managed_keys.rs`、`src/app/sync_dialogs.rs`、`src/app/updater/ui.rs` 等模块分别手工设置或清除 `active_dialog`。
- 对话框切换需要调用方自行组合 `active_dialog = None`、`window.close_dialog(cx)` 和 `window.defer(...)`；连接管理器、托管密钥选择器等路径已有多处重复序列。

**风险**

关闭回调、延迟打开和异步结果交错时，状态可能先于窗口实际生命周期变化，导致重复打开、请求被静默丢弃，或 `active_dialog` 与 GPUI 当前对话框不一致。

**建议范围**

- 引入单一 `DialogCoordinator`/`DialogState`，明确 `Idle`、`Opening`、`Open`、`Switching` 等必要状态，或采用当前对话框加单槽待处理请求的等价模型。
- 集中提供 `request`、`opened`、`closed`、`replace` 接口，对话框模块只提交请求和内容构建函数。
- 明确并测试“已有对话框时拒绝、替换或排队”的策略，不引入无界对话框队列。

**验收条件**

- `TinyShell` 不再同时维护独立的 `active_dialog` 与 `pending_dialog` 字段。
- 业务模块不再直接拼接清状态、关闭窗口和 `defer` 打开下一个对话框的生命周期序列。
- 测试覆盖重复请求、关闭后切换、异步完成时原对话框已关闭，以及传输对话框的延迟打开。
- 任意时刻状态模型与实际打开的应用级对话框一致。

### [ ] APP-108 为后台事件管道补充拥塞策略和可观测性

**优先级：P1**

**现状证据**

- `src/terminal/mod.rs::backend_event_channel` 使用容量为 16,384 的 `sync_channel`，`BackendEventSender::send` 在队列满时会阻塞生产线程。
- `src/session/store.rs::drain_events_for` 每轮最多路由 2,048 个事件，每个 owner 或未路由 ID 最多缓存 8,192 个事件；达到上限后仅保存一个 `deferred_event` 并停止继续路由。
- `src/app/mod.rs::drain_backend_events` 每轮最多消费 2,048 个事件，事件只有取出后才进入 `coalesce_backend_events`；当前没有队列深度、延迟、阻塞次数或丢弃/合并数量的观测数据。

**风险**

大量终端输出或未及时注册的 route 会形成队头阻塞，连带延迟连接状态、关闭事件和其他窗口事件；现有上限控制了部分内存，却无法判断何时持续拥塞，也无法验证合并是否真正降低压力。

**建议范围**

- 为事件类别定义明确策略：终端输出保持顺序，状态/指标允许覆盖，控制事件不得被静默丢弃。
- 在不破坏顺序屏障的前提下评估将可替换事件提前合并，或为不同类别建立有界缓冲。
- 增加低成本统计：当前/峰值积压、每轮路由与消费量、合并量、生产者阻塞或发送失败次数；默认只记录异常阈值，避免高频日志。

**验收条件**

- 压力测试覆盖单窗口高频输出、多窗口竞争、route 注册前积压和队列达到容量的场景。
- 控制事件在持续输出压力下仍能在有界轮次内被处理；输出字节顺序保持不变。
- 所有队列均有明确容量和满载策略，代码中不存在无说明的静默丢弃。
- 可以通过测试断言或诊断统计确认事件发生过合并、积压或背压。

### [ ] APP-109 将配置持久化仓库改为可管理、可刷新和可测试的生命周期组件

**优先级：P1**

**现状证据**

- `src/app/config_persistence.rs` 使用全局 `OnceLock<Arc<ConfigRepository>>`，首次异步保存时创建后台线程和标准库 mpsc 通道。
- 工作线程没有显式关闭、join 或 flush 接口；`save_full_async` 返回成功只表示请求已入队，后台实际保存失败仅写入日志。
- 全局 repository 跨测试和窗口共享，现有测试主要验证 sequence 与配置合并，没有验证防抖保存、异步失败传播、退出前落盘或线程终止。

**风险**

应用退出或测试结束时，最后一批异步偏好可能仍在 100ms 防抖窗口内；调用方无法区分“已排队”和“已落盘”，全局线程也使故障注入及测试隔离困难。

**建议范围**

- 将 repository 作为应用级依赖注入到窗口/启动编排层，保存路径或存储实现通过窄接口注入。
- 为后台 worker 定义 `flush`、`shutdown` 和完成确认语义；明确正常退出、发送端断开和 worker 启动失败时的行为。
- 保留偏好合并与完整保存的先后规则，但把防抖批处理提取为可测试的纯决策逻辑。

**验收条件**

- 应用正常退出前可以等待已接受的保存请求完成，并有超时或失败结果。
- 异步完整保存能够向需要确认的调用方返回最终落盘结果，而不只返回入队结果。
- 测试不依赖全局单例，覆盖防抖合并、完整保存覆盖旧偏好、I/O 失败、flush 和 shutdown。
- 多窗口共享同一配置写入顺序，且不会为每个窗口创建独立保存线程。

### [ ] APP-110 清理剩余用户可见硬编码状态和系统文件选择提示

**优先级：P2**

**现状证据**

- `src/sftp/ops.rs` 仍包含 `Select File to Upload`、`Select Folder to Upload`、`upload picker failed` 和 `failed to save external editor` 等用户可见英文文本。
- `src/app/session_actions.rs` 使用 `pane split`、`new window opened` 状态文本。
- `src/app/terminal_settings.rs` 与 `src/app/theme.rs` 仍直接拼接终端字号、主题模式和主题不存在等英文状态。

**风险**

中文界面会混入英文提示；相同错误在不同模块使用不同格式，后续难以统一用户提示与技术日志的边界。

**建议范围**

- 将用户可见状态、错误前缀和系统文件选择提示迁移到 `locales/zh-CN.yml`、`locales/en.yml`。
- 原始错误详情作为插值参数保留；仅供开发排查的信息继续使用 `tracing`，不直接展示内部上下文。
- 示例 URL、端口、区域、权限值等输入数据不作为自然语言文案处理。

**验收条件**

- 上述文件中的用户可见自然语言均通过 `t!` 或统一的本地化状态构造函数生成。
- 中英文资源键同时存在，插值参数名称一致。
- 中文和英文 locale 下分别验证文件/文件夹选择、选择器失败、主题切换、字号调整和窗口/窗格操作提示。
- 保留的硬编码 placeholder 仅为协议、路径、数字或格式示例，并在代码审查中逐项确认。

## 已完成

### [x] APP-001 合并高频后台事件并保留顺序屏障

**证据**

- `src/app/backend_events.rs` 已实现 `coalesce_backend_events`。
- 终端输出按标签合并，传输进度、SFTP 延迟和远程系统指标只保留队列中的最新值。
- 模块内已有输出顺序屏障和传输进度合并测试。

### [x] APP-002 收敛异步任务生命周期与过期结果覆盖

**证据**

- `src/app/runtime_state.rs` 已提供 `TaskSupervisor`、`TaskCancellation` 和任务 generation。
- 更新下载、同步调度等状态可以识别过期任务并取消同名旧任务。
- `ac05b4f` 进一步限制后台事件路由并强化异步配置持久化边界。

### [x] APP-003 聚合连接表单和 SFTP 工作区状态

**证据**

- `src/app/mod.rs` 已存在 `ConnectionFormInputs`、`SftpWorkspaceState` 和 `SftpPanelState`。
- 提交 `4fe7283` 完成连接输入与 SFTP 面板状态分组。
- 提交 `df7dc04` 继续统一标签查询和 SFTP 工作区状态，减少分散字段与重复查找。

### [x] APP-004 用明确状态模型表达工作区展示模式

**证据**

- `src/app/workspace_presentation.rs` 使用 `WorkspaceMode` 和 `CleanSftpState` 表达普通模式、清洁模式和清洁模式下的 SFTP 展开状态。
- `WorkspaceMode::presentation` 集中派生侧边栏、SFTP 页脚和最小化状态。
- 模块内已有模式切换和普通模式状态恢复测试。

### [x] APP-005 抽离更新提示 UI

**证据**

- `src/app/updater/indicator.rs` 已成为独立更新提示组件。
- 提交 `458cc47` 将该组件从主页和侧边栏的重复展示逻辑中抽离。

### [x] DOC-001 建立可持续维护的优化台账

**证据**

- 本文已记录状态约定、首项具体任务、后续候选项、验收条件和已完成证据。

## 维护方式

实施任务时按以下顺序更新本文：

1. 开始实施前保留 `[ ]`，在任务标题下补充目标提交或分支信息。
2. 代码完成但质量门禁未通过时仍保持 `[ ]`，记录未通过项。
3. 验收条件全部满足后改为 `[x]`，补充提交哈希、测试结果和关键文件。
4. 如果任务范围发生变化，先修改职责和非目标，再继续编码，避免完成状态与实际实现不一致。

## 本轮验证范围

- `cargo fmt --all`：通过。
- `cargo check --locked`：通过。
- `cargo clippy --locked --all-targets -- -D warnings`：通过。
- `cargo test --locked --all-targets`：通过，221 passed、0 failed、1 ignored。
- `git diff --check`：通过（仅有 Git 的换行符提示，无差异错误）。