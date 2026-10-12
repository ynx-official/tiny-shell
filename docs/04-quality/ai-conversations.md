# 内置 AI 实施与验证

> 状态：Review
> 最后更新：2026-10-12
> 关联：[需求验收编号](../01-requirements/ai-conversations.md)、[交互设计](../02-design/ai-conversations.md)、[实现契约](../03-architecture/ai-conversations.md)

## 回归与执行证据

- 全局 AI 授权回归先失败，返回 CONNECTION_NOT_ALLOWED；修复后开启 AI 的两个对话各自访问绑定目标，无需 MCP 机器授权，外部 MCP 仍仅能访问勾选机器。
- 两种来源的模式和审批独立：外部受控模式不改变 AI 只读规则；修改 AI 模式不撤销外部审批；修改 MCP 模式及清空机器授权不撤销 AI 审批。审计记录任务创建时的模式。
- v1 内置开关一次性迁移至 v2，保留开关及当时模式；重启后不再继承 MCP 后续的模式变化。默认关闭、绑定单目标、IPC 拒绝 ai: 伪造来源、关闭再开启不复活旧能力。
- Provider 收到目标元信息前核对当前已保存连接；旧对话目标摘要变更时不发送目标元信息或工具。只读拒绝所有 Shell 字符串；命令执行需受控审批或完全访问。
- 本机 HTTP/SSE 模型替身以 7 字节分片发送中文和工具参数；MCP 未启用且无机器授权时仍使用 AI 的受控审批，审批前远端计数为零，批准后执行一次。中途 MCP 模式与授权变化不取消 AI 审批。历史可解密，磁盘没有输出明文，正文没有 Key，删除后不复活。
- JSON 回退拒绝截断/无效工具；停止可取消卡在 JSON 正文的请求；保存 Key 不发送给未保存的新地址；系统凭据不可用时保留原密文。
- GPUI 回归真实绘制设置容器并点击确认按钮。181 项模型列表可搜索、通过键盘选中末项。输入 Provider 名称后通知会收敛并可创建第二窗口；同窗口设置跳转复用自身，不复制窗口。
- Windows Credential Manager 专项在此前已通过：随机合成凭据 binary secret/password 读写和删除，不接触真实用户 Key。本轮权限修复未修改凭据实现。
- 工具步骤先以重复按钮旧行为失败，再验证连续三次调用合并为单组且最终答复保持顺序；失败、超时、结果未知、取消、旧活动兼容、重复调用 ID 及旧轮次中断状态通过。GPUI 真实绘制 360px 面板并点击展开/折叠/审阅，修复审阅按钮被挤出窄面板的回归；工具正文无法逃出代码块加载媒体。步骤专项 8/8 通过。
- 本机 SSE/SSH 替身闭环补充验证：待审批 Activity 关联到准确的助手消息、调用 ID 和任务；加密历史恢复后关联字段保持完整。工具调用及远端执行次数不变。
- 完成步骤变细线的回归先失败：真实 GPUI 滚动容器中，运行态工具组高 103.5px，追加长 Markdown 答复后高 0px。修复根节点禁止 flex 压缩后，运行 → 完成 → 长答复 → 展开结果的回归通过；步骤专项现为 9/9。对话通过滚动承载长内容，步骤和结果不因完成而移除。
- 处理占位回归先失败：发送后未能立即显示助手反馈。修复后，从发送至首个正文到达显示占位；完成、失败、中断、工具执行及审批状态不保留或重复该占位。360px 原生 GPUI 布局保持长模型名称与停止按钮在输入容器内，禁用编辑时停止按钮仍可点击；输入区专项 3/3 通过。

## 需求逐项核对（2026-10-12）

以下把服务/引擎验证与原生界面验证区分开；源码存在不等于整条用户流程已验收。

