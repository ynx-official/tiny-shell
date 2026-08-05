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