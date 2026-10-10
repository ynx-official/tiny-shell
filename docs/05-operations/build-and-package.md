# 构建与打包

> 状态：Review
> 最后更新：2026-10-09
> 关联：[项目 README](../../README.md)、[文档索引](../README.md)、[FreeRDP 对接说明](../02-design/remote-desktop-freerdp.md)

本文集中说明 TinyShell 的源码运行、平台依赖、质量检查与打包方式。除获取代码的步骤外，命令均在项目根目录执行。

## 环境要求

- [Rust](https://www.rust-lang.org/tools/install) `1.85.0` 或更高版本。
- 支持 Rust 2024 Edition 的 Cargo。
- Git，用于获取仓库及 Git 依赖。
- Windows：MSVC Build Tools；Windows 远程桌面由系统自带的 `mstsc.exe` 提供，无需安装 FreeRDP。
- macOS：Xcode Command Line Tools；如需从源码使用 Windows 远程桌面，还需安装可由 `pkg-config` 发现的 FreeRDP 3 开发库。
- Linux：C/C++ 构建工具及 GPUI 所需的 X11、Wayland、字体和图形开发库；如需从源码使用 Windows 远程桌面，还需 FreeRDP 3 开发库。

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
cargo build --locked --release
```

## 质量检查

项目 CI 会在 Windows、macOS 和 Linux 上执行格式检查、Clippy、测试和 release 构建；Ubuntu 24.04 门禁还会生成、解包并通过 Xvfb 冒烟启动 AppImage。提交修改前至少运行：

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

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
