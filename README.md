<p align="center">
  <a href="README.md">中文</a> · <a href="README.en.md">English</a>
</p>

<p align="center">
  <img src="assets/icons/tiny-shell.png" alt="TinyShell" width="120" />
</p>

<h1 align="center">TinyShell</h1>

<p align="center">
  <strong>一款使用 Rust 与 GPUI 构建的现代化跨平台桌面终端客户端</strong>
</p>

<p align="center">
  将本地终端、SSH、SFTP、系统监控和配置同步整合到一个快速、清晰的工作区中。
</p>

<p align="center">
  <a href="https://github.com/ynx-official/tiny-shell/releases/latest"><img src="https://img.shields.io/github/v/release/ynx-official/tiny-shell?style=flat-square" alt="Release" /></a>
  <a href="https://github.com/ynx-official/tiny-shell/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/ynx-official/tiny-shell/ci.yml?branch=main&style=flat-square&label=CI" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg?style=flat-square" alt="License" /></a>
  <a href="https://github.com/ynx-official/tiny-shell/releases/latest"><img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey?style=flat-square" alt="Platform" /></a>
</p>

<p align="center">
  <a href="https://github.com/ynx-official/tiny-shell/releases/latest">下载最新版本</a>
  · <a href="CHANGELOG.md">更新日志</a>
  · <a href="https://github.com/ynx-official/tiny-shell/issues">问题反馈</a>
</p>

<p align="center">
  <img src="preview.png" alt="TinyShell 应用预览" width="960" />
</p>

---

## 当前版本

当前版本为 [`v1.1.8`](docs/upgrade/v1.1.8/README.md)，新增终端紧凑显示、纯净工作区、快捷分屏和 SSH 常用命令补全。完整历史请查看[版本记录](docs/upgrade/README.md)。

## 关于 TinyShell

