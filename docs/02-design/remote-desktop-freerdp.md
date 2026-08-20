# FreeRDP 对接说明

TinyShell 的 FreeRDP 接入分成三层：

1. `src/backend/freerdp.rs`：Rust FFI 边界，只接收状态和复制后的 BGRA 帧。
2. `native/freerdp/tiny_shell_freerdp.c`：FreeRDP 3.x 客户端上下文、GDI 帧回调和事件循环。
3. `src/backend/remote_desktop.rs`：Tokio worker、latest-frame mailbox、BGRA stride 归一化和 GPUI 图像转换。
4. `src/app/ui/terminal.rs`：使用持久 `RenderImage` 绘制远程桌面画面，避免每次 UI 重绘重复上传纹理。

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

原生层目前已经完成连接参数、GDI BGRA 帧回调、状态回调、事件循环和断开清理；GPUI 持久纹理上传已接入，鼠标键盘映射、剪贴板和证书信任策略仍需在后续阶段接入。生产构建不得通过忽略证书校验绕过信任流程。
