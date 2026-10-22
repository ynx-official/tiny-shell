# MCP 开发契约

> 状态：Approved  
> 最后更新：2026-10-22
> 关联：[产品设计](../02-design/mcp-integration.md)、[实施与验收](../04-quality/mcp-implementation.md)

## 边界与构建

新增 `crates/mcp` workspace 包，独立编译 `tiny-shell-mcp`，提供公开 DTO、Schema、权限、任务、注册存储和 IPC；依赖 rmcp 3.4.0 server + transport-io，不链接 GPUI。主应用依赖共享包，`src/app/mcp/` 实现运行协调，`src/sftp/` 复用认证和文本保存。根应用 `default-run = tiny-shell`，现有 `cargo run` 不改变。

桥接命令：`tiny-shell-mcp --client <注册 UUID>`；`--config-dir <绝对目录>` 供便携配置与隔离测试使用。默认配置目录沿用 `~/.config/tiny-shell`。未知参数、非 UUID 客户端、缺失注册直接退出，stdout 不打印日志。桥接对 MCP 初始化与工具发现使用 SDK，工具调用通过认证 IPC 转发；应用离线返回结构化 APP_NOT_RUNNING，不自动启动应用。

2026-10-12 修正源码运行入口：`cargo run` 仅构建根应用，不保证生成独立 helper。根应用 `tiny-shell --mcp --client <注册 UUID>` 在 GUI 和日志初始化前直接运行共享 stdio 入口，无需同目录 helper；参数解析、注册撤销检查、SDK 与认证 IPC 均与独立程序复用。此修正不改变已批准的工具、权限或存储契约，验收增加缺少 helper 的配置导出和真实子进程握手，见[回归验证](../04-quality/mcp-cargo-run-bridge.md)。

### 桥接进程生命周期

2026-10-22 用户要求修复关闭 MCP 后桥接进程仍驻留的问题，并明确暂不编译。原实现只在引擎中禁用访问、取消任务与关闭会话，stdio 入口只等待客户端断开。本次在两个入口共享的 `crates/mcp/src/stdio.rs` 中补齐生命周期；权限模式、注册格式与工具接口保持原有契约。

- 在创建运行时及 stdio 传输前，读取所属配置目录的外部 MCP 总开关。开关关闭或策略文件不存在时正常退出，stdout/stderr 保持空白；未知参数、无效/撤销的注册与不可读配置仍拒绝启动。
- 从初始化握手开始，每 500 ms 复核该配置的总开关与该客户端注册。检测到关闭或撤销后停止桥接；配置损坏或注册不可读时以失败状态退出。检查不依赖新工具调用或客户端主动关闭 stdin。
- 运行中的 MCP 传输收到取消后最多等待 250 ms 清理；运行时再最多等待 250 ms，随后二进制入口结束进程。Tokio stdin 的阻塞读取不能取消，不能通过普通运行时析构无限等待。桥接使用单线程调度运行时，阻塞 I/O 仍由 Tokio 的阻塞线程处理。
- 每个实例自行观察所属配置，不按进程名批量结束进程。关闭一个配置不影响其他配置的桥接；撤销一个注册不影响同配置其他有效注册；内置 AI 的独立权限、主应用 IPC 与审计服务保持原有生命周期。
- 外部客户端控制进程创建，TinyShell 无法禁止第三方发起重试；禁用期间的新进程立即返回，不建立 MCP 服务、不后台驻留。重新启用后由客户端重新连接，不自动复活旧桥接。MCP 已启用而主应用离线时，仍保留握手及结构化 `APP_NOT_RUNNING` 行为。

本次没有改变配置 schema，无数据迁移。旧二进制和已运行的旧进程不会因源码修改自动获得新行为。回归代码、检查记录和手动验收见 [MCP 桥接进程退出验证](../04-quality/mcp-bridge-lifecycle.md)；当前实现尚未编译或运行验证。

## Schema 与结果

