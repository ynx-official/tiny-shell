# TinyShell v1.5.8

> 发布日期：2026-08-17

## 版本概述

本版本重新发布当前最新的 FreeRDP/RDP 实现，确保正式标签与实际代码、安装包和更新清单一致。RDP 动态分辨率、双向 Unicode 文本剪贴板、四平台依赖打包和 Linux AppImage 能力保持完整。

## 发布修复

- 将最新 RDP 动态分辨率和双向文本剪贴板实现纳入正式发布版本。
- 保留 Windows FreeRDP DLL、macOS 内嵌动态库和 Linux FreeRDP/WinPR 运行库打包门禁。
- 保留 Linux x86_64 AppImage、`.tar.gz` 归档、AppImage 更新替换和 SHA-256 更新清单支持。
- 修正版本标签与实际发布提交的对应关系；旧的远程 `v1.5.7` 标签不被覆盖。

## 行为与界面变化

- RDP 窗口尺寸变化会经过节流后提交到 FreeRDP DISP 通道，远端桌面随窗口调整。
- 本地复制内容可提供给远端，远端复制的 Unicode 文本可回写到 TinyShell 本地剪贴板。
- Linux 用户可选择 AppImage 或 `.tar.gz`；AppImage 更新时会原子替换外层文件并从新路径重启。

## 配置与数据兼容性

- 配置、会话、v3 同步、托管密钥、SFTP 传输和 RDP 会话数据格式与 `v1.5.7` 保持兼容。
- 无需迁移；回退到旧版本不会改变配置数据，但旧安装包可能不包含完整 FreeRDP 原生运行库。

## 升级说明

Windows、macOS 和 Linux 用户可以直接覆盖安装。Linux 用户首次运行 AppImage 前需执行 `chmod +x`；若系统缺少 FUSE 2 兼容层，可使用发行版软件包或 `.tar.gz` 归档。请完全退出旧进程后再启动新版本，确保加载新安装目录中的 FreeRDP 运行库。

## 破坏性变更与已知问题

- 无配置或数据格式破坏性变更。
- Linux 构建基线为 Ubuntu 24.04，较旧发行版可能不满足 glibc 运行要求。
- AppImage 不捆绑 glibc、显卡驱动或 Mesa/Vulkan 等主机图形栈。
- 持久 GPU 纹理原位更新尚未实现；证书“始终信任”仍仅在当前进程内有效。
- 按用户要求跳过测试、静态检查和本地 release 构建；未执行真实 RDP 主机手工验收。

## 验证结果

- `cargo check --locked`：通过；Windows 构建存在未使用 `current_exe` 的 warning，按要求未执行 Clippy。
- `python scripts/release_notes.py --check-current`：通过。
- `git diff --check`：通过。
- `cargo fmt --all -- --check`：按用户要求跳过。
- `cargo clippy --locked --all-targets -- -D warnings`：按用户要求跳过。
- `cargo test --locked --all-targets`：按用户要求跳过。
- `cargo build --locked --release`：按用户要求跳过。
- 四平台构建、打包和真实 RDP 验收：由推送标签后的 GitHub Actions 执行。

## 变更依据

- 目标标签：尚未创建 `v1.5.8`
- 最近正式标签：`v1.5.7`（远程标签保留旧发布提交）
- Compare：发布后可查看 [v1.5.7...v1.5.8](https://github.com/ynx-official/tiny-shell/compare/v1.5.7...v1.5.8)

[返回版本总览](../README.md)