| 验收项 | 当前证据 | 剩余范围 |
| --- | --- | --- |
| AI-01 入口、聊天、历史、隐藏保持运行 | 嵌入星光 SVG；toolbar → toggle_ai；隐藏无 stop 调用；此前 Windows 已打开聊天侧栏 | 最新 release 的返回、隐藏与历史导航原生复核 |
| AI-02 Dock 与 Docker、宽度、覆盖 | ai_dock_uses_overlay_when_the_sidebar_leaves_too_little_terminal_space、switching_to_ai_hides_docker_without_discarding_its_page_or_pending_response 通过；resize 区间 360–600 | 最新 release 的拖拽、窄窗口和 Docker 切换 |
| AI-03 普通聊天、目标绑定 | 新增 first_message_fixes_target_and_rejects_a_second_active_turn、text_only_conditions_do_not_share_ssh_metadata_or_tools 通过；新对话按当前终端 ID 绑定保存态授权身份，后续不随 Tab 变更 | 原生目标选择；工具连接范围沿用原 MCP 的已保存 SSH，不包含临时/RDP/已删除连接 |
| AI-04 历史管理 | render_history 搜索标题/目标、时间及计数；统一 ConfirmationDialog 删除；替身闭环验证密文删除及不复活 | 最新 release 的搜索、重命名及确认交互 |
| AI-05 Provider/模型 | 多 Provider、默认项和逐模型工具声明实现；181 项模型 Select 的真实 GPUI 搜索/键盘选择通过；此前原生 Provider 草稿输入保持响应 | 最新 release 表单排布和滚动复核 |
| AI-06 主动联网及 Key 地址边界 | unsaved_endpoint_never_receives_a_saved_key 通过；固定短消息 max_tokens=8；仅测试/刷新/发送入口触发 HTTP | 真实收费 Provider 未请求；不以替身宣称真实服务兼容 |
| AI-07 按来源权限 | 内部权限 5/5：全局 AI 无需 MCP 机器授权、独立模式、独立审批、迁移及代数撤销；源代码分 Tab | 最新 release 的来源 Tab 与模式说明原生复核 |
| AI-08 元信息与权限提示 | 关闭/无目标/模型无工具/引擎不可用的四种文本请求不含 SSH 元信息或 tools；旧摘要回归通过；IPC 拒绝伪造；开关确认使用统一弹窗 | 真实权限开关及确认由用户操作；自动原生检查只读取 |
| AI-09 受控与隔离 | 本机 SSE/Engine 替身在审批前执行计数零、批准后一次；client/target/session 隔离及只读规则回归通过 | 真实 SSH 工具未执行；原生审批入口复核 |
| AI-10 流式、完整参数、单活动轮、八轮上限 | UTF-8/参数分片、无效 JSON、单活动轮回归通过；runner 明确 for 0..8、无远端重试 | 原生文本流显示及停止状态复核 |
| AI-11 取消、撤销及结果不确定 | 停止卡住的 JSON 请求会关闭响应且不重试；来源模式/启停撤销隔离与目标查询时序回归通过；最后工作区关闭调用 stop_all | 最新 release 关闭最后窗口、后台提示原生复核 |
| AI-12 凭据、密文与本机范围 | Windows 合成凭据专项此前通过；加密/AAD/篡改、历史无明文及 Key 回归通过；AI 不进入 ConfigStore 同步/归档数据 | macOS/Linux 凭据与原生运行未验证 |
| AI-13 临时模式与恢复 | 原密文不覆盖回归通过；open 将活动轮标为 Interrupted；恢复上下文用未知结果占位且不重放 | 最新 release 临时模式提示、重启恢复原生复核 |
| AI-14 国际化、状态及 Markdown | 处理占位与输入区状态/停止专项 3/3；Markdown 图片/HTML 语法树转义回归通过；状态 helper 区分来源/目标/模型；共享主题令牌；历史反馈不改 Provider busy 的失败回归已修复 | 最新 release 的深色主题、复制和真实模型流式状态原生复核 |
| AI-15 工具步骤展示 | 投影/状态/旧历史/代码块及 GPUI 展开折叠审阅、完成后长答复回归 9/9；Windows 独立原生预览中长答复与完整完成步骤同时显示 | 真实 SSH 对话与深色主题原生复核；完整结果展开交互由 GPUI 回归覆盖 |