所有输入 object 禁止未知字段。ID 为 UUID；绝对路径拒绝 NUL；POSIX `/…` 与 Windows SFTP `/C:/…` 由文件服务处理。request_id 为调用方生成的 UUID，必须在 mutating / session 工具提交时提供。可选值不得混同空字符串。

| 工具 | 输入字段，括号内为可选及默认 | 成功 data |
| --- | --- | --- |
| get_status | 无 | app_version、generation、mode、limits |
| list_connections | search（空）、offset（0）、limit（50，1–100） | connections、next_offset；每条仅 id/name/group/host/port/user |
| open_session | connection_id、request_id | task_id；终态 result.session_id |
| close_session | session_id、request_id | task_id；终态 result.closed |
| run_command | session_id、command、request_id、cwd（省略使用登录目录）、timeout_ms（30000，1000–300000） | task_id |
| get_task | task_id、offset（0）、limit（65536，4–65536）、wait_ms（0，0–10000） | id/state/error/result、output（带 stream 的 UTF-8 文本片段）、next_offset/truncated |
| cancel_task | task_id | state、cancellation_requested |
| list_directory | session_id、path、offset（0）、limit（50，1–100） | entries、next_offset；entry name/type/size/modified |
| read_file | session_id、path | content/encoding/line_ending/version/size；完整文本，没有截断读 |
| write_file | session_id、path、content、mode（create/update）、expected_version（update 必须）、request_id | task_id；终态 result.version/size |

工具名称为上表原名。完整工具 Schema 由 Rust 类型导出至 [mcp-tools.schema.json](mcp-tools.schema.json)，更新方式为 `cargo run -p tiny-shell-mcp --example export-schemas`；实现类型是生成来源，不手改生成文件。输出页最少四字节，确保多字节 UTF-8 字符可完整前进；原设计的一字节下限据此调整。结构化结果统一为 `{ok:true,data:{…}}` 或 `{ok:false,error:{code,message}}`。相同 JSON 放入 MCP text content，structuredContent 同步设置。输出 Schema 描述统一 envelope，具体 data 结构由公开 DTO 定义。任务提交成功只说明请求已接收，必须 get_task 查询终态。命令退出码非零为 failed / COMMAND_FAILED，仍可取回 stdout/stderr/exit_code。

2026-10-12 用户确认实施工具描述、状态等待、会话管理和重复调用控制四项优化。外部仍暴露原十个工具，新增的 wait_ms 是向后兼容的可选参数：省略或为零立即查询，非零时等待该任务状态变化、进入终态或达到等待上限。完成、审批、取消和权限撤销通过引擎通知唤醒；订阅在初次快照之前建立，防止完成通知丢失。每次返回重新核对客户端、内部能力代数及任务归属；任务查询无需重新加载远端连接配置，实际执行仍验证目标摘要。十秒等待处于现有十五秒 IPC 调用期限内。更新后的描述引导复用连接/会话并使用等待查询。

本轮内置 AI 的四工具裁剪、应用管理会话、单轮预算及短时读缓存只用于进程内 AI 协调器；外部调用方继续使用现有 request_id 幂等、资源上限及独立会话。详见[AI 实现契约](ai-conversations.md)和[优化验证](../04-quality/ai-tool-call-optimization.md)。因项目新增“编译类命令由用户明确发起”的约定，静态 Schema 导出文件尚待用户运行上述生成命令同步 wait_ms 和新描述；运行时工具 Schema 直接由当前 Rust 类型生成。

JSON-RPC 缺失方法/畸形结构由 SDK 处理；工具参数解析错误返回 INVALID_ARGUMENT，且不能触发远端 I/O。错误只包含固定安全说明；不返回底层带凭据、密钥路径或本机配置的原始异常。

固定上限：命令 64 KiB、文件原始及编码后均 256 KiB、任务输出总计 1 MiB、IPC 消息 1 MiB、每客户端活跃执行 2、每连接变更 1、每客户端会话 8、全局在途任务 128、每客户端保留任务 256。目录最多读取 10000 项，再拒绝 DIRECTORY_LIMIT；分页目录不是服务器快照，变化可能造成分页漂移。

## 策略与本机存储

