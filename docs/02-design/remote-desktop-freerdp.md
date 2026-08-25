# FreeRDP 对接说明

TinyShell 的远程桌面按平台分为两条路径：

- Windows：连接管理器只保存连接信息，双击后生成临时 `.rdp` 配置并启动系统 `mstsc.exe`。TinyShell 不创建 RDP 标签、不渲染桌面帧，也不接管 Windows 客户端的输入、剪贴板、文件复制和全屏。
- macOS/Linux：使用下面的 FreeRDP 三层接入，远程桌面作为 TinyShell 标签显示。

FreeRDP 路径分成三层：

1. `src/backend/freerdp.rs`：Rust FFI 边界，只接收状态和复制后的 BGRA 帧。
2. `native/freerdp/tiny_shell_freerdp.c`：FreeRDP 3.x 客户端上下文、GDI 帧回调和事件循环。
3. `src/backend/remote_desktop.rs`：Tokio worker、latest-frame mailbox 和后端生命周期。

macOS/Linux 的默认 Cargo 特性 `freerdp-auto` 会在构建时自动发现 FreeRDP 3.x。找到完整依赖后，普通的 `cargo run` 会启用原生 RDP 后端；没有找到时会生成不包含 RDP 后端的回退版本。Windows 的 `freerdp` 特性被忽略，始终使用系统 `mstsc.exe`。

```bash
# 默认：自动发现；依赖缺失时构建无后端回退版本
cargo run

# 强制：macOS/Linux 必须编译 FreeRDP 后端；依赖缺失时立即失败
cargo run --features freerdp

# 明确禁用自动发现并构建无后端回退版本
cargo run --no-default-features
```

macOS/Linux 通过 `pkg-config` 查找版本不低于 3 的 `freerdp-client3`、`freerdp3` 和 `winpr3`。应先使用系统包管理器安装 FreeRDP 3 开发包（Debian/Ubuntu 上的软件包通常为 `freerdp3-dev`）；安装在非标准前缀时，可设置 `PKG_CONFIG_PATH` 后再运行 Cargo。

也可以显式提供 macOS/Linux 的安装路径。`TINY_SHELL_FREERDP_INCLUDE_DIRS` 使用平台路径列表分隔符，可同时包含 FreeRDP 与 WinPR 的头文件目录；`TINY_SHELL_FREERDP_LIB_DIR` 指定库目录：

```bash
TINY_SHELL_FREERDP_INCLUDE_DIRS=/prefix/include/freerdp3:/prefix/include/winpr3 \
TINY_SHELL_FREERDP_LIB_DIR=/prefix/lib \
cargo run --features freerdp
```

单个头文件目录也可通过 `TINY_SHELL_FREERDP_INCLUDE_DIR` 指定。如果库文件名称不是默认值，可以通过 `TINY_SHELL_FREERDP_LIBS` 覆盖，多个库使用逗号分隔：

```bash
TINY_SHELL_FREERDP_LIBS=freerdp-client3,freerdp3,winpr3
```

原生层目前已经完成连接参数、GDI BGRA 帧回调、状态回调、事件循环和可中断的断开清理。协商 RDPGFX 时会在动态通道建立与断开阶段分别初始化、清理 FreeRDP GDI graphics pipeline，确保图形更新仍进入统一的 `BeginPaint` / `EndPaint` 帧回调；只有链接的 FreeRDP 本身包含 H.264 解码器时才公告 GFX H.264 能力。应用层会消费 latest-frame mailbox、移除原生行填充并将最新 BGRA 帧显示在 GPUI 界面中；FreeRDP 的 BGRX 路径不保证第 4 字节可作为透明度，因此进入 GPUI 前统一将桌面像素规范为不透明。动态画面只保留当前与上一张实际渲染纹理，更旧纹理通过 `Window::drop_image` 从 GPUI atlas 显式回收，避免长连接持续占用 GPU 内存。邮箱已有未消费帧时只替换数据而不重复投递 UI 唤醒事件；帧通知通过高优先级生命周期队列非阻塞投递，并在下游路由队列溢出时优先保留，若首次投递失败则重新武装并在下一帧重试，避免永久停留在等待首帧状态，也不会阻塞 FreeRDP 事件线程。无行填充的紧凑帧直接转移给渲染层，避免第二次整帧拷贝。

