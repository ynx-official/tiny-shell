# TinyShell v1.6.7

> 发布日期：2026-09-30
> 状态：Archived（发布失败，保留标签记录）
> 最后更新：2026-09-30
> 关联版本：[v1.6.8](../v1.6.8/README.md)

## 发布结果更正

本标签未完成正式发布。其提交中的 `Cargo.toml` 和 `Cargo.lock` 仍为 `1.6.6`，与标签 `v1.6.7` 不一致；[流水线](https://github.com/ynx-official/tiny-shell/actions/runs/36715775096) 在发布资料校验时失败，所有平台编译和发布步骤均被跳过。以下是当时的构建参数调整记录，不代表已交付或已验证的修复。

## 版本概述

本次尝试针对 v1.6.6 Windows 正式构建失败调整 release 编译参数。该版本日志显示 `rustc.exe` 因 `STATUS_ACCESS_VIOLATION (0xc0000005)` 异常退出，但未提供足以证明 Thin-LTO 为根因的诊断；调整效果需要后续成功编译验证。

## 改进与修复

- 关闭 release profile 的 Thin-LTO，并将代码生成单元调整为 16，作为编译器访问冲突的尝试性规避。
- 保留 release 优化、符号裁剪和 abort panic 行为。

## 行为与界面变化

应用运行时界面和用户工作流没有变化。本版本主要修复正式构建和产物交付链路。

## 配置与数据兼容性

配置、会话、同步协议和工作区数据格式保持兼容，无需迁移。

## 升级说明

本标签没有可供安装的正式产物。已有 v1.6.5 用户应等待后续成功发布的版本。

## 破坏性变更与已知问题

未发现破坏性变更。v1.6.6 的失败原因为 Windows hosted runner 上的 Rust 编译器访问冲突；本版本的跨平台产物仍由新标签流水线验证。

## 验证结果

- 已核对 v1.6.6 GitHub Actions：验证、Linux 和 macOS 构建成功，Windows release 编译以 `STATUS_ACCESS_VIOLATION (0xc0000005)` 失败。
- 按用户要求跳过本地测试、Clippy、格式检查和 release 构建。
- 发布日志确认标签 `v1.6.7` 与包版本 `1.6.6` 不一致，发布资料校验失败。此前关于“已更新版本号”的说明有误；`Cargo.toml` 和 `Cargo.lock` 的应用版本均漏改。
- 此次未执行任何平台编译，关闭 Thin-LTO 的效果未获得验证。

## 变更依据

- 目标标签：`v1.6.7`。
- 最近祖先版本：`v1.6.6`。
- 比较链接：[v1.6.6...v1.6.7](https://github.com/ynx-official/tiny-shell/compare/v1.6.6...v1.6.7)

[返回版本总览](../README.md)