SSH 目标回归先失败：终端使用仅本次登录的口令或已展开托管密钥时，AI 将运行态摘要误当作保存态授权摘要。修复采用保存态身份，同时拒绝端点/代理变更后的隐式改绑；旧版错误摘要的历史需新建对话，不能自动改写固定目标。

历史/Provider 状态回归先失败：本机 Provider 请求尚未回应时，历史重命名完成错误地将 busy 置 false。现在历史反馈独立保存，Provider 请求保持 busy，完成结果与历史错误均保留。

## 原生验证

此前 Windows 新版 debug 实例已验证：AI 右侧聊天栏、内联 AI 设置编辑后仍响应、工具栏打开独立设置窗口、独立 AI 设置跳转权限页保持同一窗口。旧实现的通知循环和漏绘制确认层已修复。

本轮新权限 Tab 与提示尚待新构建原生检查。运行中的旧 debug 版保留用户会话，已请用户自行关闭；不强制终止。原生自动检查只导航和查看，不修改真实权限、测试收费 Provider 或执行真实 SSH 命令。

工具步骤独立原生预览使用生产 ToolGroupView 和合成数据。Windows 浅色 420px 栏检查三步顺序、命令摘要、成功/待确认/失败状态、组头审阅入口，无重叠或溢出。首次展开操作报告用户输入，随后刷新报告 foreground window did not report a process id，重新枚举时预览窗口已关闭；没有再次启动或操作用户的真实 TinyShell。深色与展开后的原生排布未宣称通过。

细线问题修复后，原生预览增加完成并追加长答复的操作。新截图确认长答复超过视口时，三步完成组保持完整高度和可读内容。随后展开动作报告 foreground window did not report a process id；不以这一动作宣称已完成原生展开验证，完成结果的展开与滚动由真实 GPUI 回归验证。

输入区修订后的 Windows 浅色预览确认：处理文案与三个点位于对话中，底部白色细边框容器包含目标/权限、文本区和模型/停止操作，占位仅保留描述文字，没有快捷键或底部生成文案。预览使用合成数据，未调用真实 Provider/SSH。随后窗口关闭，不扩大自动操作范围。

## 质量门禁

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --locked --all-targets -- -D warnings`：工具高度、处理反馈与输入容器修订后通过。
- `cargo test --locked --bin tiny-shell ai::service::tests:: --no-default-features -- --quiet`：最终服务回归 7/7 通过。
- `cargo test --workspace --locked --all-targets --release -- --quiet`：本轮完整运行通过：主程序 554 通过、2 忽略（系统凭据专项、既有手动性能基准）；工具步骤与输入容器示例 12/12、其他集成与示例测试均通过；内部权限回归 5/5。覆盖此前历史反馈、目标身份修复、工具高度、处理反馈与输入容器修订。
- `cargo build --workspace --locked --release`：工具高度、处理动画与输入容器修订后通过，产物 `target/release/tiny-shell.exe`；包含此前反馈和目标身份修复。
- AI 中英文 123 个键对齐，无新增重复，UI 引用键均存在；新文档相对链接与 `git diff --check` 通过。两种语言原有 local_terminal_opened 重复键不属于本任务。

中间一次修订曾把快捷键提示移至占位第二行并完成格式、Clippy 与构建检查；该显示方式已被用户的新要求替代。当前规范为无快捷键占位、聊天内处理动画和一体输入容器，见交互设计；实际 Enter 发送与 Shift+Enter 换行绑定保留。

常规 debug 全目标测试被原有 tiny-shell-mcp.exe 进程锁住产物，使用全目标 release 测试覆盖同组目标；不强制结束既有客户端进程。Windows 是本轮实际平台；macOS/Linux 原生凭据、窗口和构建未在本机验证。真实收费 Provider 请求和真实 SSH 操作不在自动测试内，自动闭环使用本机模型替身及 Fake Remote，既有 MCP 基础证据见[真实应用冒烟](mcp-live-smoke.md)。
