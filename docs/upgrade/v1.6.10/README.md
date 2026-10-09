# TinyShell v1.6.10

> 发布日期：2026-10-09
> 状态：Approved（四个平台构建、打包与正式发布均成功）
> 正式发布时间：2026-10-09 11:08（Asia/Shanghai）
> 最后更新：2026-10-09
> 发布测试策略：skip
> 关联文档：[应用图标与平台导出](../../02-design/application-icons.md)、[v1.6.9](../v1.6.9/README.md)

## 版本概述

本版本将 TinyShell 的三平台软件图标统一为用户确认的简洁黑白终端窗口：只有白色窗口轮廓与 `>_`，不带应用名称、SSH 徽章或代码内容。同步主窗口、辅助窗口、Windows 安装器、macOS App Bundle、Linux PNG 和 README 标识，并改善 16px 图标的清晰度。本次没有修改连接、终端、SFTP、同步或更新业务逻辑。

## 改进与修复

- 用黑白扁平终端标识替换 v1.6.9 的深蓝、青绿及品牌文字图标，移除名称、徽章、服务器、连接虚线、彩色控制点、渐变和立体装饰。
- Windows/Linux 从同一权威母图等比例导出，继续保持紧凑留白及透明四角，避免软件图标偏小。
- macOS 导出至不透明满画布纯黑背景，由系统处理外层遮罩；保留正确的 ICNS 小尺寸 ARGB 与较大尺寸 PNG 编码。
- 16px 导出增加 1.2 倍灰阶对比补偿，改善缩小后的细线灰化；清除 Windows/Linux 最外层四角的半透明采样残留，不改变母图造型。

## 工程与发布

- 两个平台源图直接使用用户最后确认的生成 PNG；导出脚本统一为中性灰阶，不再次绘制图形。
- 保留现有尺寸、透明性和编码检查，增加灰阶、对比度、Linux PNG 及底部名称回归检查。
- 本版本按用户明确要求跳过发布阶段测试。沿用既有单版本策略，未修改后续版本的默认测试行为；版本资料校验、四个平台构建打包及更新清单生成继续执行。

## 行为与界面变化

应用与安装器图标变为无文字的黑白终端窗口，便于识别终端工具。仅更换应用品牌素材，不修改软件内部功能按钮图标、Sentry UI 主题、菜单、快捷键、窗口流程或业务功能。

## 配置与数据兼容性

配置格式、存储路径、连接会话、同步协议、加密数据、认证权限和更新清单协议均保持兼容，无数据迁移。可以覆盖安装，也可以回退至 v1.6.9；回退不会自动刷新操作系统的图标缓存。

## 升级说明

直接覆盖安装并完全退出旧进程后重启。Windows 继续提供安装包和便携包；macOS Apple Silicon/Intel 继续提供基础版及 RDP 版的安装包和便携包；Linux 继续提供 AppImage 与压缩包。

若仍显示旧图标，确认快捷方式指向新版本，再重新固定任务栏或 Dock 图标。无需删除应用配置或会话来刷新图标。macOS 系统正常的材质高光仍可能存在，不代表应用再次添加了外壳。

## 破坏性变更与已知问题

没有数据、配置或 API 的破坏性变化。macOS 12–15 可能保留满画布素材的方形外形，代理未在这些系统及 macOS 26+ 实机验证；本轮沿用此前针对用户 macOS 26+ 的满画布规则。多平台构建成功不等于验证了各系统桌面的原生图标显示或运行表现。

## 验证结果

- 图标落地阶段（版本仍为 1.6.9）：六项图标检查通过；底部名称回归先失败后通过，16px 对比度与角像素问题由现有断言发现后修正，未弱化测试。
- 该阶段 `cargo fmt --all -- --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked --all-targets` 通过（532 项通过、1 项既有手动性能基准忽略），Windows debug/release 构建通过。
- 该阶段两种 Windows EXE 的资源 ID 1 在 16/32/48/64/256px 与项目 ICO 原生解码像素差异均为 0；四个消费素材重复导出的 SHA-256 一致。
- 正式发布阶段按用户要求不重跑本地测试、Clippy、格式检查或发布工具测试。以上升级前结果不作为 v1.6.10 的测试记录。
- `cargo check`、`cargo check --locked`：均通过；Cargo.lock 仅更新 TinyShell 包版本，没有更改依赖解析。
- `python scripts/release_notes.py --check-current` 与 `python scripts/release_notes.py --tag v1.6.10`：发布资料校验通过，发布说明已生成；`git diff --check` 通过。
- [v1.6.10 标签流水线](https://github.com/ynx-official/tiny-shell/actions/runs/37876125444)：发布资料校验通过，`Test release tooling` 按本版本策略跳过。
- Windows x86_64：release 构建、安装包与便携包打包、产物上传全部成功。
- Linux x86_64：FreeRDP release 构建、AppImage 与压缩包打包、产物上传全部成功。
- macOS Apple Silicon 与 Intel：各自基础版及 RDP 版 release 构建、安装包与便携包打包、产物上传全部成功。
- `Publish Release`：更新清单生成和非草稿、非预发布的正式 Release 创建成功；GitHub 最新正式版本为 `v1.6.10`。
- 已核对更新清单的 `v1.6.10` 版本及 12 个平台产物；文件名、大小、下载地址和 SHA-256 均与 GitHub Release 资产记录一致，下载的清单本身 SHA-256 也一致。Release 共包含 14 个文件（12 个平台产物、更新清单、发布说明）。
- 已下载并解包 Windows 工作流产物，包内 EXE 的 `ProductVersion` 为 `1.6.10`；16/32/48/64/256px 资源 ID 1 与新 ICO 的原生像素差异均为 0。核对后确认这两个 Windows 工作流文件的 SHA-256 与正式 Release 的对应文件完全一致，没有运行应用或测试套件。
- 代理未在 macOS/Linux 实机运行应用；常规 macOS 原生图标解码 CI 尚未运行。

## 变更依据

- 正式标签：[`v1.6.10`](https://github.com/ynx-official/tiny-shell/releases/tag/v1.6.10)，应用源码提交为 `7dc5c0dd88e56251c1f427d4af467f2ffac496f2`；已创建并推送，没有复用已发布标签。
- 最近祖先发布标签与最近成功版本：[`v1.6.9`](https://github.com/ynx-official/tiny-shell/releases/tag/v1.6.9)。
- 代码差异：[v1.6.9...v1.6.10](https://github.com/ynx-official/tiny-shell/compare/v1.6.9...v1.6.10)。
- 图标变更提交：`edb72a0`；相对 v1.6.9 的额外历史提交仅补记上次发布结果。

[返回版本总览](../README.md)
