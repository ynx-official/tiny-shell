[中文](README.md) | [English](README.en.md)

<div align="center">

<img src="assets/icons/tiny-shell.png" alt="TinyShell" width="128" />

# TinyShell

**一款使用 Rust 与 GPUI 构建的现代化跨平台桌面终端客户端**

将本地终端、SSH 连接管理、SFTP 文件操作、远程系统监控和配置同步整合到一个快速、清晰的工作区中。

[![Release](https://img.shields.io/github/v/release/ynx-official/tiny-shell?style=flat-square)](https://github.com/ynx-official/tiny-shell/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/ynx-official/tiny-shell/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/ynx-official/tiny-shell/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey?style=flat-square)](https://github.com/ynx-official/tiny-shell/releases/latest)

[下载最新版本](https://github.com/ynx-official/tiny-shell/releases/latest) · [查看版本记录](docs/upgrade/README.md) · [报告问题](https://github.com/ynx-official/tiny-shell/issues)

</div>

![TinyShell 应用预览](preview.png)

## 关于 TinyShell

TinyShell 面向需要同时使用本地 Shell 和远程服务器的开发者、运维人员与高级用户。应用基于 [GPUI](https://github.com/zed-industries/zed) 和 [GPUI Component](https://github.com/longbridge/gpui-component) 构建，终端核心使用 `alacritty_terminal`，在保持原生桌面体验的同时提供低延迟终端渲染和统一的连接工作流。

你可以在同一个应用中完成以下工作：

- 打开本地终端，通过标签页和 Pane 组织多个任务。
- 保存、分组、搜索并快速连接 SSH 主机。
- 通过图形化 SFTP 面板浏览和维护远程文件。
- 查看本机或远程服务器的 CPU、内存、网络、磁盘和进程信息。
- 通过 WebDAV 或 S3 在多台设备间同步连接配置。
- 使用主题、字体、快捷键和布局设置定制工作区。

## 目录

- [核心能力](#核心能力)
- [快速开始](#快速开始)
- [安装](#安装)
- [数据与安全](#数据与安全)
- [从源码构建](#从源码构建)
- [项目结构](#项目结构)
- [版本历史](#版本历史)
- [技术栈](#技术栈)
- [许可证](#许可证)

## 核心能力

### 终端工作区

- **高性能终端模拟**：基于 `alacritty_terminal`，支持 ANSI 转义序列、真彩色、光标样式和鼠标事件。
- **多标签页与分屏**：在多个标签页中组织会话，并在单个标签页内拆分多个 Pane，构建类似 tmux 的工作区。
- **本地与远程统一体验**：本地 Shell 和 SSH 会话使用一致的终端交互与视觉风格。
- **终端交互**：支持选区、复制粘贴、右键操作和终端鼠标语义。
- **内置字体**：随应用提供 Maple Mono NF CN，兼顾 CJK 字符和 Nerd Font 图标显示。
- **实时外观调整**：可修改终端字体、字号、行间距和主题，无需重启应用。

### SSH 与连接管理

- **多种认证方式**：支持密码、私钥文件和内联私钥，并支持带 `passphrase` 的加密私钥。
- **代理连接**：支持 SOCKS5 和 HTTP 代理，可使用全局代理，也可按连接单独配置。
- **树形连接目录**：通过分组管理大量连接，支持搜索、排序、展开折叠和连接移动。
- **独立连接窗口**：快速连接、新建、编辑、分组操作、移动和归档导入导出使用独立系统窗口，不阻塞主工作区。
- **隔离编辑草稿**：可以同时打开多个 SSH 新建或编辑窗口，各窗口维护独立草稿。
- **安全保存**：连接编辑采用原子写入，并通过乐观并发检测避免多个窗口相互覆盖修改。
- **回收站机制**：连接和分组支持软删除与恢复，同步时保留相应删除状态。

### SFTP 文件管理

SSH 连接建立后，可以直接使用内置 SFTP 文件管理器处理远程文件，无需切换到独立客户端。

- 浏览远程目录并快速跳转路径。
- 上传、下载和拖拽文件。
- 多选文件并执行批量操作。
- 创建、重命名、移动和删除文件或目录。
- 查看并修改远程文件权限。
- 在应用内打开和编辑远程文本文件。
- 查看传输进度、历史和失败状态。

### 系统监控

- **实时遥测**：展示 CPU、内存、Swap、网络和磁盘使用情况及历史趋势。
- **远程采集**：SSH 连接后自动采集远程服务器指标，无需安装额外 Agent。
- **进程查看器**：查看远程服务器进程并执行支持的管理操作。
- **工作区集成**：监控信息与当前会话关联，便于在终端操作期间观察资源变化。

### 配置同步与迁移

- **WebDAV 与 S3**：通过常见的远程存储后端同步连接和应用配置。
- **同步前检查**：上传前验证配置和凭据，减少无效配置导致的失败。
- **隐私密码验证**：同步敏感字段前检查隐私密码，及时识别密码缺失或不一致。
- **冲突合并**：同步模型包含更新时间和删除状态，用于跨设备合并连接目录。
- **加密归档**：连接和分组可导出为带密码保护的 TinyShell JSON 归档，再导入到其他设备。
- **可选敏感字段**：导出归档时可以决定是否包含密码、私钥和代理凭据等敏感信息。

### 主题与效率工具

- **内置主题**：提供 Matrix、Gruvbox、Tokyo Night、Solarized 和 Phygerr 等主题。
- **自定义主题**：可以导入 JSON 主题文件扩展配色方案。
- **亮色与暗色模式**：按使用环境选择合适的界面外观。
- **可折叠侧边栏**：需要专注终端时可释放更多显示空间。
- **快捷键管理**：可视化查看和修改快捷键，并检测按键冲突。
- **命令面板**：集中搜索会话、文件和常用操作。
- **布局持久化**：保存常用界面布局和交互偏好。
- **中英文切换**：应用内支持中文和英文界面切换。

### 跨平台自动更新

TinyShell 可以从 GitHub Releases 检查并安装新版本，且会校验下载文件的 SHA-256 完整性。

- **Windows**：通过独立脚本完成应用替换。
- **macOS**：更新 App Bundle 或安装包。
- **Linux**：使用临时文件和原子替换更新可执行文件。

## 快速开始

1. 从 [Releases 页面](https://github.com/ynx-official/tiny-shell/releases/latest) 下载与你的平台和架构匹配的安装包。
2. 启动 TinyShell，使用本地终端验证字体、主题和 Shell 环境。
3. 在连接管理器中新建 SSH 连接，填写主机、端口、用户名和认证方式。
4. 连接成功后，可在同一工作区打开 SFTP 和远程系统监控。
5. 如果需要跨设备使用连接配置，在设置中配置 WebDAV 或 S3，并为敏感字段设置隐私密码。

> 在导入连接归档或启用配置同步前，请妥善保存加密密码。密码错误时，TinyShell 无法解密归档或同步的敏感字段。

## 安装

当前发布流水线提供以下平台产物：

| 平台 | 架构 | 发布产物 |
| --- | --- | --- |
| Windows | x86_64 | 安装版 `.exe`、便携版 `.zip` |
| macOS | Apple Silicon / Intel | 安装版 `.pkg`、便携版 `.zip` |
| Linux | x86_64 | 通用 `.tar.gz` |

### macOS

#### Homebrew（推荐）

```bash
brew install ynx-official/taps/tiny-shell --cask
```

更新：

```bash
brew update && brew upgrade tiny-shell --cask
```

#### 从 Releases 安装

根据 Mac 的处理器下载对应的 `macos-aarch64` 或 `macos-x86_64` 产物：

- **安装版**：下载 `tiny-shell-*-macos-*-setup.pkg`，按照安装器提示完成安装。
- **便携版**：下载 `tiny-shell-*-macos-*-portable.zip`，解压后将 `TinyShell.app` 移动到“应用程序”目录。

CI 产物使用临时签名。如果 macOS 阻止首次启动，可在“系统设置 → 隐私与安全性”中允许应用运行。若系统提示应用“已损坏”，可以执行：

```bash
sudo xattr -cr /Applications/TinyShell.app
```

### Windows

从 [Releases 页面](https://github.com/ynx-official/tiny-shell/releases/latest) 选择以下任一版本：

- **安装版**：下载 `tiny-shell-*-windows-x86_64-setup.exe`。安装向导提供开始菜单快捷方式、可选桌面快捷方式和标准卸载入口。
- **便携版**：下载 `tiny-shell-*-windows-x86_64-portable.zip`，解压后直接运行 `tiny-shell.exe`，不会写入传统安装信息。

### Linux

下载 `tiny-shell-*-linux-x86_64.tar.gz`，然后执行：

```bash
tar -xzf tiny-shell-*-linux-x86_64.tar.gz
cd tiny-shell-*-linux-x86_64
./tiny-shell
```

如果系统缺少 GPUI 所需的图形或字体运行库，请通过发行版包管理器安装对应的 X11、Wayland、Fontconfig、FreeType、OpenGL 和 GTK 运行库。

> 仓库保留了 Debian 包元数据，开发者可以使用 `cargo-deb` 从源码生成 `.deb`；当前 GitHub Release 流水线默认发布的是通用 `.tar.gz`。

## 数据与安全

TinyShell 会处理 SSH 密码、私钥和代理凭据等敏感信息。使用时请注意以下边界：

- 导入的 SSH 私钥由应用复制并托管，删除原始文件不会影响已有连接。
- 配置同步使用隐私密码保护敏感字段；字段加密采用 Argon2id 派生密钥和 XChaCha20-Poly1305 认证加密。
- 连接归档必须设置非空密码，同样使用 Argon2id 与 XChaCha20-Poly1305；导出的 JSON 不直接保存明文敏感字段。
- 隐私密码校验值不可用于恢复原密码。更换设备后，部分与本机绑定的加密状态可能需要重新输入密码。
- 在线更新下载使用 SHA-256 校验文件完整性，但用户仍应只从项目官方 Releases 页面获取安装包。
- 将配置同步到 WebDAV 或 S3 时，远端服务的访问控制、可用性和数据保留策略仍由服务提供方或部署者负责。

## 从源码构建

### 环境要求

- [Rust](https://www.rust-lang.org/tools/install) `1.85.0` 或更高版本。
- 支持 Rust 2024 Edition 的 Cargo。
- Git，用于获取仓库及 Git 依赖。
- Windows：MSVC Build Tools。
- macOS：Xcode Command Line Tools。
- Linux：C/C++ 构建工具及 GPUI 所需的 X11、Wayland、字体和图形开发库。

### Linux 构建依赖

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

### 获取代码并运行

```bash
git clone https://github.com/ynx-official/tiny-shell.git
cd tiny-shell
cargo run
```

构建优化版本：

```bash
cargo build --locked --release
```

### 质量检查

项目 CI 会在 Windows、macOS 和 Linux 上执行格式检查、Clippy、测试和 release 构建。提交修改前至少运行：

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

### 平台打包

macOS App Bundle：

```bash
./scripts/package-macos-app.sh
```

Windows 安装版与便携版（需要 Inno Setup 6）：

```powershell
./scripts/package-windows.ps1
```

可选 Debian 包：

```bash
cargo install cargo-deb
cargo deb
```

## 项目结构

```text
src/
├── app/       应用编排、窗口、对话框、设置、主题和更新交互
├── backend/   本地终端与 SSH 后端
├── crypto/    同步、归档和配置使用的共享加密原语
├── session/   会话配置、连接目录、存储、归档和 SSH 密钥
├── sftp/      SFTP 文件操作与远程文本文件处理
├── sync/      WebDAV / S3 同步模型、合并和敏感字段处理
├── system/    本地及远程系统信息采集
├── terminal/  终端渲染、输入、选区和终端语义
└── main.rs    应用入口

assets/        图标、字体、主题和平台资源
locales/       中文与英文国际化资源
scripts/       macOS 与 Windows 打包脚本
docs/          版本升级记录和专项文档
.github/       CI 与跨平台发布流水线
```

应用层负责协调窗口、终端、会话、SFTP、同步和系统监控；各领域模块保持独立职责，通过明确接口向上层提供能力。

## 版本历史

当前版本：[`v1.1.7`](docs/upgrade/v1.1.7/README.md)

### v1.1.7

- 将快速连接、分组操作、连接移动和归档导入导出迁移到独立系统窗口，保持主窗口可操作。
- SSH 新建与编辑支持多个独立窗口、隔离草稿、原子保存和乐观并发冲突检测。
- 优化 SSH 编辑窗口尺寸和紧凑布局，支持按 `Esc` 关闭。
- 双击或通过菜单连接会话后自动关闭快速连接窗口。

### v1.1.6

- 将快速连接升级为独立的树形连接管理器。
- 增加搜索、排序、展开折叠、右键操作和连接移动。
- 增加软删除、回收站恢复和同步删除状态。
- 增加带密码加密的 TinyShell JSON 连接归档。

### v1.1.5

- 增加同步隐私密码验证，提前识别密码缺失或不匹配。
- 改进同步设置、操作提示和最近同步状态展示。

完整发布记录、升级说明和版本比较链接请查看 [TinyShell 版本更新记录](docs/upgrade/README.md)。

## 技术栈

| 领域 | 技术 |
| --- | --- |
| 编程语言 | Rust 2024 Edition |
| GUI 框架 | [GPUI](https://github.com/zed-industries/zed) |
| 组件库 | [gpui-component](https://github.com/longbridge/gpui-component) |
| 异步运行时 | [Tokio](https://tokio.rs/) |
| 终端引擎 | [alacritty_terminal](https://github.com/alacritty/alacritty) |
| 本地伪终端 | [portable-pty](https://crates.io/crates/portable-pty) |
| SSH / SFTP | [russh](https://github.com/warp-tech/russh) / `russh-sftp` |
| 国际化 | [rust-i18n](https://github.com/longbridge/rust-i18n) |
| 系统信息 | [sysinfo](https://github.com/GuillaumeGomez/sysinfo) |
| 序列化 | [Serde](https://serde.rs/) |
| 网络请求 | [Reqwest](https://docs.rs/reqwest/) + rustls |
| 密码学 | Argon2id、XChaCha20-Poly1305、SHA-256 |

## 参与贡献

欢迎通过 [Issues](https://github.com/ynx-official/tiny-shell/issues) 报告问题或提出建议。提交代码前请确保：

- 修改符合现有模块职责，不把跨领域逻辑堆入终端或入口模块。
- 用户可见文案同时维护 `locales/zh-CN.yml` 和 `locales/en.yml`。
- 路径、进程、终端和窗口相关改动考虑 Windows、macOS 与 Linux 差异。
- 格式检查、Clippy 和测试全部通过。
- 不提交用户配置、构建产物、调试日志、密钥或真实服务凭据。

## 许可证

TinyShell 基于 [GPL-3.0-or-later](LICENSE) 许可证开源。