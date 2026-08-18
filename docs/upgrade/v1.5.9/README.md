# TinyShell v1.5.9

> 发布日期：2026-08-18

## 版本概述

本版本调整跨平台 RDP 实现边界：Windows 使用系统自带的 `mstsc.exe`，macOS/Linux 继续使用可选的 FreeRDP 3 原生后端。这样 Windows 安装包不再携带 FreeRDP DLL，同时可以直接复用系统远程桌面客户端的成熟能力。

## 改进与修复

- Windows RDP 连接生成临时 `.rdp` 配置并启动系统 `mstsc.exe`，支持系统客户端提供的键盘、剪贴板、磁盘重定向、全屏和重连能力。
- Windows 临时配置只写入主机、端口、用户名和安全选项，不保存密码；凭据交由 Windows 凭据管理器或系统客户端提示处理。
- Windows 运行时不再链接或打包 FreeRDP DLL；发布流程保留 macOS/Linux 的 FreeRDP 依赖发现和原生库打包。
- macOS/Linux 继续支持 FreeRDP 动态分辨率、双向 Unicode 文本剪贴板、基础输入、证书信任和断线重连。
- 补充 Windows 原生 RDP 启动、系统能力提示、剪贴板和平台依赖的中英文文案。
- 更新 CI、发布工作流、安装包脚本和 README，明确 Windows、macOS、Linux 的 RDP 构建与运行要求。

## 行为与界面变化

- Windows 双击 RDP 连接后打开系统远程桌面窗口，不在 TinyShell 标签页中渲染远程桌面画面。
- Windows 的 RDP 输入、剪贴板、全屏和重连行为由 `mstsc.exe` 接管；macOS/Linux 继续在 TinyShell 标签页中显示嵌入式桌面。
- Windows 系统缺少远程桌面连接组件或 `mstsc.exe` 启动失败时，TinyShell 显示明确错误并清理临时配置。

## 配置与数据兼容性

- 配置、会话、v3 同步、托管密钥和 SFTP 传输数据格式保持兼容，无需迁移。
- 既有 RDP 会话配置仍可使用；Windows 连接路径从嵌入式后端切换为系统客户端，不改变会话字段。
- 临时 `.rdp` 文件在系统客户端退出后自动删除，不作为用户配置持久化。

## 升级说明

Windows 用户可以直接覆盖安装，并确认系统启用了“远程桌面连接”组件。首次使用 RDP 时，系统可能显示 Windows 凭据或证书确认界面。macOS/Linux 用户若从源码构建嵌入式 RDP，需要安装 FreeRDP 3 开发库并确保 `pkg-config` 可以发现它。

## 破坏性变更与已知问题

- Windows RDP 不再在 TinyShell 内部渲染桌面标签，改由系统 `mstsc.exe` 单独显示窗口。
- macOS/Linux 仍受 FreeRDP 3、主机图形栈和发行版运行库依赖影响。
- AppImage 不捆绑 glibc、显卡驱动或 Mesa/Vulkan 等主机图形栈。
- 按用户要求跳过测试、静态检查和本地 release 构建，未执行真实 Windows/macOS/Linux RDP 手工验收。

## 验证结果

- `cargo check --locked`：通过。
- `python scripts/release_notes.py --check-current`：通过。
- `git diff --check`：通过。
- `cargo fmt --all -- --check`：按用户要求跳过。
- `cargo clippy --locked --all-targets -- -D warnings`：按用户要求跳过。
- `cargo test --locked --all-targets`：按用户要求跳过。
- `cargo build --locked --release`：按用户要求跳过。
- Windows、macOS、Linux 真实 RDP 验收：由标签流水线和后续手工验证确认。

## 变更依据

- 目标标签：尚未创建 `v1.5.9`
- 最近正式标签：`v1.5.8`
- Compare：发布后可查看 [v1.5.8...v1.5.9](https://github.com/ynx-official/tiny-shell/compare/v1.5.8...v1.5.9)

[返回版本总览](../README.md)