`mcp/policy.json` schema_version=1，包含 enabled=false、mode=read_only、scope=specified、grants（connection_id → target_digest）、known_targets、revision。凭据不进入此文件。target_digest 是主机/端口/用户名/认证引用/密钥内容摘要/代理配置摘要的 SHA-256，不包含名称、分组、last_used；只保存摘要。修改目标令已有授权失效。all scope 首次选择时明确确认“包括以后新增”；首次遇到的新 ID 纳入 known_targets，已有 ID 摘要变化永远不自动覆盖。指定授权经用户勾选才更新摘要。

`mcp/clients/<UUID>.json` 包含 schema_version、id、label、enabled 和 32 字节随机秘密；注册撤销后秘密失效。目录 Unix 0700、文件 0600；Windows 用受保护 DACL 只允许当前登录用户。拒绝符号链接目录/注册文件；不把秘密放到客户端配置、日志或公开 DTO。客户端标签不作为身份验证。

2026-10-12 为用户要求的客户端重命名增加可选 display_name，schema_version 保持 1。label 保留原注册模板，用于选择配置格式；Registration::name 优先返回 display_name，否则返回 label，已有注册无需迁移。Store::rename_client 校验单行名称并仅更新显示元数据，重复保存同名不写文件；改回模板名称时不序列化 display_name。Store::register_or_reuse 仅复用同类型、启用且仍为默认名称的注册，改名实例允许与新注册并存。共享 Store 克隆的元数据更新、默认注册复用和撤销使用同一互斥锁，防止重命名使用旧 enabled 状态覆盖撤销。原子保存和文件权限规则保持，IPC 与工具契约没有变化。旧版严格反序列化器不保证读取新增字段；需要降级时先改回模板名称以移除别名字段。

策略、注册和日志都不参与连接同步、导入导出。写入采用同目录临时文件 + 刷新 + 原子替换；加载损坏配置 fail closed，设置显示错误，不能自动恢复完全访问。应用内只保留一份进程级协调器。政策 revision 变化使所有未执行审批失效；撤销和关闭优先阻止新请求并取消相关活动任务。

元数据记录 `mcp/audit.jsonl`：timestamp/client_id/connection_id/operation/mode/state/exit_code；不存正文、命令、输出、凭据。按七天和 10 MiB 截短；在设置中清空。任务正文内存三十分钟，清理后 TASK_NOT_FOUND，不自动重放。request_id 去重至少同保留期；同客户端 ID + 相同规范化参数返回同任务，不同参数 REQUEST_ID_CONFLICT。

## IPC 与认证

协议版本 1；帧为 4 字节 big-endian 长度 + UTF-8 JSON，限制 1 MiB；超过限额立即关闭，不先分配攻击者声明的空间。Windows 当前用户 DACL 的命名管道禁用远程连接；Unix 私有目录 socket 权限 0600。Windows 管道名从配置目录规范化绝对路径 SHA-256 得出，Unix 路径固定 mcp/bridge.sock。

服务器对新连接发送 `{version:1,generation,nonce}`，nonce 32 随机字节；桥接返回 `{client_id,proof}`，proof=HMAC-SHA256(secret, version + generation + nonce + client_id 的确定性序列化)。服务端常量时间验证，nonce 每连接只用一次，超时 5 秒；成功后才接收 `{generation,tool,arguments}`。generation 不匹配拒绝 INSTANCE_CHANGED。服务端每次调用重新核对注册启用和策略，撤销既有连接也立即生效。一个 IPC 连接只处理一次工具调用，结果后关闭，降低退出和重连复杂度；整体调用超时 15 秒（远端长操作返回任务票据）。最大 16 个认证连接，拒绝无限排队。

多进程通过端点的排他创建检测冲突；Unix 不盲目删除既有 socket，只有确认无监听且为当前用户普通 socket 才回收；Windows first_pipe_instance。桥接不依据 PID 猜测其他实例。退出取消监听，关闭 AI 会话；突然退出可能留下远端进程，不能承诺远端已停止。

## 会话、审批和任务

