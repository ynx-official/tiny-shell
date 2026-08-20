# TinyShell v1.5.6

> 发布日期：2026-08-15

## 版本概述

本版本尝试修复 v1.5.5 正式安装包未启用 FreeRDP 的发布配置问题，但标签流水线在 Windows、Linux 和 macOS 的 RDP 编译阶段失败，最终发布步骤被跳过，没有生成 GitHub Release 或可安装产物。后续修复改由 v1.5.7 发布。

## 改进与修复

- 标签构建统一添加 `--features freerdp`。
- Windows 接入 vcpkg FreeRDP 3，并准备将运行 DLL 加入便携包和安装器。
- macOS 接入 Homebrew FreeRDP 与 `dylibbundler`，准备将非系统动态依赖嵌入应用包。
- Linux 改用 Ubuntu 24.04 FreeRDP 3 开发包，并准备将 FreeRDP/WinPR 共享库加入归档。
- 构建脚本新增多个 FreeRDP/WinPR 头文件目录支持。

## 行为与界面变化

- 本版本没有成功生成安装包，因此用户端行为未发生变化。

## 配置与数据兼容性

- 源码中的配置、会话、v3 同步、托管密钥和 SFTP 传输数据格式与 v1.5.5 一致。
- 由于没有发布产物，不需要从 v1.5.5 升级或回退。

## 升级说明

请勿使用 v1.5.6 标签自行替代稳定安装包；需要内置 FreeRDP 的正式产物请升级到后续修复版本。

## 破坏性变更与已知问题

- 启用 `freerdp` 后，`src/backend/freerdp.rs` 从错误的父模块导入远程桌面类型，导致 Rust 编译失败。
- Linux 和 macOS 原生桥接缺少 `winpr/interlocked.h`，导致 WinPR 原子操作未声明；Linux 仅产生警告，macOS C 编译器将其视为错误。
- Linux 构建基线计划提升到 Ubuntu 24.04，后续成功发布版本的最低 glibc 兼容性会低于 v1.5.5。

## 验证结果

- 本地测试、Clippy、格式检查和 release 构建：按发布要求跳过。
- GitHub 发布资料校验：通过。
- Linux `--features freerdp` release 构建：失败，Rust 模块导入错误。
- macOS Apple Silicon `--features freerdp` release 构建：失败，WinPR 原子操作未声明。
- macOS Intel `--features freerdp` release 构建：失败，WinPR 原子操作未声明。
- Windows vcpkg FreeRDP 3.30.0 安装：通过。
- Windows `--features freerdp` release 构建：失败，Rust 模块导入错误。
- GitHub Release 发布：跳过，未生成产物。

## 变更依据

- 失败标签：`v1.5.6`
- 最近祖先版本：`v1.5.5`
- Compare：[v1.5.5...v1.5.6](https://github.com/ynx-official/tiny-shell/compare/v1.5.5...v1.5.6)
- 失败流水线：[Release 31871217849](https://github.com/ynx-official/tiny-shell/actions/runs/31871217849)

[返回版本总览](../README.md)
