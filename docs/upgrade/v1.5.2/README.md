# TinyShell v1.5.2

> 发布日期：2026-08-14

## 版本概述

本版本新增 Docker 工具面板，允许用户在当前本地或 SSH 终端目标上查看和管理 Docker 容器与镜像；同时集中优化终端、SFTP、工作区和连接管理器的布局与交互反馈，提升窄窗口下的可读性和操作稳定性。

## 新功能

- 工具面板支持在本地或当前 SSH 主机上查看 Docker 容器和镜像。
- 容器列表支持按名称、镜像、状态、端口和 ID 搜索，并可筛选运行中或已停止的容器。
- 支持启动、停止、重启和删除容器，以及查看和切换自动启动策略。
- Docker 操作支持固定命令参数、请求代次、超时和输出大小限制。
- 工具面板可以根据当前目标终端切换本地主机和远程 SSH 主机，并在目标断开时显示明确状态。

## 改进与修复

- Docker CLI 缺失、守护进程不可用、权限不足、远程会话断开和命令超时都会显示可理解的失败原因。
- Docker 操作不会自动执行 sudo，避免在用户未授权时触发额外权限请求。
- 优化 SFTP 面板、终端工作区和工具面板的响应式布局、空状态、加载状态和错误反馈。
- 连接管理器统一普通分组、会话、回收站分组和回收站会话的树形缩进及图标槽位，长名称、主机和用户名会安全省略。
- 记录快速连接窗口使用的 `awesome-design-md` Sentry 风格约束，保持紧凑的信息层级和 8px 间距节奏。

## 行为与界面变化

- 打开工具面板后，需要先打开并进入本地或 SSH 终端；SSH 会话断开时不能继续执行 Docker 操作。
- 容器操作中的按钮会进入进行中状态，重复请求和过期响应不会覆盖当前目标状态。
- 连接管理器的树行现在为展开控制、图标和文本保留固定槽位，嵌套分组与普通会话的左侧对齐更加稳定。

## 配置与数据兼容性

- 配置、会话、同步、托管密钥和 SFTP 传输数据格式保持兼容，无需迁移。
- Docker 工具面板状态属于本地窗口运行状态，不会上传到配置同步数据。
- Docker 操作不修改远程主机上的 Docker 配置以外的应用数据；删除容器属于用户主动操作，执行前需要确认。

## 升级说明

Windows、macOS 和 Linux 用户可以直接覆盖安装。使用 Docker 工具面板前，请确认目标主机已安装 Docker CLI，当前用户具备访问 Docker daemon 的权限；远程主机还需要保持 SSH 会话连接。

## 破坏性变更与已知问题

- 无配置、会话和同步协议破坏性变更。
- Docker 工具面板仅支持 Docker CLI 能力，不包含 Docker Compose、Swarm、Kubernetes 或自动安装 Docker。
- 未执行真实 Docker daemon、远程 Docker、Windows/macOS/Linux 手工验收。
- 按用户要求跳过测试，因此本版本不声明测试套件通过。

## 验证结果

- `cargo check --locked`：通过。
- `cargo fmt --all -- --check`：通过。
- `cargo clippy --locked --all-targets -- -D warnings`：通过。
- `cargo test --locked --all-targets`：按用户要求跳过。
- `cargo build --locked --release`：通过。
- `python scripts/release_notes.py --check-current`：通过。

## 变更依据

- 目标标签：尚未创建 `v1.5.2`
- 最近祖先版本：`v1.5.1`
- Compare：发布后可查看 [v1.5.1...v1.5.2](https://github.com/ynx-official/tiny-shell/compare/v1.5.1...v1.5.2)

[返回版本总览](../README.md)
