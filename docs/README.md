# TinyShell 文档索引

> 状态：Approved
> 最后更新：2026-10-12

## 设计

- [内置 AI 会话](02-design/ai-conversations.md)：原生聊天、工具步骤、本机加密历史、模型配置及按来源独立的 SSH 权限。
- [内置 AI 需求与验收](01-requirements/ai-conversations.md)：已确认范围及 AI-01 至 AI-15 验收行为。

- [应用图标与平台导出](02-design/application-icons.md)：已选定的 A 圆润终端图标、平台适配与验证。

- [软件图标候选方案](02-design/application-icon-candidates.md)：三套方案、小尺寸对比与 A 方案选择记录。

- [主工作区与常用命令布局](02-design/workspace-ui/design.md)

- [快速连接浮层](02-design/quick-connect-popover.md)

- [统一确认弹窗与接入方式](02-design/confirmation-dialogs.md)

- [标签栏连接编辑](02-design/tab-connection-edit.md)

- [SSH 私钥导入交互](02-design/ssh-key-import.md)

- [终端内容高亮方案](02-design/terminal-content-highlighting.md)

- [SSH 常用命令补全与使用频次](02-design/terminal-command-completion.md)

- [MCP 功能设计](02-design/mcp-integration.md)：SSH 命令与远程文本操作、三种权限模式、本机桥接和任务反馈，已批准实施基线。

- [MCP 设置交互](02-design/mcp-settings-interaction.md)：外部 MCP 的连接授权搜索、客户端配置和任务审阅基线；新增 AI 全局权限 Tab 见内置 AI 交互。

## 架构

- [内置 AI 实现契约](03-architecture/ai-conversations.md)：模块、能力、数据、协议、取消和恢复。
- [内置 AI 安全边界](03-architecture/ai-threat-model.md)：模型、Provider、SSH、凭据与历史的信任边界。

- [MCP 开发契约](03-architecture/mcp-contracts.md)：工具、IPC、存储、状态机、资源限制和平台入口。

## 开发与运维

- [构建与打包](05-operations/build-and-package.md)：源码运行、平台依赖、FreeRDP 选项、质量检查与安装包生成。

## 质量验证

- [AI 执行预算与系统通知](04-quality/ai-execution-budget.md)：可配置预算、可恢复暂停、原生通知、回归用例和本轮未运行范围。

- [AI 与 MCP 调用链优化](04-quality/ai-tool-call-optimization.md)：工具裁剪、通知等待、应用管理会话、预算和重复控制的实际证据及手动验收。

- [内置 AI 实施与验证](04-quality/ai-conversations.md)：回归、模型/SSH替身闭环、系统凭据和原生验收状态。

- [Rust 1.96.1 升级验证](04-quality/rust-1-96-1-validation.md)：固定工具链、全目标测试、实际覆盖率与平台验证范围。

- [MCP 实施与验收](04-quality/mcp-implementation.md)：实施进度与实际测试证据。

- [MCP 设置交互验证](04-quality/mcp-settings-interaction.md)：本轮失败回归、质量检查、配置适配与未完成的原生验收。

- [MCP 真实应用基础冒烟](04-quality/mcp-live-smoke.md)：完全访问模式下真实 SSH/SFTP、版本冲突和清理结果，以及未验证范围。

- [MCP 源码运行桥接回归](04-quality/mcp-cargo-run-bridge.md)：普通 cargo run 缺少独立 helper 时的配置导出与主程序 stdio 入口验证。

## 其他现有文档

- [优化路线图](optimization-roadmap.md)
- [版本升级记录](upgrade/README.md)
- `bug/`：问题分析与修复记录
- `sync/`：同步功能设计与说明
