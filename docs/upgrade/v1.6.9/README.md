# TinyShell v1.6.9

> 发布日期：2026-10-08
> 状态：Approved（已批准正式发布，构建与发布结果待流水线确认）
> 日期说明：以上为本次计划发布日期，正式交付后更新发布状态。
> 最后更新：2026-10-08
> 发布测试策略：skip
> 关联文档：[应用图标与平台导出](../../02-design/application-icons.md)、[v1.6.8](../v1.6.8/README.md)

## 版本概述

本版本针对 Windows 图标偏小、macOS Tahoe 26+ 图标外框叠加的反馈，分别调整平台素材。保留 TinyShell 的深蓝、青绿、白色调及 `TinyShell`、`SSH` 文字，优化系统图标的可见尺寸和外形适配。没有修改终端、连接、配置或同步业务逻辑。

## 改进与修复

- Windows 图标裁去多余透明留白并等比例居中；32px 图标可见主体从约 25×27px 增至 29×31px，16px 从 12×14px 增至 14×15px。窗口、任务栏与安装器继续使用原资源路径。
- macOS 改用独立满画布深蓝素材，移除最外层预绘制圆角外壳与描边，由 26+ 系统处理最终外形和材质，减少素材与系统外框重复叠加。
- 修正 ICNS 16/32px 普通尺寸为 ARGB 编码，较大与 Retina 尺寸保持 PNG，避免将 PNG 放进小尺寸原生槽位。
- Linux 256px PNG 同步使用收紧留白后的品牌图，Debian/AppImage 的消费路径保持不变。

## 工程与发布

- 保留权威源图，新增可重复的图标生成脚本及素材内容回归测试。
- 常规 CI 增加 macOS `iconutil` 原生解码检查；该检查不等于已经验证 Dock 的实际外观。
- 发布工具测试允许按单个版本详情中的策略跳过。本版本按用户明确要求跳过；后续没有该标记的版本仍默认执行测试，版本资料校验、构建、打包与更新清单生成继续执行。

## 行为与界面变化

变化仅涉及软件图标，不修改快捷键、菜单、窗口流程或终端行为。Windows 需要完全退出旧进程后启动新版本，已固定的任务栏快捷方式可能需要重新固定以刷新显示。macOS 系统自身的材质高光仍可能存在，不代表 TinyShell 额外添加了外壳。

## 配置与数据兼容性

配置格式、存储路径、会话、同步协议、加密数据、认证权限与更新清单协议均保持兼容，无数据迁移。可以覆盖安装，也可以回退到 v1.6.8；回退不会自动恢复此前 Windows 图标缓存。

## 升级说明

直接覆盖安装并重启应用即可，无需额外配置或权限。Windows 继续提供安装包与便携包；macOS Apple Silicon/Intel 继续提供基础版与 RDP 版的安装包及便携包；Linux 继续提供 AppImage 与压缩包。

若覆盖安装后仍显示旧图标，先退出旧程序，确认快捷方式指向新版本，再重新固定任务栏或 Dock 图标。不要删除应用配置来刷新图标。

## 破坏性变更与已知问题

没有数据、配置或 API 的破坏性变化。macOS 26+ 的正常系统材质效果不会被本版本禁用；macOS 12–15 可能保留满画布素材的方形外形，代理未在这些系统上实机验证。用户已确认本轮图标优化效果，但这不替代多平台原生自动化测试。

## 验证结果

- 图标优化阶段（版本升级前）：`cargo fmt --all -- --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked --all-targets`、Windows debug/release 构建已通过；新图标四项回归测试先失败后通过。
- 从优化阶段 Windows release EXE 的资源 ID 1 读取 16/32/48/64px 图标，与项目 ICO 的像素差异均为 0。
- 图标生成脚本重复导出的四个消费文件 SHA-256 与项目素材一致。
- 正式发布阶段按用户要求跳过本地测试、Clippy、格式检查与发布工具测试，不将上一阶段的运行结果冒充 v1.6.9 的测试结果。
- `cargo check`、`cargo check --locked`：均通过，Cargo.lock 仅更新 TinyShell 包版本，没有更改依赖解析。
- `python scripts/release_notes.py --check-current` 与 `python scripts/release_notes.py --tag v1.6.9`：发布资料校验通过；发布说明已生成。
- `git diff --check`：通过。多平台构建、打包和正式 Release 结果，在执行后补记。
- 代理未在 macOS/Linux 实机运行应用；常规 macOS 原生图标 CI 检查尚未运行。

## 变更依据

- 目标正式标签：[`v1.6.9`](https://github.com/ynx-official/tiny-shell/releases/tag/v1.6.9)，准备阶段尚未创建或推送，不复用已发布标签。
- 最近祖先发布标签与最近成功版本：[`v1.6.8`](https://github.com/ynx-official/tiny-shell/releases/tag/v1.6.8)。
- 代码差异：[v1.6.8...v1.6.9](https://github.com/ynx-official/tiny-shell/compare/v1.6.8...v1.6.9)。
- 图标优化提交：`8332e31`；相对 v1.6.8 的额外历史提交仅补记上次发布结果。

[返回版本总览](../README.md)
