# FreeRDP 对接说明

TinyShell 的 FreeRDP 接入分成三层：

1. `src/backend/freerdp.rs`：Rust FFI 边界，只接收状态和复制后的 BGRA 帧。
2. `native/freerdp/tiny_shell_freerdp.c`：FreeRDP 3.x 客户端上下文、GDI 帧回调和事件循环。
3. `src/backend/remote_desktop.rs`：Tokio worker、latest-frame mailbox 和后端生命周期。

默认构建不链接 FreeRDP，便于现有 SSH 构建继续工作。启用原生后端时，需要准备 FreeRDP 3.x 的头文件和库：

```bash
TINY_SHELL_FREERDP_INCLUDE_DIR=/path/to/freerdp/include \
TINY_SHELL_FREERDP_LIB_DIR=/path/to/freerdp/lib \
cargo run --features freerdp
```

如果库文件名称不是默认值，可以通过 `TINY_SHELL_FREERDP_LIBS` 覆盖，多个库使用逗号分隔：

```bash
TINY_SHELL_FREERDP_LIBS=freerdp-client3,freerdp3,winpr3
```

原生层目前已经完成连接参数、GDI BGRA 帧回调、状态回调、事件循环和断开清理。应用层会消费 latest-frame mailbox、移除原生行填充并将最新 BGRA 帧显示在 GPUI 界面中，同时保留最后一帧以避免重绘时闪烁。

应用层还接入了视口尺寸感知、基础键盘/鼠标事件和本地剪贴板文本粘贴：视口尺寸用于按 `ObjectFit::Contain` 结果映射输入，字母箱留白区域不会发送鼠标事件；在 DISP 动态分辨率通道接入前，远端桌面保持服务端已协商的分辨率，不会仅在本地重置 GDI 帧缓冲区。粘贴内容通过 FreeRDP Unicode 键盘事件发送，避免把 RDP 文本误写入终端解析器。RDP 断开后会按 0.5 秒、1 秒、1.5 秒退避自动重连最多三次，成功连接后清除重试计数；用户主动断开不会触发自动重连。未知或变化的证书会暂停连接并显示主机、端口和指纹，用户可选择“本次信任”“始终信任此指纹”或拒绝；永久信任当前限定为进程内主机+端口指纹 pinning，不写入配置文件，UI 不可用、窗口关闭或 60 秒超时均保持拒绝。服务端到本地的剪贴板通知和持久 GPU 纹理原位更新仍待后续阶段完成。生产构建不得通过忽略证书校验绕过信任流程。

当前默认构建验证记录：`cargo fmt --all -- --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked --all-targets`（363 个测试通过，1 个忽略）以及 `cargo build --locked --release` 均通过。启用 `--features freerdp` 的构建仍需目标平台安装 FreeRDP 3.x 头文件和库；缺少 `freerdp/client.h` 时不会伪称原生后端已验证。
