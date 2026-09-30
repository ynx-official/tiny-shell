# TinyShell v1.6.7

> 发布日期：2026-09-30

## 版本概述

本版本修复 v1.6.6 Windows 正式构建失败问题。GitHub Actions 日志显示，Windows release 编译阶段的 `rustc.exe` 因 `STATUS_ACCESS_VIOLATION (0xc0000005)` 异常退出；本版本调整 release 编译参数，降低托管构建环境触发编译器崩溃的风险。

## 改进与修复

- 关闭 release profile 的 Thin-LTO，并将代码生成单元调整为 16，避免 Windows hosted runner 上的编译器访问冲突。
- 保留 release 优化、符号裁剪和 abort panic 行为。

## 行为与界面变化

应用运行时界面和用户工作流没有变化。本版本主要修复正式构建和产物交付链路。

## 配置与数据兼容性

配置、会话、同步协议和工作区数据格式保持兼容，无需迁移。

## 升级说明

可直接覆盖安装。v1.6.6 未完成完整 Windows 产物发布，建议使用本版本产物。

## 破坏性变更与已知问题

未发现破坏性变更。v1.6.6 的失败原因为 Windows hosted runner 上的 Rust 编译器访问冲突；本版本的跨平台产物仍由新标签流水线验证。

## 验证结果

- 已核对 v1.6.6 GitHub Actions：验证、Linux 和 macOS 构建成功，Windows release 编译以 `STATUS_ACCESS_VIOLATION (0xc0000005)` 失败。
- 按用户要求跳过本地测试、Clippy、格式检查和 release 构建。
- 已更新版本号、变更日志、升级总览和版本详情；`Cargo.lock` 无依赖或包版本变化。

## 变更依据

- 目标标签：`v1.6.7`。
- 最近祖先版本：`v1.6.6`。
- 比较链接：[v1.6.6...v1.6.7](https://github.com/ynx-official/tiny-shell/compare/v1.6.6...v1.6.7)

[返回版本总览](../README.md)
