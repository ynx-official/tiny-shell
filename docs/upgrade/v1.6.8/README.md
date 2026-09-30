# TinyShell v1.6.8

> 发布日期：2026-09-30
> 状态：Approved（四个平台构建与正式发布均成功）
> 最后更新：2026-09-30
> 关联文档：[失败的 v1.6.7 发布记录](../v1.6.7/README.md)、[快速连接浮层设计](../../02-design/quick-connect-popover.md)

## 版本概述

本次重新发布 v1.6.6 中的快速连接浮层改进，并修正 v1.6.7 漏改应用包版本造成的发布阻塞。应用包、锁文件、升级文档与新标签统一使用 1.6.8。保留关闭 Thin-LTO、使用 16 个代码生成单元的配置；本次 Windows 编译成功，所有平台的安装包与便携产物已完成正式交付。

## 改进与修复

- 修正标签与应用包版本不一致导致的发布资料校验失败。
- 包含 v1.6.6 的快速连接浮层自适应宽度与高度、名称 / 地址 / 用户三列展示、IPv6 地址与完整信息提示。
- 保留 `opt-level = 3`、符号裁剪与 `panic = "abort"`，同时使用关闭 Thin-LTO 的 release 配置。该配置作用于 Windows、macOS 与 Linux，可能影响产物大小和优化效果，尚无性能比较数据。

## 行为与界面变化

相对于最近成功交付的 v1.6.5，快速连接列表在窗口空间足够时展示更多内容，展开和折叠分组时自动调整高度；仅列表区域滚动，搜索、列标题和底栏保持独立。相对于 v1.6.6 标签源码，本次没有进一步修改交互代码。

## 配置与数据兼容性

没有修改配置格式、存储路径、同步协议、加密数据或会话结构，无需迁移。可回退至最近成功发布的 v1.6.5。

## 升级说明

可从 [v1.6.8 正式 Release](https://github.com/ynx-official/tiny-shell/releases/tag/v1.6.8) 下载并直接覆盖安装。macOS 用户继续选择原有基础版或 RDP 版，Windows 用户可选择安装包或便携包，Linux 用户可选择 AppImage 或压缩包；无需额外配置操作。

## 破坏性变更与已知问题

没有数据或 API 的破坏性变化。v1.6.6 的 Windows 编译器崩溃原因尚未完全确定，v1.6.7 在编译开始前因版本不一致失败。本次四个平台的真实编译、打包与正式发布均成功，Windows 未复现此前崩溃；这证明本次构建能够交付，不等于已确定编译器崩溃的底层根因。此前两个失败标签未复用。

## 验证结果

- 已从 v1.6.7 日志和本地发布脚本复现版本不一致错误。
- 保留用户要求，跳过本地测试、Clippy、Rust 格式检查和 release 构建。
- `cargo metadata --locked --offline --no-deps --format-version 1`：确认应用包版本为 `1.6.8`；独立核对 `Cargo.lock` 的 `tiny-shell` 版本为 `1.6.8`，没有更改依赖解析。
- `python scripts/release_notes.py --check-current` 和 `python scripts/release_notes.py --tag v1.6.8`：发布资料校验通过。
- `git diff --check`：发布文件未发现空白错误。
- [v1.6.8 标签流水线](https://github.com/ynx-official/tiny-shell/actions/runs/36725573805) 已通过发布资料校验。
- Windows：`cargo build --locked --release --target x86_64-pc-windows-msvc --no-default-features`、安装包和便携包打包、产物上传全部成功，本次未出现访问冲突崩溃。
- Linux：FreeRDP release 构建、AppImage 与压缩包打包、产物上传全部成功。
- macOS Apple Silicon：基础版与 RDP 版 release 构建、安装包与便携包打包、产物上传全部成功。
- macOS Intel：基础版与 RDP 版 release 构建、安装包与便携包打包、产物上传全部成功。
- `Publish Release`：生成更新清单并创建非草稿、非预发布的正式 Release 成功。
- 已核对更新清单的 `v1.6.8` 版本和 12 个平台产物；其文件名、大小、下载地址及 SHA-256 均与 GitHub Release 资产记录一致。Release 共包含 14 个文件（12 个平台产物、更新清单、发布说明）。
- 本地未验证 macOS/Linux 原生界面、远程连接或运行性能。

## 变更依据

- 正式标签：[`v1.6.8`](https://github.com/ynx-official/tiny-shell/releases/tag/v1.6.8)，应用源码提交为 `bee554d1f83f11dfe7cceacbbeb3df1cdcd03de3`。
- 最近祖先标签：`v1.6.7`（资料校验失败）；本版本之前最近成功发布版本：`v1.6.5`。
- 比较链接：[v1.6.7...v1.6.8](https://github.com/ynx-official/tiny-shell/compare/v1.6.7...v1.6.8)、[v1.6.5...v1.6.8](https://github.com/ynx-official/tiny-shell/compare/v1.6.5...v1.6.8)。

[返回版本总览](../README.md)
