# 统一确认弹窗

> 状态：Approved  
> 最后更新：2026-10-12  
> 关联：[文档索引](../README.md)、[项目规范](../../AGENTS.md)、[工作区设计](workspace-ui/design.md)

## 适用范围与视觉约定

根据已确认的关闭弹窗效果图，将“标题、说明、取消、执行”两操作确认统一到 `src/app/confirmation_dialog.rs` 的 `ConfirmationDialog`。继续使用项目的 awesome-design-md / Sentry 风格与原生 GPUI 弹窗层。

- 默认宽度 380px、内边距 24px、圆角 8px；标题 17px、说明 13px、按钮高 32px。随应用字号放大，并保留窗口两侧各 16px 留白。
- 按文字真实换行计算高度，不为简短说明预留空白。长说明和目标清单可滚动，按钮保留在底部。
- 按钮靠右、不拉伸；长按钮在窄窗口中纵向排列。取消为描边按钮，普通执行操作使用主题主色，危险操作使用实心红色与白字，并提供悬停、按下和键盘焦点状态。
- 弹窗背景、正文、边框与遮罩沿用当前主题；危险按钮使用已确认的 `#c93737`，避免上游 Danger/Custom 变体的半透明浅色效果。
- 禁止点击遮罩关闭，不额外提供右上角叉号。默认支持 Escape 取消；焦点位于按钮时 Enter / Space 执行该按钮。保留编辑器原有的 `keyboard(false)` 约束，禁用弹窗级 Enter / Escape，避免与编辑器快捷键冲突。

## 已接入的场景

- 主窗口关闭，显示当前窗口实际活动连接数；无活动连接仍保留原有确认流程。
- 删除保存的连接、删除被连接引用的密钥。
- SFTP 普通删除、快速删除；保留选中目标、不可撤销提示、系统路径或快速删除警告。
- 编辑器关闭未保存文件、关闭全部文件、会话关闭、切换编码、覆盖冲突文件、重新加载冲突文件。
- Docker 停止、移除、强制移除与重启确认，保留目标与 generation 检查。
- 更新安装并重启，安装失败时保持弹窗并保留既有错误处理。
- 同步敏感信息被阻止后的重置入口提示；实际重置仍进入原有密码表单和验证流程。

包含输入框、选择器、复杂操作表单的窗口继续使用原有组件。确认组件不读取业务状态，不连接服务，不自行决定删除、保存或重试规则。

## 关闭窗口时的等待状态

点击“关闭窗口”后，原弹窗保持打开，标题切换为“正在关闭窗口”，说明为“正在完成同步并结束会话，请稍候。”。红色执行按钮显示持续旋转的白色图标及“关闭中…”，取消按钮置灰。等待期间不响应取消、Escape、Enter、Space 或重复确认；原生标题栏再次请求关闭也不会创建第二个确认。

关闭确认通过 `pending_on_confirm(title, description, label)` 启用等待状态：确认回调返回 `true` 表示操作已接受，组件切换为等待态并保留弹窗，由业务方完成同步、保存和清理后移除窗口。组件使用明确的 Ready / Acting / Pending / Completed 状态，鼠标与键盘共用同一个动作守卫，防止延迟回调重复执行。回调返回 `false` 时仍允许重试或取消。

确认态与等待态共同参与高度和按钮宽度测量，切换时保持原位置和大小；长文本、应用大字号与窄窗口仍沿用原有换行、滚动及按钮纵向排列规则。普通确认未启用等待状态时继续在接受操作后关闭弹窗。

主窗口仍在收到关闭请求时启动原有后台同步，确认后等待该流程结束，再执行已有保存和清理路径。同步失败时沿用既有失败处理。等待弹窗随所属窗口一起移除，不在同步尚未完成时单独关闭。

## 快速接入

独立窗口或不需要业务模态队列的场景：

```rust
use crate::app::confirmation_dialog::ConfirmationDialog;

ConfirmationDialog::new(
    t!("confirm_delete").to_string(),
    t!("session_delete_confirm", name = session_name).to_string(),
)
.danger(true)
.confirm_label(t!("delete").to_string())
.on_ok(move |_, window, cx| {
    // 业务操作在调用处处理；返回 false 时弹窗保持打开。
    perform_action(window, cx);
    true
})
.open(window, cx);
```

需要主应用按窗口隔离、排队或替换弹窗时，仍调用 `open_modal_dialog` / `replace_modal_dialog`，在 builder 末尾使用 `.build(dialog, window, cx)`，并通过 `.on_close(...)` 调用原有的 `modal_dialog_closed(token, window, cx)`。可参考 `src/app/dialogs/deletion.rs`。

调用约束：

1. 标题、说明、按钮文本来自中英双语资源。不要传示例连接数量。
2. 普通确认省略 `.danger(true)`；取消按钮文字可通过 `.cancel_label(...)` 调整。
3. 普通确认的 `on_ok` / `on_cancel` 返回 true 后，由组件关闭弹窗并调用一次 `on_close`。回调内不要再次调用 `close_dialog` 或 `dismiss_modal_dialog`，避免误关下一层。启用 `pending_on_confirm` 的关闭确认例外：`on_ok` 返回 true 进入等待，由业务方移除窗口，取消只在确认前可用。
4. 鼠标和键盘共用延迟回调路径，避免窗口更新重入。下一步要打开其他弹窗时，使用 `window.defer(...)`。
5. `open` 适用于单层或覆盖现有业务弹窗的确认；已有复杂模态流优先走业务队列，避免任意叠加多层确认。

## 独立原生预览

```bash
cargo run --locked --example confirmation-dialog
```

示例直接引用生产组件，提供短提示、长列表、普通确认、关闭中演示、主题切换、字号切换与中英切换。启动默认展示关闭确认；“关闭中演示”接受确认后保持等待 10 秒，再模拟完成并关闭弹窗。示例只记录确认次数，不加载用户配置、不执行删除、不连接远程服务，可用于后续视觉回归。

## 验证

布局测试先在旧固定高度模型下确认失败，再改为按实测文本计算。单元测试覆盖简短内容、换行增长、大字号、窄窗口、重复提交和回调拒绝后重试。

2026-09-30 实际执行结果：

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --locked --all-targets -- -D warnings`：通过。
- `cargo test --locked --all-targets --quiet`：主程序 485 项通过、1 项原有手动性能基准忽略；构建契约 1 项通过；示例复用的组件与布局测试 11 项通过。
- `cargo build --locked --bins --examples`：Windows 开发构建通过。
- Windows 原生组件预览：检查了简短提示、按钮右对齐、亮暗主题、21px 应用字号与英文换行、长列表滚动到末项、鼠标确认与取消、Escape 取消、Tab 焦点与 Enter 取消。

键盘回归步骤：打开短确认 → 按 Tab 使“取消”获得焦点 → 按 Enter。修复前会增加确认次数；修复后仅显示取消状态，确认次数不变。原因是上游 Dialog 的 Enter 动作早于原始 key-down 回调处理；现在由获得焦点的按钮拦截 ConfirmDialog 动作。

未执行真实服务器删除、文件覆盖、配置同步重置或安装更新。未运行 macOS / Linux 原生窗口验证，也未构建或发布 release 安装包。
