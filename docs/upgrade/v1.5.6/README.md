# TinyShell v1.5.6

> 发布日期：2026-08-15

## 版本概述

本版本修复 v1.5.5 正式安装包未启用 FreeRDP 的发布配置问题。Windows、macOS 和 Linux 的标签构建现在统一启用 `freerdp` 特性，并将对应平台需要的 FreeRDP 与 WinPR 运行库纳入发布产物，使安装包中的 RDP 会话可以实际连接远程桌面服务。

## 改进与修复

- GitHub Release 的四个平台构建统一使用 `cargo build --locked --release --features freerdp`。
- Windows 使用 vcpkg 安装 FreeRDP 3 客户端组件，编译时接入头文件和导入库，并将运行 DLL 同时复制到便携包与 Inno Setup 安装目录。
- macOS Apple Silicon 与 Intel 分别在原生架构 runner 上安装 Homebrew FreeRDP，使用 `dylibbundler` 将 FreeRDP、WinPR 及非系统依赖递归嵌入 `TinyShell.app/Contents/Frameworks`，重写动态库加载路径后再统一签名。
- Linux 使用 Ubuntu 24.04 提供的 FreeRDP 3 开发包构建，将链接到的 FreeRDP 与 WinPR 共享库放入归档的 `lib` 目录，并将可执行文件运行路径设置为相对该目录。
- 构建脚本新增多头文件目录支持，兼容 FreeRDP 与 WinPR 分离的系统、Homebrew 和 vcpkg 安装布局。
- Linux 和 Windows 打包步骤会拒绝缺少 FreeRDP/WinPR 运行库的产物，避免再次发布只有界面而没有原生后端的安装包。

## 行为与界面变化

- 官方 v1.5.6 安装包中的 RDP 会话不再显示“FreeRDP 后端尚未接入”，可以进入证书确认、连接、画面显示和输入流程。
- RDP 功能行为与 v1.5.5 保持一致，包括本次信任/进程内始终信任证书、基础键鼠输入、本地文本粘贴和最多三次断线重试。
- SSH、SFTP、本地终端和配置同步界面没有行为变化。

## 配置与数据兼容性

- 配置、会话、v3 同步、托管密钥和 SFTP 传输数据格式与 v1.5.5 完全一致，不需要迁移。
- v1.5.5 已创建的 RDP 会话可直接由 v1.5.6 使用。
- 可回退到 v1.5.5，但该版本的官方安装包没有 FreeRDP 原生后端，RDP 会话无法连接；回退到 v1.5.3 前仍应备份或移除 RDP 会话。

## 升级说明

Windows、macOS 和 Linux 用户可以直接覆盖安装。升级后请完全退出 v1.5.5 进程并重新启动 TinyShell，确保加载 v1.5.6 随包提供的 FreeRDP 运行库。

Linux 发布 runner 从 Ubuntu 22.04 调整为 Ubuntu 24.04，以使用系统 FreeRDP 3.x 开发包。Linux 用户需要具备与 Ubuntu 24.04 相当的 glibc 运行环境；较旧发行版如无法启动，应暂时继续使用 v1.5.5 的 SSH/SFTP 功能或从源码在目标发行版构建启用 `freerdp` 的版本。

## 破坏性变更与已知问题

- 无配置或数据格式破坏性变更。
- Linux 二进制的最低 glibc 环境随构建 runner 提升，旧 Linux 发行版兼容性低于 v1.5.5。
- RDP 服务端到本地剪贴板同步、DISP 动态分辨率和持久 GPU 纹理原位更新仍未实现；当前仅支持将本地文本粘贴到远端。
- 证书“始终信任”仍只在当前进程内有效，应用重启后需要重新确认。
- 按发布要求未执行本轮本地测试、静态检查、release 构建或 Windows/macOS/Linux 手工 RDP 验收。GitHub 标签发布流程会执行发布资料校验、默认发布工具测试以及启用 `freerdp` 的多平台 release 构建和运行库打包检查。

## 验证结果

- `cargo check --locked`：按发布要求跳过。
- `cargo fmt --all -- --check`：按发布要求跳过。
- `cargo clippy --locked --all-targets -- -D warnings`：按发布要求跳过。
- `cargo test --locked --all-targets`：按发布要求跳过。
- 本地 `cargo build --locked --release --features freerdp`：按发布要求跳过。
- Windows、macOS、Linux 手工 RDP 验收：按发布要求跳过。
- GitHub Actions 工作流 YAML 语法检查：通过。
- `python3 scripts/release_notes.py --check-current`：已执行，用于校验版本号与发布资料一致性。

## 变更依据

- 目标标签：尚未创建 `v1.5.6`
- 最近祖先版本：`v1.5.5`
- Compare：发布后可查看 [v1.5.5...v1.5.6](https://github.com/ynx-official/tiny-shell/compare/v1.5.5...v1.5.6)

[返回版本总览](../README.md)
