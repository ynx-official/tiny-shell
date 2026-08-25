# TinyShell v1.5.10

> 发布日期：2026-08-18

## 版本概述

本版本修复 `v1.5.9` 在 macOS/Linux 启用 FreeRDP 后的 release 构建错误。修复后，RDP 工作线程使用独立的 `Arc` 克隆，主线程仍可安全持有鼠标移动状态，不改变运行时协议或用户数据。

## 修复

- 修复 `src/backend/remote_desktop.rs` 中 `mouse_move` 被 `spawn_blocking` 闭包移动后再次使用的 Rust `E0382` 编译错误。
- 统一 FreeRDP 后端工作线程与返回给终端后端的共享状态所有权，允许 Linux、macOS release 构建继续编译。
- 修正 `v1.5.9` 发布详情中的标签元数据格式。

## 行为与界面变化

- 无用户界面和操作流程变化。
- RDP 的 Windows `mstsc.exe` 路径、macOS/Linux FreeRDP 动态分辨率、双向文本剪贴板和证书流程保持不变。

## 配置与数据兼容性

- 配置、会话、v3 同步、托管密钥、SFTP 传输和 RDP 会话数据格式保持兼容，无需迁移。

## 升级说明

Windows、macOS 和 Linux 用户可以直接覆盖安装。此次修复主要影响 macOS/Linux FreeRDP release 构建；Windows 原生 RDP 行为不变。

## 破坏性变更与已知问题

- 无配置、协议或数据格式破坏性变更。
- 真实 macOS/Linux RDP 主机验收仍由发布流水线和后续手工验证完成。
- 按用户要求跳过测试、Clippy 和本地 release 构建。

## 验证结果

- `cargo check --locked`：通过。
- `python scripts/release_notes.py --check-current`：通过。
- `git diff --check`：通过。
- `cargo fmt --all -- --check`：按用户要求跳过。
- `cargo clippy --locked --all-targets -- -D warnings`：按用户要求跳过。
- `cargo test --locked --all-targets`：按用户要求跳过。
- `cargo build --locked --release`：按用户要求跳过；当前 Windows 环境无法复现 macOS/Linux FreeRDP 特性构建。

## 变更依据

- 目标标签：尚未创建 `v1.5.10`
- 最近正式标签：`v1.5.9`
- Compare：发布后可查看 [v1.5.9...v1.5.10](https://github.com/ynx-official/tiny-shell/compare/v1.5.9...v1.5.10)

[返回版本总览](../README.md)
