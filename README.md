[中文](README.md) | [English](README.en.md)

# TinyShell

<p align="center">
  <img src="assets/icons/tiny-shell.png" alt="TinyShell" width="128" />
</p>

<p align="center">
  <strong>一款现代化、高性能的 Rust 桌面终端客户端</strong>
</p>

<p align="center">
  <a href="https://github.com/ynx-official/tiny-shell/releases/latest"><img src="https://img.shields.io/github/v/release/ynx-official/tiny-shell?style=flat-square" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/ynx-official/tiny-shell"><img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey?style=flat-square" alt="Platform"></a>
</p>

---

TinyShell 是一款基于 GPUI 框架和 GPUI Component 组件库构建的桌面终端客户端，采用 Rust 编写。它将本地终端、SSH 远程连接、SFTP 文件管理、系统监控等能力整合在一个高性能、美观的统一工作区中。

## 功能特性

### 🖥️ 终端体验
- **Alacritty 终端引擎** — 基于 `alacritty_terminal` 的高性能终端模拟器，支持完整的 ANSI 转义序列、真彩色、光标样式和鼠标事件
- **多标签页与分屏** — 支持多个标签页，每个标签页内可自由拆分多个 Pane，提供类 tmux 的工作流体验
- **内置等宽字体** — 开箱自带 Maple Mono NF CN 字体，完美支持 CJK 字符与 Nerd Font 图标
- **全局字体与字号调节** — 支持实时调整终端字体、字号和行间距

### 🔗 远程连接
- **SSH 客户端** — 支持密码、密钥文件、内联密钥三种认证方式，支持 `passphrase` 加密私钥
- **会话管理** — 保存、编辑、删除和快速切换 SSH 会话，支持分组管理
- **SFTP 文件管理器** — 内置图形化 SFTP 客户端，支持上传、下载、拖拽、多选、远程文件编辑和权限修改
- **代理支持** — 支持 SOCKS5 代理和 HTTP 代理，可全局配置或按会话独立配置

### 📊 系统监控
- **实时遥测** — 左侧边栏实时展示 CPU、内存、Swap、网络和磁盘的使用率与历史曲线
- **远程监控** — 通过 SSH 连接后自动采集远程服务器的系统指标，无需额外安装 agent
- **进程管理** — 内置进程查看器，可查看和管理远程服务器上的运行进程

### 🎨 主题与外观
- **多主题切换** — 内置 Matrix、Gruvbox、Tokyo Night、Solarized、Phygerr 等多种主题，支持亮色/暗色模式
- **自定义主题** — 支持导入自定义 JSON 主题文件
- **紧凑布局** — 可折叠侧边栏，支持全屏沉浸式终端体验

### ⚙️ 效率工具
- **快捷键系统** — 可视化快捷键管理，支持查看、修改和冲突检测
- **命令面板** — 全局快捷键快速切换会话、打开文件、执行操作
- **配置同步** — 支持通过 WebDAV 或 S3 同步会话配置，多设备间无缝切换
- **导入密钥管理** — 密钥导入后由应用托管，删除原文件不影响已配置的连接

### 🔄 自动更新
- **跨平台更新** — 内置自动更新功能，支持从 GitHub Releases 检查、下载和安装最新版本
- **平台适配** — Linux 原子替换、macOS App Bundle 替换、Windows 批处理脚本，各平台采用最佳策略

## 安装

### macOS

#### Homebrew（推荐）
```bash
brew install ynx-official/taps/tiny-shell --cask
```

更新：
```bash
brew update && brew upgrade tiny-shell --cask
```