每个客户端独立 session_id，绑定 connection_id + target_digest + generation。open 使用已有受控凭据解析和 HostKeyVerifier，无 PTY、无系统/Docker探测。缺少密码/口令进入 awaiting_user；UI 复用认证流程，认证信息只保存在所请求的内存会话，不写入 MCP 存储。连接握手最多 60 秒；空闲 10 分钟释放。

任务状态：queued → awaiting_user（受控变更或凭据输入）→ queued（批准）→ running → succeeded/failed/outcome_unknown；queued/awaiting_user 可 cancelled，审批超时 failed/APPROVAL_EXPIRED；running 取消先 cancel_requested，尽力关闭 channel 后 outcome_unknown；执行超时 timed_out（明确结果不确定），应用断开 interrupted。终态不可回到 running。审批截止时间 120 秒与执行时间分别计时。批准绑定完整参数、连接摘要和策略 revision；排队后及远端调用前再次校验。受控写入先获取旧内容/版本形成不可变审阅快照，再批准；前后不一致返回冲突，不重新提交。

UI 在 MCP 设置页显示待审阅队列及任务状态；审阅内容为远端数据，完整展示命令/cwd/账号和文件旧新文本，长文本可滚动。执行/拒绝使用项目 ConfirmationDialog；关窗不视为批准；同一任务最多确认一次。多窗口对同一仓库操作，批准采用锁下状态转换，防止双击重复执行。设置关闭/撤销提供状态反馈并刷新其他窗口。

不解析或过滤任意 Shell 命令来判定“安全”。原始 command 直接发送远端默认 shell；当前连接配置未记录远程 Shell 类型，因此首版指定 cwd 返回 UNSUPPORTED_SHELL；调用方可把明确的目录切换写进完整 command，由用户在受控模式审阅。后续增加经用户确认的 Shell 类型后再提供独立 cwd 包装。不得把 Windows 目录拼进 POSIX 命令。非交互 stdin/PTY 均不提供。

## 文件与取消

读文件先 lstat 类型/大小，最多限额+1 实际读取，避免属性被竞态绕过；返回 SHA-256（原始字节 + 权限/修改时间版本信息）。update 必须完整版本相等，复用既有编码/换行和文本保存校验；create 排他创建，目标存在不覆盖。写入拒绝最终及父路径符号链接，应用内每连接变更串行；跨远端进程的读取/替换并非全局 CAS。替换进入恢复/校验阶段后不直接 abort，先完成一致性流程，再报告取消结果不明。

## 客户端与平台入口

配置示例采用 MCP 通用 mcpServers：同目录存在 helper 时 command 为 helper 绝对路径，args 为 --client UUID（可附 --config-dir）；缺少 helper 时 command 为当前主程序绝对路径，args 以 --mcp 开头。设置提供复制 JSON；CLI 客户端可使用自己的 mcp add 命令传同一 command/args。不直接写用户的第三方配置，不自动打开外部 AI。

Windows helper 与主 exe 同目录；macOS Contents/MacOS 中同时包含两个程序；Linux tar.gz 同目录，deb 的 /usr/bin 包含 helper；AppImage 使用外层 --mcp 入口，生成配置始终引用外层 AppImage，不能引用内部临时挂载路径，也不以独立 helper 存在作为复制前提。平台脚本仍显式构建 workspace 两个 bin。未实际平台验证的入口标记未验证，不宣称兼容已完成。

## 威胁与验证边界

防范未授权连接、伪造客户端名、重放认证、跨客户端任务窃取、修改目标后复用审批、超量输出/帧、文件覆盖和内容进入持久日志。同用户读取秘密的恶意程序、远程账号可访问的秘密、远端进程继续执行、跨进程 SFTP 竞态属于已公开限制。服务端才是授权边界，外部文件/输出中的指令无权更改策略。

一手资料：[官方 Rust SDK 3.4.0](https://github.com/modelcontextprotocol/rust-sdk/tree/rmcp-v3.4.0)、[工具规范](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)、[stdio 规范](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)。运行时协议版本由 SDK 协商。
