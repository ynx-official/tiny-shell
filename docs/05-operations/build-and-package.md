# 构建与打包

> 状态：Review
> 最后更新：2026-10-16
> 关联：[项目 README](../../README.md)、[文档索引](../README.md)、[FreeRDP 对接说明](../02-design/remote-desktop-freerdp.md)

本文集中说明 TinyShell 的源码运行、平台依赖、质量检查与打包方式。除获取代码的步骤外，命令均在项目根目录执行。

## 环境要求

- [Rust](https://www.rust-lang.org/tools/install) `1.96.1` 或更高版本。
- 支持 Rust 2024 Edition 的 Cargo。
- Git，用于获取仓库及 Git 依赖。
- Windows：MSVC Build Tools；Windows 远程桌面由系统自带的 `mstsc.exe` 提供，无需安装 FreeRDP。
- macOS：Xcode Command Line Tools；如需从源码使用 Windows 远程桌面，还需安装可由 `pkg-config` 发现的 FreeRDP 3 开发库。
- Linux：C/C++ 构建工具及 GPUI 所需的 X11、Wayland、字体和图形开发库；如需从源码使用 Windows 远程桌面，还需 FreeRDP 3 开发库。

仓库使用 `rust-toolchain.toml` 固定 Rust 1.96.1，并安装 rustfmt、Clippy；CI 与发布构建使用相同版本。通过 rustup 执行 `cargo` 时自动选择该工具链，不修改机器的全局默认工具链。依赖更新后应实际使用 Rust 1.96.1 检查锁文件兼容性。

## Linux 构建依赖

Debian/Ubuntu 可安装与 CI 一致的依赖：

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config cmake \
  libfontconfig1-dev libfreetype6-dev \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libgl1-mesa-dev libegl1-mesa-dev libgtk-3-dev \
  libudev-dev
```

在提供该软件包的 Debian/Ubuntu 版本上，可额外安装 FreeRDP 3 开发库：

```bash
sudo apt-get install -y freerdp3-dev
```

Windows 连接会由 TinyShell 生成临时 `.rdp` 配置并调用系统 `mstsc.exe`；请确保系统启用了“远程桌面连接”组件。

## 获取代码并运行

```bash
git clone https://github.com/ynx-official/tiny-shell.git
cd tiny-shell
cargo run
```

从源码运行也支持 MCP：复制配置时，若同目录没有 `tiny-shell-mcp`，应用自动使用当前主程序的 `--mcp` stdio 入口，无需另行构建桥接程序。外部客户端仍需要已注册的客户端 UUID，并通过本机桥接连接正在运行的 TinyShell。完整安装包优先使用独立 helper；AppImage 使用外层 AppImage 的 `--mcp` 入口。

仍在运行修复前的开发程序时，可以在同一 Cargo target 目录执行 `cargo build --locked -p tiny-shell-mcp --bin tiny-shell-mcp`，补齐 helper 后再次复制；或关闭旧窗口再运行更新后的源码。若设置了 `CARGO_TARGET_DIR`，构建 helper 时需要使用相同的值。缺失 helper 的真实握手与配置导出验证见 [MCP 源码运行桥接回归](../04-quality/mcp-cargo-run-bridge.md)。

默认特性 `freerdp-auto` 仅用于 macOS/Linux，通过 `pkg-config` 查找 `freerdp-client3`、`freerdp3` 和 `winpr3`。Windows 不编译 FreeRDP，而是在双击 RDP 连接时调用系统 `mstsc.exe`。未发现 macOS/Linux FreeRDP 时仍可构建和运行，但会使用不包含 RDP 后端的回退版本。

需要保证原生后端存在时使用强制模式；依赖缺失会立即构建失败。若明确只需无 RDP 后端的版本，则关闭默认特性；发布版 macOS 基础包就是按此方式构建的：

```bash
cargo run --features freerdp # macOS/Linux
cargo run --no-default-features
```

macOS/Linux 的非标准 FreeRDP 安装目录可以通过 `TINY_SHELL_FREERDP_INCLUDE_DIRS`、`TINY_SHELL_FREERDP_LIB_DIR` 指定，详见 [FreeRDP 对接说明](../02-design/remote-desktop-freerdp.md)。

macOS/Linux 的嵌入式 RDP 支持 Cmd/Ctrl、数字键和文本剪贴板；Mac 本地系统剪贴板中的文件也可以粘贴到远程 Windows。RDP 纯净模式会隐藏侧栏与标签栏，并在顶部自动收起工具栏。Windows 本机仍完全使用系统 `mstsc.exe`，不使用这套 FreeRDP 输入和剪贴板路径。

构建优化版本：

```bash
cargo build --workspace --locked --release
```

## 质量检查

项目 CI 会在 Windows、macOS 和 Linux 上执行格式检查、Clippy、测试和 release 构建；Ubuntu 24.04 门禁还会生成、解包并通过 Xvfb 冒烟启动 AppImage。提交修改前至少运行：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo test --workspace --locked --all-targets
```

### 代码覆盖率

需要覆盖率时安装 `cargo-llvm-cov` 与当前 Rust 工具链对应的 LLVM 组件，然后运行完整的当前平台目标：

```bash
rustup component add llvm-tools-preview --toolchain 1.96.1
cargo +stable install cargo-llvm-cov --locked
cargo llvm-cov --locked --all-targets --html
```

报告默认在 `target/llvm-cov/html/index.html`。覆盖率构建使用独立缓存，不上传到外部服务；行覆盖率、函数覆盖率和区域覆盖率按实际结果报告。稳定版不提供完整分支覆盖率，当前平台的结果也不能替代其他平台原生验证。项目原有的手动性能基准不属于默认自动测试。

## 平台打包

macOS App Bundle：

```bash
./scripts/package-macos-app.sh                  # 基础版，体积更小
./scripts/package-macos-app.sh --edition rdp    # RDP 版，需要 FreeRDP 3
```

RDP 版脚本会使用 `dylibbundler` 将 FreeRDP 动态库收入 App Bundle；请先运行 `brew install freerdp dylibbundler`。发布流程会分别生成基础版和 RDP 版的 `.pkg` 与 `.zip`。

Windows 安装版与便携版（需要 Inno Setup 6）：

```powershell
./scripts/package-windows.ps1
```

Windows 安装包不携带 FreeRDP DLL，运行时使用系统 `mstsc.exe`。

Linux AppImage（需要 FreeRDP 3 开发包、`curl`、`desktop-file-utils`、`file` 和 `patchelf`）：

```bash
bash scripts/package-linux-appimage.sh
```

脚本固定使用 `linuxdeploy 1-alpha-20251107-1` 并校验下载文件的 SHA-256；也可通过 `--linuxdeploy <path>` 使用已准备好的工具。输出为 `dist/tiny-shell-vX.Y.Z-linux-x86_64.AppImage`。

可选 Debian 包：

```bash
cargo install cargo-deb
cargo deb
```

## MCP

使用 `cargo build --workspace --locked` 或平台打包脚本同时构建主应用与 `tiny-shell-mcp`。在设置 → MCP 中启用、选择权限并勾选连接，再注册客户端。新安装默认关闭、只读、无授权连接。复制配置引用与应用同目录的 helper；AppImage 配置引用外层 AppImage 的 `--mcp` 入口。

通用 JSON 使用 `mcpServers`；不同 CLI 的配置格式可能不同，应将相同 command 与 args 配到其 MCP 配置中。保持 TinyShell 运行，受控操作在设置页审阅。更改权限会关闭当前 AI 会话，客户端需重新打开。

首版不提供 PTY、stdin、sudo 密码、二进制上传下载或结构化删除；`cwd` 因缺少已确认的远程 Shell 类型返回 `UNSUPPORTED_SHELL`，可将明确的目录切换写进完整命令并审阅。取消或超时无法保证远端进程停止，先检查结果再重试。

实现与实际验证见 [MCP 实施与验收](../04-quality/mcp-implementation.md)。

## 线上更新说明

从 v2.1.2 开始，每份版本详情需要包含“## 线上更新说明”，其正文直接编写面向用户的独立纯文本，不包含 Markdown 标记、文档链接、内部路径或发布验证记录。Markdown 详情仍可保存完整工程记录及相互引用。

发布工作流使用 `python scripts/release_notes.py --tag vX.Y.Z --plain-text --output dist/release-notes.txt` 导出，缺少说明或混入 Markdown/文档引用时阻止发布。`release-notes.txt` 同时作为 GitHub Release 正文和更新清单 notes_url 的目标；更新器 HTML 回退也优先读取该附件。清单 schema_version 和原版本比较/产物校验规则不变。

仅跳过本地测试时，不添加“发布测试策略：skip”标记，云端发布工具测试继续执行。
