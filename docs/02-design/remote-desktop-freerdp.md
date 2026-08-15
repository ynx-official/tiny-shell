# FreeRDP 对接说明

TinyShell 的 FreeRDP 接入分成三层：

1. `src/backend/freerdp.rs`：Rust FFI 边界，只接收状态和复制后的 BGRA 帧。
2. `native/freerdp/tiny_shell_freerdp.c`：FreeRDP 3.x 客户端上下文、GDI 帧回调和事件循环。
3. `src/backend/remote_desktop.rs`：Tokio worker、latest-frame mailbox 和后端生命周期。

默认的本地 Cargo 构建不链接 FreeRDP，便于现有 SSH 开发环境继续工作；GitHub 正式发布流水线会安装 FreeRDP 3.x、启用 `freerdp` 特性，并将运行库放入安装包。自行启用原生后端时，需要准备 FreeRDP 3.x 的头文件和库：

```bash
TINY_SHELL_FREERDP_INCLUDE_DIR=/path/to/freerdp/include \
TINY_SHELL_FREERDP_LIB_DIR=/path/to/freerdp/lib \
cargo run --features freerdp
```

如果库文件名称不是默认值，可以通过 `TINY_SHELL_FREERDP_LIBS` 覆盖，多个库使用逗号分隔：

```bash
TINY_SHELL_FREERDP_LIBS=freerdp-client3,freerdp3,winpr3
```

原生层目前已经完成连接参数、GDI BGRA 帧回调、状态回调、事件循环和可中断的断开清理。应用层会消费 latest-frame mailbox、移除原生行填充并将最新 BGRA 帧显示在 GPUI 界面中，同时保留最后一帧以避免重绘时闪烁。邮箱已有未消费帧时只替换数据而不重复投递 UI 唤醒事件；无行填充的紧凑帧直接转移给渲染层，避免第二次整帧拷贝。

应用层还接入了视口尺寸感知、基础键盘/鼠标事件、DISP 动态分辨率和 cliprdr 双向文本剪贴板。视口变化在 200ms 防抖后通过 `SendMonitorLayout` 通知服务端，通道尚未就绪时保留最新尺寸；鼠标坐标按 `ObjectFit::Contain` 的实际画面映射，字母箱留白区域不会发送输入。用户粘贴文本时优先公告 `CF_UNICODETEXT`，cliprdr 不可用时回退为 FreeRDP Unicode 键盘事件；远端文本剪贴板更新会写入本地剪贴板，相同内容不重复写入。

RDP 断开后会按 0.5 秒、1 秒、1.5 秒退避自动重连最多三次；成功连接或用户手动重连会重置预算，延时任务通过 backend generation 避免干扰新连接。用户主动断开、拒绝证书或遇到认证/账户错误时不会自动重连。未知证书会显示当前指纹；证书变化会通过独立回调显示新旧指纹和更强警告。用户可选择“本次信任”“始终信任此指纹”或拒绝；“始终信任”当前限定为进程内主机+端口指纹 pinning，不写入配置文件。UI 不可用、窗口关闭、连接取消或 60 秒超时均保持拒绝。生产构建不得通过忽略证书校验绕过信任流程。

当前默认构建验证记录：`cargo fmt --all -- --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked --all-targets`（364 个测试通过，1 个手工性能基准忽略）以及 `cargo build --locked --release` 均通过。FreeRDP 3.5.1 和当前上游头文件的 C11 `-Wall -Wextra -Werror` 语法检查、`cargo check --locked --all-targets --features freerdp` 及对应的 Clippy 检查均通过；当前 macOS 环境未安装可链接的 FreeRDP 动态库，因此原生特性的最终链接与真实服务器交互仍由多平台发布 CI/手工环境完成。
