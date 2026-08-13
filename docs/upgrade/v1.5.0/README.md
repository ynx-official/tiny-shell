# TinyShell v1.5.0

> 发布日期：2026-03-14

## 版本概述

本版本将配置同步协议正式切换到 v3，使用稳定实体 ID、实体版本和墓碑记录协调多设备上的新增、修改、删除与恢复。同步 baseline 现在保存完整的加密 v3 远端快照，且同步与 SFTP 的失败、取消和任务生命周期反馈更加可靠。

## 新功能

- 新增唯一正式 v3 同步协议，覆盖 sessions、managed keys、connection groups、quick commands 和删除快照。
- 新增按同步目标隔离的加密 v3 baseline，保存远端 payload、revision、ETag 和同步时间。
- 新增基于 generation、更新时间和 device ID 的确定性实体版本仲裁。

## 改进与修复

- WebDAV、S3、ETag 条件写入、隐私密码校验、加密 JSON 编解码和应用层合并统一使用 v3。
- 修复重复墓碑、同版本 active/tombstone 冲突、删除后恢复、连接组投影和本地版本推进问题。
- 修复过期同步任务结果覆盖当前任务状态以及错误推进窗口关闭流程的问题。
- 修复 SFTP 上传、下载和打包下载失败事件遗漏；普通 I/O、遍历和目录创建失败现在报告明确失败状态。
- 保持 FinalShell、归档导入、配置编辑、managed key、代理和快速命令功能可用，并以真实持久化成功作为导入完成条件。

## 行为与界面变化

- 同步失败、取消和上传/下载状态继续通过现有状态提示展示，但失败事件不再被错误归类为可恢复状态。
- 旧同步对象按不支持的协议版本处理，不再自动迁移或静默转换。

## 配置与数据兼容性

- 这是一次同步数据格式破坏性升级。v3 是唯一正式同步协议；旧 v1/v2 同步对象不能由本版本自动迁移。
- 本地应用配置、会话、FinalShell 导入、托管密钥和 SFTP 传输记录不要求迁移。
- v3 baseline 使用当前设备硬件密钥加密，按 WebDAV/S3 目标隔离保存。

## 升级说明

- Windows、macOS 和 Linux 用户可直接覆盖安装。
- 升级前如需保留旧同步对象，请先自行导出或备份；启用 v3 同步时应确认远端对象可被重新建立。
- 首次使用 v3 同步前，确认所有设备升级到支持 v3 的版本，避免旧客户端覆盖 v3 数据。

## 破坏性变更与已知问题

- 破坏性变更：不再读取、迁移或写回 v1/v2 同步 payload。
- 按用户要求跳过测试，因此本版本未声明测试套件通过。
- 未执行真实 WebDAV/S3 服务和跨平台手工验收。

## 验证结果

- `cargo check --offline`：通过，用于同步 `Cargo.toml` 与 `Cargo.lock` 中的版本号。
- `cargo test`：按用户要求跳过，未执行。
- `cargo clippy`：按用户要求跳过，未执行。
- `cargo build --release`：按用户要求跳过，未执行。
- `git diff --check`：将在提交前执行。
- `python scripts/release_notes.py --check-current`：将在发布资料完成后执行。

## 变更依据

- 目标标签：`v1.5.0`
- 最近祖先版本：`v1.4.5`
- Compare：[v1.4.5...v1.5.0](https://github.com/ynx-official/tiny-shell/compare/v1.4.5...v1.5.0)

[返回版本总览](../README.md)