应用层还接入了视口尺寸感知、基础键盘/鼠标事件、DISP 动态分辨率和 cliprdr 双向文本剪贴板。视口变化在 200ms 防抖后通过 `SendMonitorLayout` 通知服务端，通道尚未就绪时保留最新尺寸；鼠标坐标按 `ObjectFit::Contain` 的实际画面映射，字母箱留白区域不会发送输入。用户粘贴文本时优先公告 `CF_UNICODETEXT`，cliprdr 不可用时回退为 FreeRDP Unicode 键盘事件；远端文本剪贴板更新会写入本地剪贴板，相同内容不重复写入。macOS/Linux 嵌入式会话使用本机系统剪贴板，其中 Mac 本地剪贴板的文件条目会以 `FileGroupDescriptorW` + `FileContents` 发布给远端 Windows，文件内容按请求分块读取；Windows 本机不走这条路径，仍由系统 `mstsc.exe` 接管剪贴板。

RDP 帧邮箱会分别统计发布和消费 FPS，界面同时显示接收与消费指标；FreeRDP 事件等待上限为 16ms，鼠标移动在发送端和事件线程两级合并，避免高频指针事件挤占键盘、剪贴板命令。连接握手超过 30 秒会被后端主动取消；连接成功后如果 15 秒仍未收到首帧，后端也会主动结束本次会话并进入可重试的失败流程，避免永久停留在等待画面。RDP 工具栏提供纯净模式：隐藏侧栏、标签栏和底部面板，顶部工具栏默认收起，鼠标移入顶部热区后展开；再次点击最小化按钮恢复普通工作区。

RDP 断开后会按 0.5 秒、1 秒、1.5 秒退避自动重连最多三次；成功连接或用户手动重连会重置预算，延时任务通过 backend generation 避免干扰新连接。用户主动断开、拒绝证书或遇到认证/账户错误时不会自动重连。未知证书会显示当前指纹；证书变化会通过独立回调显示新旧指纹和更强警告。用户可选择“本次信任”“始终信任此指纹”或拒绝；“始终信任”当前限定为进程内主机+端口指纹 pinning，不写入配置文件。UI 不可用、窗口关闭、连接取消或 60 秒超时均保持拒绝。生产构建不得通过忽略证书校验绕过信任流程。

Linux 正式发布同时生成 `.tar.gz` 与 x86_64 AppImage。`scripts/package-linux-appimage.sh` 固定并校验 `linuxdeploy`，递归部署主程序的非系统动态依赖，且在打包前后检查 FreeRDP/WinPR 三个核心运行库、未解析 ELF 依赖和可重定位 RPATH。AppImage 运行时通过 `$APPIMAGE` 识别外层文件；内置更新器选择 `.AppImage` 资产，将校验后的文件原子替换到外层路径，并从该路径重启，禁止尝试覆盖只读挂载中的 `usr/bin/tiny-shell`。Ubuntu 24.04 CI 会实际生成、解包并在 Xvfb 下限时启动该产物。

Windows 原生启动器只把主机、端口和用户名写入临时 `.rdp` 文件，不保存密码；系统客户端使用 Windows 凭据管理器或显示自己的凭据提示。`mstsc.exe` 退出后临时文件会自动删除。

Windows 使用系统 `mstsc.exe`，不再链接或打包 FreeRDP DLL；macOS/Linux 继续由 FreeRDP CI 覆盖自动发现、强制后端与显式禁用三种模式。真实 RDP 服务器交互仍需在对应平台手工连接验证。