TinyShell 面向需要同时使用本地 Shell 和远程服务器的开发者、运维人员与高级用户。应用基于 [GPUI](https://github.com/zed-industries/zed) 和 [GPUI Component](https://github.com/longbridge/gpui-component) 构建，终端核心使用 `alacritty_terminal`，在保持原生桌面体验的同时提供低延迟终端渲染和统一的连接工作流。

| 终端工作区 | 远程连接 | 文件与监控 |
| --- | --- | --- |
| 本地 Shell、多标签页、Pane 分屏 | SSH 会话、分组、搜索与代理 | SFTP、系统指标与进程查看 |
| 主题、字体、快捷键与布局 | 密码、私钥和加密私钥认证 | WebDAV / S3 配置同步 |

---

## 核心能力

### 终端工作区

- **高性能终端模拟**：基于 `alacritty_terminal`，支持 ANSI 转义序列、真彩色、光标样式和鼠标事件。
- **多标签页与分屏**：在多个标签页中组织会话，并在单个标签页内拆分多个 Pane，构建类似 tmux 的工作区。
- **本地与远程统一体验**：本地 Shell 和 SSH 会话使用一致的终端交互与视觉风格。
- **终端交互**：支持选区、复制粘贴、右键操作和终端鼠标语义。
- **内容高亮规则**：28 条内置规则覆盖日志级别、JSON/logfmt、HTTP、网络地址、标识符、文件位置、堆栈，以及可选的 Git、容器、数据库和安全规则包；支持捕获组、命中解释、全局/分组/会话作用域及 JSON 导入导出。按住 Ctrl（macOS 为 Command）点击可打开 URL/邮箱/本地路径，或复制远程路径、IP、MAC 与 UUID；终端配色和全屏 TUI 原生样式保持不变。
- **跨平台字体**：界面默认跟随系统字体；终端在 Windows 优先使用 Consolas、在 macOS 优先使用 Menlo，并自动使用已安装字体补充中文与 Emoji 字形。
- **Nerd Font 支持**：需要 Powerline/Nerd Font 图标时，请安装并在终端字体设置中选择对应的 Nerd Font。
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

### Docker 工具面板

- **跟随当前会话**：本地终端管理本机 Docker，SSH 终端管理当前远程主机，无需在用户终端中注入命令。
- **容器快捷操作**：查看全部容器，并按状态执行启动、停止或重启；停止和重启前会显示目标确认。
- **镜像查看**：查看目标主机上的镜像、标签、大小和创建时间。
- **无干扰布局**：屏幕空间充足时窗口向右扩展，保持原有终端宽度；空间不足时使用覆盖面板。

### 配置同步与迁移

- **WebDAV 与 S3**：通过常见的远程存储后端同步连接和应用配置。
- **WebDAV 自动对账**：启动、保存、窗口激活、休眠恢复和分钟级周期检查时自动拉取、合并并按需推送。
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

---

## 安装

当前发布流水线提供以下平台产物：

| 平台 | 架构 | 发布产物 |
| --- | --- | --- |
| Windows | x86_64 | 安装版 `.exe`、便携版 `.zip` |
| macOS | Apple Silicon / Intel | 基础版与 RDP 版各提供安装版 `.pkg`、便携版 `.zip` |
| Linux | x86_64 | 单文件 `.AppImage`、通用 `.tar.gz` |

### macOS

从 [Releases 页面](https://github.com/ynx-official/tiny-shell/releases/latest) 下载与处理器对应的 `macos-aarch64` 或 `macos-x86_64` 产物。macOS 提供两个独立版本：

- **基础版（推荐）**：下载不带 `-rdp-` 的 `tiny-shell-*-macos-*-setup.pkg` 或 `tiny-shell-*-macos-*-portable.zip`，不携带 FreeRDP，体积更小。
- **RDP 版**：下载带 `-rdp-` 的 `tiny-shell-*-macos-*-rdp-setup.pkg` 或 `tiny-shell-*-macos-*-rdp-portable.zip`，内置 FreeRDP 运行库，可直接连接 Windows 远程桌面。

两种版本都安装为 `TinyShell.app`，同一台 Mac 上建议只保留正在使用的一个版本；RDP 版会覆盖基础版，反之亦然。

CI 产物使用临时签名。如果 macOS 阻止首次启动，可在“系统设置 → 隐私与安全性”中允许应用运行。若系统提示应用“已损坏”，可以执行：

```bash
sudo xattr -cr /Applications/TinyShell.app
```

### Windows

从 [Releases 页面](https://github.com/ynx-official/tiny-shell/releases/latest) 选择以下任一版本：

- **安装版**：下载 `tiny-shell-*-windows-x86_64-setup.exe`。安装向导提供开始菜单快捷方式、可选桌面快捷方式和标准卸载入口。
- **便携版**：下载 `tiny-shell-*-windows-x86_64-portable.zip`，解压后直接运行 `tiny-shell.exe`，不会写入传统安装信息。

### Linux

推荐下载 `tiny-shell-*-linux-x86_64.AppImage`，赋予执行权限后直接启动：

```bash
chmod +x tiny-shell-*-linux-x86_64.AppImage
./tiny-shell-*-linux-x86_64.AppImage
```

AppImage 版本会被应用内更新器识别；更新时会校验 SHA-256，原子替换外层 AppImage 文件并从该文件重新启动。若系统没有 FUSE 2 兼容层（Ubuntu 24.04 对应 `libfuse2t64`），可安装发行版对应的软件包，或改用 `.tar.gz`：

```bash
tar -xzf tiny-shell-*-linux-x86_64.tar.gz
cd tiny-shell-*-linux-x86_64
./tiny-shell
```

AppImage 会携带 TinyShell、FreeRDP/WinPR 及其非系统动态依赖，但不会捆绑 glibc、显卡驱动或 Mesa/Vulkan 等主机图形栈。Linux 发布仍以 Ubuntu 24.04 的 glibc 为基线；系统缺少图形或字体运行库时，请通过发行版包管理器安装对应的 X11、Wayland、Fontconfig、FreeType 和 OpenGL 运行库。

> 仓库保留了 Debian 包元数据，开发者可以使用 `cargo-deb` 从源码生成 `.deb`；GitHub Release 同时保留 `.tar.gz`，便于无法运行 AppImage 的环境使用。

---

## 数据与安全

TinyShell 会处理 SSH 密码、私钥和代理凭据等敏感信息。使用时请注意以下边界：

- 导入的 SSH 私钥由应用复制并托管，删除原始文件不会影响已有连接。
- 配置同步使用隐私密码保护敏感字段；字段加密采用 Argon2id 派生密钥和 XChaCha20-Poly1305 认证加密。
- 连接归档必须设置非空密码，同样使用 Argon2id 与 XChaCha20-Poly1305；导出的 JSON 不直接保存明文敏感字段。
- 隐私密码校验值不可用于恢复原密码。更换设备后，部分与本机绑定的加密状态可能需要重新输入密码。
- 在线更新下载使用 SHA-256 校验文件完整性，但用户仍应只从项目官方 Releases 页面获取安装包。
- 将配置同步到 WebDAV 或 S3 时，远端服务的访问控制、可用性和数据保留策略仍由服务提供方或部署者负责。

---

## 从源码构建

### 环境要求

- [Rust](https://www.rust-lang.org/tools/install) `1.85.0` 或更高版本。
- 支持 Rust 2024 Edition 的 Cargo。
- Git，用于获取仓库及 Git 依赖。
- Windows：MSVC Build Tools；Windows 远程桌面由系统自带的 `mstsc.exe` 提供，无需安装 FreeRDP。
- macOS：Xcode Command Line Tools；如需从源码使用 Windows 远程桌面，还需安装可由 `pkg-config` 发现的 FreeRDP 3 开发库。
- Linux：C/C++ 构建工具及 GPUI 所需的 X11、Wayland、字体和图形开发库；如需从源码使用 Windows 远程桌面，还需 FreeRDP 3 开发库。

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

在提供该软件包的 Debian/Ubuntu 版本上，可额外安装 FreeRDP 3 开发库：

```bash
sudo apt-get install -y freerdp3-dev
```

Windows 连接会由 TinyShell 生成临时 `.rdp` 配置并调用系统 `mstsc.exe`；请确保系统启用了“远程桌面连接”组件。

### 获取代码并运行

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

macOS/Linux 的非标准 FreeRDP 安装目录可以通过 `TINY_SHELL_FREERDP_INCLUDE_DIRS`、`TINY_SHELL_FREERDP_LIB_DIR` 指定，详见 [FreeRDP 对接说明](docs/02-design/remote-desktop-freerdp.md)。

macOS/Linux 的嵌入式 RDP 支持 Cmd/Ctrl、数字键和文本剪贴板；Mac 本地系统剪贴板中的文件也可以粘贴到远程 Windows。RDP 纯净模式会隐藏侧栏与标签栏，并在顶部自动收起工具栏。Windows 本机仍完全使用系统 `mstsc.exe`，不使用这套 FreeRDP 输入和剪贴板路径。

构建优化版本：

```bash
cargo build --locked --release
```

### 质量检查

项目 CI 会在 Windows、macOS 和 Linux 上执行格式检查、Clippy、测试和 release 构建；Ubuntu 24.04 门禁还会生成、解包并通过 Xvfb 冒烟启动 AppImage。提交修改前至少运行：

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

### 平台打包

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

assets/        图标、主题和平台资源
locales/       中文与英文国际化资源
scripts/       macOS 与 Windows 打包脚本
docs/          版本升级记录和专项文档
.github/       CI 与跨平台发布流水线
```

应用层负责协调窗口、终端、会话、SFTP、同步和系统监控；各领域模块保持独立职责，通过明确接口向上层提供能力。

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
