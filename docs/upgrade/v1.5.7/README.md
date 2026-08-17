# TinyShell v1.5.7

> 发布日期：2026-08-17

## 版本概述

本版本继续完成 v1.5.6 未能交付的 FreeRDP 正式打包：修复启用特性后暴露的 Rust 模块导入与 WinPR 原子头文件问题，并保留四个平台的 FreeRDP 依赖安装、链接和运行库打包门禁。正式产物中的 RDP 会话现在以启用原生后端为发布条件。

## 改进与修复

- `freerdp.rs` 直接从 `backend::remote_desktop` 导入证书决策、帧邮箱和画面尺寸类型，修复特性构建中的未解析导入。
- 原生 FreeRDP C 桥接显式包含 `winpr/interlocked.h`，确保 Linux 和 macOS 声明 `InterlockedExchange` 与 `InterlockedCompareExchange`。
- Windows 使用 vcpkg FreeRDP 3 客户端组件构建，便携包和 Inno Setup 安装器都会复制 vcpkg 运行 DLL。
- macOS Apple Silicon 与 Intel 在原生架构 runner 上安装 Homebrew FreeRDP，使用 `dylibbundler` 将非系统动态依赖嵌入 `TinyShell.app/Contents/Frameworks` 并重写加载路径。
- macOS 构建固定 14.0 部署目标，Apple Silicon runner 保持 macOS 14，避免 FreeRDP 修复无意提高到 macOS 15。
- Linux 使用 Ubuntu 24.04 FreeRDP 3 开发包，并将链接到的 FreeRDP/WinPR 共享库复制到归档 `lib` 目录，设置 `$ORIGIN/lib` 运行路径并检查未解析依赖。
- Linux 新增单文件 x86_64 AppImage，并继续保留 `.tar.gz`；打包脚本使用固定版本和 SHA-256 的 `linuxdeploy`，检查 FreeRDP/WinPR 运行库、ELF 依赖与可重定位 RPATH。
- Ubuntu 24.04 CI 会生成、解包并在 Xvfb 下限时启动 AppImage，发布工作流将 AppImage 纳入工作流产物、GitHub Release 和 SHA-256 更新清单。
- 构建脚本支持多个 FreeRDP/WinPR 头文件目录，兼容 apt、Homebrew 和 vcpkg 的分离头文件布局。
- RDP 支持远端桌面动态分辨率调整，窗口尺寸变化会经过节流后提交到 FreeRDP DISP 通道。
- RDP 支持 Unicode 文本双向剪贴板：本地复制内容可提供给远端，远端复制内容可回写到 TinyShell 本地剪贴板。

## 行为与界面变化

- 官方安装包中的 RDP 会话可以加载 FreeRDP 原生后端，进入证书确认、连接、画面显示与输入流程。
- Linux AppImage 可直接赋予执行权限后启动；应用内更新会识别 `$APPIMAGE`，下载匹配的 AppImage，原子替换外层文件并从该路径重启。
- RDP 行为保持 v1.5.5 的设计，并增加动态分辨率和双向文本剪贴板；仍支持基础键鼠输入、进程内证书指纹信任和最多三次断线重试。
- SSH、SFTP、本地终端和配置同步界面没有行为变化。

## 配置与数据兼容性

- 配置、会话、v3 同步、托管密钥和 SFTP 传输数据格式与 v1.5.5 一致，不需要迁移。
- v1.5.5 创建的 RDP 会话可直接使用；v1.5.6 没有生成安装包，因此不存在从该版本迁移的数据。
- 回退到 v1.5.5 后 RDP 会话仍会保留，但该版本官方安装包没有原生后端，无法实际连接。

## 升级说明

Windows、macOS 和 Linux 用户可以直接覆盖安装。请完全退出旧进程后启动 v1.5.7，确保加载新安装目录或应用包内的 FreeRDP 运行库。Linux 用户可选择 AppImage 或原有 `.tar.gz`；首次运行 AppImage 前需执行 `chmod +x`。

Linux 构建基线从 Ubuntu 22.04 提升到 Ubuntu 24.04。Linux 用户需要具备与 Ubuntu 24.04 相当的 glibc 运行环境；较旧发行版无法启动时，需要在目标系统从源码构建启用 `freerdp` 的版本。

## 破坏性变更与已知问题

- 无配置或数据格式破坏性变更。
- Linux 二进制最低 glibc 环境高于 v1.5.5，旧发行版兼容性降低。
- AppImage 不捆绑 glibc、显卡驱动或 Mesa/Vulkan 等主机图形栈，同样需要与 Ubuntu 24.04 相当的 glibc 环境；没有 FUSE 2 兼容层时可安装发行版对应软件包或使用 `.tar.gz`。
- 持久 GPU 纹理原位更新仍未实现；画面更新继续通过当前帧缓冲路径处理。
- 证书“始终信任”仅在当前进程内有效，应用重启后需要重新确认。
- 按发布要求未执行本轮本地测试、静态检查、release 构建或真实 RDP 主机手工验收。

## 验证结果

- `cargo check --locked`：按发布要求跳过。
- `cargo fmt --all -- --check`：按发布要求跳过。
- `cargo clippy --locked --all-targets -- -D warnings`：按发布要求跳过。
- `cargo test --locked --all-targets`：按发布要求跳过。
- 本地 `cargo build --locked --release --features freerdp`：按发布要求跳过。
- Windows、macOS、Linux 真实 RDP 手工验收：按发布要求跳过。
- GitHub Actions 工作流 YAML 语法检查：通过。
- v1.5.6 环境预验证：Ubuntu FreeRDP 3 安装、macOS Homebrew FreeRDP 安装及 Windows vcpkg FreeRDP 3.30.0 安装均成功；编译错误已在 v1.5.7 修复。
- `python scripts/release_notes.py --check-current`：通过，用于校验版本号与发布资料一致性。

## 变更依据

- 目标标签：尚未创建 `v1.5.7`
- 最近祖先版本：`v1.5.6`（构建失败，无发布产物）
- 最近可安装版本：`v1.5.5`
- Compare：[v1.5.6...v1.5.7](https://github.com/ynx-official/tiny-shell/compare/v1.5.6...v1.5.7)

[返回版本总览](../README.md)
