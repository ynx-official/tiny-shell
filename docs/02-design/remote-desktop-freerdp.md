# FreeRDP 对接说明

TinyShell 的 FreeRDP 接入分成三层：

1. `src/backend/freerdp.rs`：Rust FFI 边界，只接收状态和复制后的 BGRA 帧。
2. `native/freerdp/tiny_shell_freerdp.c`：FreeRDP 3.x 客户端上下文、GDI 帧回调和事件循环。
3. `src/backend/remote_desktop.rs`：Tokio worker、latest-frame mailbox 和后端生命周期。

默认 Cargo 特性 `freerdp-auto` 会在构建时自动发现 FreeRDP 3.x。找到完整的头文件、链接库和运行库后，普通的 `cargo run` 会编译并启用原生 RDP 后端；没有找到时会输出构建警告并生成不包含 RDP 后端的回退版本，SSH 等其他能力仍可使用。可按需要选择以下模式：

```bash
# 默认：自动发现；依赖缺失时构建无后端回退版本
cargo run

# 强制：必须编译 FreeRDP 后端；依赖缺失时立即失败
cargo run --features freerdp

# 明确禁用自动发现并构建无后端回退版本
cargo run --no-default-features
```

Windows 上会按目标架构查找 vcpkg 安装前缀，支持 `VCPKG_ROOT`、`VCPKG_INSTALLATION_ROOT`、`VCPKG_INSTALLED_DIR`、`VCPKG_TARGET_TRIPLET` 和 `VCPKG_DEFAULT_TRIPLET`，也会检查项目下的 `vcpkg_installed/<triplet>`、`target/vcpkg_installed/<triplet>` 与 `target/*/vcpkg_installed/<triplet>`。例如：

```powershell
vcpkg install "freerdp[client]:x64-windows"
$env:VCPKG_ROOT = "C:\path\to\vcpkg"
cargo run
```

自动发现成功后，构建脚本会将 vcpkg `bin` 目录中的 DLL 复制到 Cargo 的 `OUT_DIR` 并加入运行时搜索路径，因此 `cargo run` 和 `cargo test` 不需要额外修改 `PATH`。直接复制或双击 `target` 中的可执行文件不保证能找到这些 DLL；可独立分发的应用应使用项目打包脚本生成。

放在 `target/` 下的项目本地 vcpkg 安装会被 `cargo clean` 一并删除；需要长期保留依赖时应使用 `VCPKG_ROOT` 或 `VCPKG_INSTALLED_DIR` 指向仓库外的安装位置。安装或移动依赖后若 Cargo 仍复用了无后端构建缓存，可执行 `cargo clean -p tiny-shell` 后重新运行。

macOS/Linux 通过 `pkg-config` 查找版本不低于 3 的 `freerdp-client3`、`freerdp3` 和 `winpr3`。应先使用系统包管理器安装 FreeRDP 3 开发包（Debian/Ubuntu 上的软件包通常为 `freerdp3-dev`）；安装在非标准前缀时，可设置 `PKG_CONFIG_PATH` 后再运行 Cargo。

也可以显式提供安装路径。`TINY_SHELL_FREERDP_INCLUDE_DIRS` 使用平台路径列表分隔符，可同时包含 FreeRDP 与 WinPR 的头文件目录；Windows 的运行库目录可通过 `TINY_SHELL_FREERDP_RUNTIME_DIR`（兼容 `FREERDP_RUNTIME_DIR`）指定：

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

应用层还接入了视口尺寸感知、基础键盘/鼠标事件、DISP 动态分辨率和 cliprdr 双向文本剪贴板。视口变化在 200ms 防抖后通过 `SendMonitorLayout` 通知服务端，通道尚未就绪时保留最新尺寸；鼠标坐标按 `ObjectFit::Contain` 的实际画面映射，字母箱留白区域不会发送输入。用户粘贴文本时优先公告 `CF_UNICODETEXT`，cliprdr 不可用时回退为 FreeRDP Unicode 键盘事件；远端文本剪贴板更新会写入本地剪贴板，相同内容不重复写入。

RDP 断开后会按 0.5 秒、1 秒、1.5 秒退避自动重连最多三次；成功连接或用户手动重连会重置预算，延时任务通过 backend generation 避免干扰新连接。用户主动断开、拒绝证书或遇到认证/账户错误时不会自动重连。未知证书会显示当前指纹；证书变化会通过独立回调显示新旧指纹和更强警告。用户可选择“本次信任”“始终信任此指纹”或拒绝；“始终信任”当前限定为进程内主机+端口指纹 pinning，不写入配置文件。UI 不可用、窗口关闭、连接取消或 60 秒超时均保持拒绝。生产构建不得通过忽略证书校验绕过信任流程。

Linux 正式发布同时生成 `.tar.gz` 与 x86_64 AppImage。`scripts/package-linux-appimage.sh` 固定并校验 `linuxdeploy`，递归部署主程序的非系统动态依赖，且在打包前后检查 FreeRDP/WinPR 三个核心运行库、未解析 ELF 依赖和可重定位 RPATH。AppImage 运行时通过 `$APPIMAGE` 识别外层文件；内置更新器选择 `.AppImage` 资产，将校验后的文件原子替换到外层路径，并从该路径重启，禁止尝试覆盖只读挂载中的 `usr/bin/tiny-shell`。Ubuntu 24.04 CI 会实际生成、解包并在 Xvfb 下限时启动该产物。

2026-08-17 的 Windows 验证使用 vcpkg FreeRDP 3.17.2：默认 `cargo run --locked` 在未注入 FreeRDP 环境变量或 `PATH` 的 shell 中成功启动，生成的 EXE 导入 `freerdp-client3.dll`、`freerdp3.dll` 与 `winpr3.dll`，Cargo `OUT_DIR` 包含 8 个所需 DLL。`cargo test --locked --all-targets` 为 377 个测试通过、1 个手工性能基准忽略；`cargo test --locked --all-targets --no-default-features` 为 376 个通过、1 个忽略；强制后端 `cargo check --locked --no-default-features --features freerdp`、Windows target release 构建、格式检查和不放宽 warning 的 Clippy 均通过。当前未执行 macOS/Linux 本机特性构建，也尚未用本次透明度与纹理回收修复后的构建完成真实 RDP 服务器交互验收；新增 Ubuntu 24.04 FreeRDP CI 用于覆盖默认自动发现、强制后端与显式禁用三种模式，真实服务器仍需手工连接验证。