#### 手动安装
从 [Releases 页面](https://github.com/ynx-official/tiny-shell/releases/latest) 下载 `tiny-shell-*-macos-*.zip`，解压后将 `TinyShell.app` 拖入应用程序目录。

> 首次启动若提示"已损坏"，执行：
> ```bash
> sudo xattr -cr /Applications/TinyShell.app
> ```

### Windows

从 [Releases 页面](https://github.com/ynx-official/tiny-shell/releases/latest) 选择需要的版本：

- **安装版**：下载 `tiny-shell-*-windows-x86_64-setup.exe`，运行安装向导。支持开始菜单快捷方式、可选桌面快捷方式及系统卸载入口。
- **便携版**：下载 `tiny-shell-*-windows-x86_64-portable.zip`，解压后直接运行 `tiny-shell.exe`，无需安装。

### Linux

#### Debian/Ubuntu（.deb 包）
从 [Releases 页面](https://github.com/ynx-official/tiny-shell/releases/latest) 下载 `.deb` 包：
```bash
sudo dpkg -i tiny-shell_*.deb
```

#### 通用 Linux（tar.gz）
```bash
tar -xzf tiny-shell-*-linux-x86_64.tar.gz
cd tiny-shell-*-linux-x86_64
./tiny-shell
```

## 从源码构建

### 前置条件
- Rust 工具链 1.85+
- Linux: `pkg-config` `libfontconfig1-dev` `libxcb1-dev` 等 X11/Wayland 开发库
- macOS: Xcode Command Line Tools
- Windows: MSVC 构建工具

### 构建与运行
```bash
# 克隆仓库
git clone https://github.com/ynx-official/tiny-shell.git
cd tiny-shell

# 运行
cargo run --release

# 打包 macOS App Bundle
./scripts/package-macos-app.sh

# 打包 Linux .deb
cargo install cargo-deb
cargo deb
```

## 版本历史

### v1.1.7
- 将快速连接、分组操作、连接移动和归档导入导出迁移到独立系统窗口，保持主窗口可操作
- SSH 新建与编辑支持多个独立窗口、草稿隔离、原子保存和乐观并发冲突检测
- 优化 SSH 编辑窗口尺寸和紧凑布局，并支持按 `Esc` 关闭连接窗口
- 双击或通过菜单连接会话后自动关闭快速连接窗口

### v1.1.6
- 将快速连接升级为独立的树形连接管理器
- 支持连接和分组的搜索、排序、展开折叠、右键操作与移动
- 支持连接和分组软删除、回收站恢复以及同步墓碑
- 支持带密码加密的 TinyShell JSON 导入和导出，敏感字段禁止明文保存

### v1.1.5
- 为敏感数据同步增加隐私密码校验，提前识别密码缺失或不匹配
- 增强同步设置和操作提示，降低配置与同步流程的理解成本
- 在 SFTP 底部状态栏展示最近同步状态，并完善相关可见性设置

### v1.1.4
- 增加同步上传前置检查和操作引导，降低错误配置导致的上传失败
- 改进 WebDAV 连接验证、配置处理和错误提示
- 完善同步相关中英文文案
- 优化跨平台发布流水线并自动生成发布说明

### v1.1.3
- 修复 Windows 发布流水线解析 `cargo pkgid` 版本失败的问题
- 修复发布标签与应用内部版本不一致导致的在线更新循环
- 发布前强制校验 Git 标签与 `Cargo.toml` 版本一致
- 在线更新下载增加 SHA-256 完整性校验

### v1.0.1
- 从 ashell 正式更名为 TinyShell，仓库迁移至 [ynx-official/tiny-shell](https://github.com/ynx-official/tiny-shell)
- 新增跨平台自动更新功能，支持 Linux / macOS / Windows
- 新增中英文自动更新相关 UI 提示
- 更新所有品牌资源：图标、桌面入口、配置目录、环境变量

### v0.4.x
- 可视化快捷键管理与冲突检测
- Tab 内多 Pane 分屏（类 tmux 体验）
- SFTP 传输历史增强
- SSH 私钥 passphrase 支持
- Block Elements 等终端图形字符完整渲染

### v0.3.x
- 全局字体与字号可调
- SFTP 并发传输
- 布局持久化记忆
- 中英文热切换
- 终端右键复制粘贴

## 技术栈

| 组件 | 技术 |
|------|------|
| GUI 框架 | [GPUI](https://github.com/zed-industries/zed) |
| 组件库 | [gpui-component](https://github.com/longbridge/gpui-component) |
| 终端引擎 | [alacritty_terminal](https://github.com/alacritty/alacritty) |
| SSH 协议 | [russh](https://github.com/warp-tech/russh) |
| 国际化 | [rust-i18n](https://github.com/longbridge/rust-i18n) |
| 系统信息 | [sysinfo](https://github.com/GuillaumeGomez/sysinfo) |

## 协议

本项目基于 [GPL-3.0-or-later](LICENSE) 协议开源。
