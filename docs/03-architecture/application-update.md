# TinyShell 版本更新机制与复用指南

> 状态：Review
> 最后更新：2026-10-29
> 源码基线：`Cargo.toml` 版本 `2.2.1`；提交 `1dd78867dc2d844153015a86eac6c3558ddbe7f8`，本地描述 `v2.2.1-1-g1dd7886`
> 关联：[文档索引](../README.md)、[Windows 安装版交互与验收](../02-design/windows-installer-update.md)、[构建与打包](../05-operations/build-and-package.md)、[版本历史](../upgrade/README.md)、[发布规范](../../AGENTS.md)

> 分支增量说明（2026-10-29）：本页继续记录 2.2.1 源码基线；当前工作区新增的并行选源、清单签名及待安装保护见[在线更新线路选择](../02-design/update-routes.md)。下文“没有多源或独立签名”等边界仅指该基线，不代表未发布分支的新实现。

本文记录上述源码基线中的实际实现，覆盖应用内检查、下载、安装，以及发布端如何提供更新产物。它是更新实现与移植边界的统一说明；Windows 安装向导的交互决策和历史验收记录仍以关联专项文档为准，历次发布结果以版本详情为准。

“已实现”指代码中存在该逻辑，不代表本次已运行验证。本文未查询线上最新 Release，也不把工作区版本等同于当前线上版本。复用建议在第 11 节明确列出，不属于已经完成的功能。

快速导航：

- [整体机制](#1-整体机制)与[代码地图](#2-代码地图与职责)
- [检查调度](#3-检查入口与调度规则)与[元数据协议](#4-更新元数据来源与协议)
- [版本比较和选包](#5-版本比较与安装类型识别)、[状态机](#6-状态机取消和重试)、[下载校验](#7-下载校验与文件准备)
- [各平台安装与重启](#8-安装与重启的跨平台差异)
- [发布端流程](#9-发布端如何支撑客户端更新)
- [当前边界](#10-当前边界与容易误解的地方)与[迁移复用](#11-迁移到其他项目时如何复用)
- [验证、验收与排障](#12-验证验收与排障索引)

## 1. 整体机制

更新采用“GitHub Releases 分发完整安装产物 + 客户端 SHA-256 校验 + 按平台安装”的方式，无独立更新服务端。

```text
发布端
Cargo.toml 版本 + 对应版本详情 + 索引
  → 校验版本资料
  → Windows / Linux / macOS 双架构构建打包
  → 12 个平台产物 + release-notes.txt
  → 生成 update-manifest.json
  → 上传 GitHub Release

客户端
启动 / 定时 / 手动检查
  → 更新清单 → 必要时 GitHub API → 必要时 GitHub HTML
  → 版本比较 → 按操作系统、架构、安装类型、macOS 版本分支选包
  → 用户点击下载 → 完整下载 → 大小与 SHA-256 校验 → 准备安装文件
  → 显示“更新已下载，等待安装”
  → 用户确认 → 平台安装路径 → 启动新程序或由安装向导决定是否启动
```

核心规则：

- 自动任务只检查版本；下载和安装都需要用户主动操作。
- 使用完整产物，无差量更新、断点续传或后台自动安装。
- 下载与安装分离。下载完成不修改正在运行的主程序；Windows 便携版会先在主程序旁暂存新文件。
- `ReadyToRestart` 是历史枚举名称，实际表示“已下载、已校验、等待安装”，不是安装成功。
- 仅关闭并重新打开应用不会安装下载好的更新。运行状态没有持久化恢复机制。
- SHA-256 校验用于完整性校验；当前没有独立更新签名、公钥验签或统一的安装后健康确认协议。

## 2. 代码地图与职责

| 位置 | 主要职责与入口 |
| --- | --- |
| [Cargo.toml](../../Cargo.toml) / [Cargo.lock](../../Cargo.lock) | 应用版本来源与锁文件对应包版本；运行时通过 `env!("CARGO_PKG_VERSION")` 获取 |
| [updater/release_source.rs](../../src/app/updater/release_source.rs) | `fetch_latest_release`：清单、API、HTML 三种来源，元数据校验及更新说明 |
| [updater/mod.rs](../../src/app/updater/mod.rs) | `installation_kind`、`check_for_update`、`download_update`、`install_and_restart`：检测、选包、下载、校验、平台安装 |
| [updater/ui.rs](../../src/app/updater/ui.rs) | 检查调度、异步结果回传、下载取消与重试、更新窗口、安装确认 |
| [updater/presentation.rs](../../src/app/updater/presentation.rs) | 按安装类型提供按钮、提示和确认文案键，供两个 UI 入口共用 |
| [updater/indicator.rs](../../src/app/updater/indicator.rs) | 检查等状态的小型动画图标 |
| [updater/windows_setup_update.ps1](../../src/app/updater/windows_setup_update.ps1) | Windows 安装版退出后的向导启动、退出码检查、版本核对与日志 |
| [settings/update.rs](../../src/app/settings/update.rs) | 设置 → 在线更新：频率、通知、状态和操作按钮 |
| [runtime_state.rs](../../src/app/runtime_state.rs) | `UpdateRuntimeState`：状态、取消句柄、下载代次和调度代次 |
| [startup.rs](../../src/app/startup.rs) | 第一个工作区的自动更新调度；进程级 `STARTUP_UPDATE_CHECK_STARTED` 防止启动重复触发 |
| [config_file.rs](../../src/session/config_file.rs) / [config.rs](../../src/session/config.rs) | 更新偏好的默认值、序列化和边界归一化 |
| [confirmation_dialog.rs](../../src/app/confirmation_dialog.rs) / [feedback.rs](../../src/feedback.rs) | 统一安装确认与发起窗口中的安装启动失败提示 |
| [zh-CN.yml](../../locales/zh-CN.yml) / [en.yml](../../locales/en.yml) | 用户可见更新文案 |
| [release.yml](../../.github/workflows/release.yml) | 版本校验、跨平台构建、打包、汇总与发布 |
| [release_notes.py](../../scripts/release_notes.py) | 校验发布资料，导出独立纯文本更新说明 |
| [update_manifest.py](../../scripts/update_manifest.py) | 校验完整产物集合，计算大小和 SHA-256，生成清单 |
| [ci.yml](../../.github/workflows/ci.yml) | 独立的日常质量门禁；不能与 Release 构建结果混同 |

更新引擎与 GPUI 之间通过 `UpdateInfo`、`UpdateCheckResult`、下载进度回调等传递数据。平台安装、网络和 UI 编排虽分区，但仍位于应用模块中，并非可直接独立发布的通用更新库。

## 3. 检查入口与调度规则

### 3.1 配置契约

| 配置字段 | 默认值 | 当前行为 |
| --- | --- | --- |
| `update_check_mode` | `startup` | 序列化值为 `startup`、`interval`、`disabled` |
| `update_interval_hours` | `24` | 读取和写入都夹紧到 `1..=8760` 小时，仅定时模式使用 |
| `update_notify` | `true` | 自动检查发现更新时打开更新窗口；关闭通知不等于关闭检查 |
| `update_last_checked_at` | `0` | 上次成功检查的 UTC Unix 秒；写入时不小于 0 |

检查成功包括“发现更新”和“没有更高版本”，两者都会更新时间并通过 `mark_config_preferences_dirty` 进入配置保存流程。检查失败不更新时间。

### 3.2 启动、定时与手动操作

1. **启动模式**：第一个工作区准备好后触发检查。进程级原子标记限制启动阶段只安排一次；不是每个新窗口都重新执行启动检查。
2. **定时模式**：首次等待时间由上次成功检查时间决定，之后按固定间隔发起检查。
3. **禁用模式**：取消自动调度，手动检查仍可用。
4. **手动检查**：设置页和更新窗口都调用 `check_for_updates`。首次打开更新窗口且状态为 `None` 时会自动发起一次检查。
5. **修改频率**：切换模式或修改有效小时数会重建调度代次。切换到 `startup` 时不会立刻补做启动检查。

首次定时等待的等价公式：

```text
last_checked_at <= 0：立即检查
否则：
  interval_seconds = clamp(hours, 1, 8760) × 3600
  elapsed = max(now - last_checked_at, 0)
  delay = max(interval_seconds - elapsed, 0)
```

因此，已经超过间隔会立即检查；系统时钟回拨时最多重新等待一个完整间隔。循环是“发起检查后等待一个间隔”，没有失败指数退避。手动成功检查会记录时间，但不会直接重置已经运行的定时器。

检查入口只在 `Checking` 或 `Downloading` 状态下忽略重复调用。当前没有保护 `ReadyToRestart`：手动重查或定时检查可能把“等待安装”替换成新的检查结果，见第 10 节。

### 3.3 异步与窗口归属

- 网络检查和下载提交到共享 Tokio runtime；GPUI 使用 oneshot 接收结果，下载使用进度通道接收字节数。
- 自动调度使用 supervisor 中的 `automatic-update` 任务和 `schedule_generation`；重排后旧循环不再发起新检查。
- 下载使用 `update-download` 任务、独立的 `DownloadCancellation` 和 `download_generation`。
- 自动检查携带发起窗口句柄；仅在发现更新、`update_notify=true` 且窗口可更新时打开更新窗口。
- 手动检查不额外触发自动通知弹窗，结果在已有状态界面展示。
- 更新状态属于对应 `TinyShell` 实体，不是覆盖所有窗口和进程的全局安装锁。启动去重也不能替代多实例互斥。

## 4. 更新元数据来源与协议

### 4.1 来源顺序

| 优先级 | 当前固定地址 | 用途 |
| --- | --- | --- |
| 1 | `https://github.com/ynx-official/tiny-shell/releases/latest/download/update-manifest.json` | 正常更新入口，直接获取发布附件 |
| 2 | `https://api.github.com/repos/ynx-official/tiny-shell/releases/latest` | 清单缺失或服务不可用时回退 |
| 3 | `https://github.com/ynx-official/tiny-shell/releases/latest` | 解析最终标签地址，再请求 `releases/expanded_assets/<tag>` |

客户端固定使用这些 `latest` 入口，没有遍历历史版本、自定义更新源或稳定版/测试版频道选择。预发布版本能被版本解析器识别，并不代表存在订阅预发布渠道的功能。

### 4.2 只对特定不可用情况回退

| 情况 | 清单阶段 | API 阶段 |
| --- | --- | --- |
| 请求发送失败，例如连接失败 | 回退 API | 回退 HTML |
| HTTP 404 | 视为旧 Release 无清单，回退 API | 直接报“尚未发布”，不进入 HTML |
| HTTP 403 / 429 / 5xx | 回退 API | 回退 HTML |
| 其他非成功 HTTP 状态 | 直接失败 | 直接失败 |
| 成功响应但 JSON、版本、URL、摘要等不合法 | 直接失败 | 直接失败 |
| 成功响应后的正文读取/解析错误 | 直接失败 | 直接失败 |
| 更新说明下载失败 | 保留版本与产物，说明置空 | API 正文随响应解析；HTML 的独立说明失败时置空 |

这个区别很重要：回退用于可用性兼容，不用于绕过损坏或非法的元数据。格式错误的清单不会被悄悄忽略后继续更新。

API 错误会保留 HTTP 状态，尽可能解析 `x-ratelimit-reset`，响应正文最多保留 512 个字符。API 和 HTML 都失败时，最终错误包含两个阶段的上下文。

### 4.3 Manifest v1

以下是单个产物的结构示例，大小和摘要为占位数据，不能用于真实发布；当前生成器要求完整的 12 项集合。

```json
{
  "schema_version": 1,
  "version": "v2.2.1",
  "notes_url": "https://github.com/ynx-official/tiny-shell/releases/download/v2.2.1/release-notes.txt",
  "assets": [
    {
      "name": "tiny-shell-v2.2.1-windows-x86_64-setup.exe",
      "url": "https://github.com/ynx-official/tiny-shell/releases/download/v2.2.1/tiny-shell-v2.2.1-windows-x86_64-setup.exe",
      "size": 123456,
      "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    }
  ]
}
```

客户端校验规则：

- `schema_version` 必须等于 `1`，`assets` 不能为空，版本必须能解析为 SemVer。
- 文件名不能为空，不能包含 `/` 或 `\`。
- 产物 URL 必须是 HTTPS，主机为 `github.com`，路径必须精确对应 `/ynx-official/tiny-shell/releases/download/<version>/<name>`，不能带 query 或 fragment。
- `digest` 必须以小写前缀 `sha256:` 开始，后面恰好为 64 位十六进制字符；最终摘要比较不区分十六进制字母大小写。
- `size` 为 `u64`；大于 0 时下载完成必须严格相等，为 0 时不做元数据长度相等检查。
- `notes_url` 可省略。存在时只检查 HTTPS，没有复用产物的仓库、主机和路径白名单。
- 清单内所有条目都先校验，再选择当前平台条目；其他平台的一条非法记录也会使本次检查失败。

清单客户端没有要求恰好 12 项，也没有检查重复名；严格产物集合检查由当前 Python 生成器负责。

### 4.4 API 与 HTML 的归一化

三种来源最终统一为：

```text
ReleaseMetadata
  version
  notes
  assets[]
    name
    download_url
    size
    digest
```

- API 把 `tag_name`、`body`、`browser_download_url`、`size`、`digest` 映射到该结构。每个附件都必须有合法摘要；没有摘要时拒绝继续。
- HTML 首先确认 `latest` 的最终 URL 为同一仓库的 `releases/tag/<tag>`，然后读取 `expanded_assets/<tag>`。
- HTML 以 `<li` 分段，在同一个条目内提取附件路径及 `sha256:` 摘要，避免把其他附件的摘要误配给当前附件。
- HTML 不读取精确大小，设 `size=0`；下载进度优先使用 HTTP `Content-Length`，完整性仍必须通过 SHA-256。
- HTML 解析依赖 GitHub 页面结构，属于兼容回退路径。

### 4.5 更新说明

正常发布时，`docs/upgrade/<tag>/README.md` 中的“线上更新说明”导出为 `release-notes.txt`，同时作为 Release 正文和清单 `notes_url` 的目标。

HTML 回退优先找 `release-notes.txt`，旧版本没有该附件时读取标签下的 `docs/upgrade/<tag>/README.md` 原文。客户端过滤“验证结果 / Validation Results”“变更依据 / Change Basis”章节及返回版本总览的链接。它没有将任意 Markdown 完整转换为纯文本的逻辑，正常路径依赖发布端提供纯文本。

独立说明读取失败只记录日志并返回空文本，UI 显示“暂无更新说明”，不阻止有效安装产物的更新。

## 5. 版本比较与安装类型识别

### 5.1 版本比较

当前版本来自编译期 `CARGO_PKG_VERSION`。远端与本地版本先去掉首尾空白和单个 `v` / `V` 前缀，再使用 `semver::Version` 解析并直接执行 `latest > current`。

- 数字段按版本类型比较，不做字符串排序，避免 `1.0.10` 被排在 `1.0.9` 前面。
- 只有更大的版本才进入产物选择；相同或更低版本均返回 `UpToDate(ReleaseInfo)`，不会自动降级。
- 非法版本直接失败。
- `UpToDate` 保存的是远端返回的版本和说明，因此本地版本高于远端时，“最新版本”信息仍来自远端。
- 发布脚本的标签正则比 Rust 解析器窄：接受 `vX.Y.Z` 及 `-预发布后缀`，不接受 `+构建元数据`；复用时应统一两端约束。

### 5.2 安装类型检测

| 平台 | 判定规则 | 结果 |
| --- | --- | --- |
| Windows | 在 HKCU / HKLM 中读取固定 Inno Setup 卸载键的 `InstallLocation`，与当前 exe 父目录做忽略大小写、去末尾分隔符的字符串比较 | 匹配为 `WindowsInstaller`，否则 `Portable` |
| macOS | 当前 exe 的祖先目录存在 `.app`，且 `pkgutil --pkg-info-plist <identifier>` 成功 | `MacInstaller` |
| macOS | 在 `.app` 内但没有安装收据 | `MacApp` |
| macOS | 不在 `.app` 内 | `Portable` |
| Linux | `APPIMAGE`、`APPDIR`、当前 exe 都是有效绝对路径，canonicalize 后类型正确，exe 确实在 `APPDIR` 内 | `LinuxAppImage` |
| Linux | 不是 AppImage，exe 在 `/usr/bin` 或 `/opt` 下 | `LinuxPackage` |
| Linux | 其他路径 | `Portable` |

Windows 安装器身份绑定固定 AppId `{8E091D1C-6D7C-4C29-9CA2-8B3D84A42CF8}`，卸载键后缀为 `_is1`。不能仅凭存在同名安装记录就认定当前运行文件是安装版。

macOS 基础版收据 ID 为 `dev.tiny-shell.app`，RDP 版为 `dev.tiny-shell.rdp`。RDP 构建兼容旧基础版收据；基础版只识别基础版收据。当前检测没有把收据安装位置与正在运行的 `.app` 位置做进一步关联。

Linux 特别核对 exe 与 `APPDIR` 的包含关系，防止从其他 AppImage 启动 TinyShell 时继承的环境变量误指向父应用。`LinuxPackage` 只是路径分类，不代表使用系统包管理器安装更新。

### 5.3 产物匹配矩阵

下表文件名均以前缀 `tiny-shell-<tag>-` 开始：

| 平台 / 安装类型 | 匹配的名称后缀 | 实际安装对象 |
| --- | --- | --- |
| Windows x86_64 安装版 | `windows-x86_64-setup.exe` | Inno Setup 安装目录中的文件 |
| Windows x86_64 便携版 | `windows-x86_64-portable.zip` | 当前主 exe |
| macOS aarch64 / x86_64 安装版 | `macos-<arch>-setup.pkg` 或 `macos-<arch>-rdp-setup.pkg` | `/Applications/TinyShell.app` |
| macOS aarch64 / x86_64 App / 便携版 | `macos-<arch>-portable.zip` 或 `macos-<arch>-rdp-portable.zip` | 整个 `.app`；独立运行二进制时为当前主程序 |
| Linux x86_64 AppImage | `linux-x86_64.AppImage` | 外层 AppImage 文件 |
| Linux x86_64 便携 / 系统路径 | `linux-x86_64.tar.gz` | 当前主程序 |

macOS 是否匹配 `-rdp-` 由当前二进制的 `cfg!(tiny_shell_freerdp_backend)` 决定，更新不会自动切换基础版与 RDP 版。没有对应架构或安装类型产物时直接报错，不回退到另一种安装类型。

当前支持 Windows/Linux x86_64、macOS x86_64/aarch64，其他平台映射为 `unknown`。选包主要基于名称包含平台串、后缀和 RDP 标记，取第一个匹配项；不是一个带显式 OS/arch 字段的安装协议。

## 6. 状态机、取消和重试

| 状态 | 进入条件 | 可执行操作 / 后续状态 |
| --- | --- | --- |
| `None` | 新建运行状态 | 手动或自动进入 `Checking` |
| `Checking` | 发起检查 | `UpToDate` / `UpdateAvailable` / `Error`；忽略重复检查 |
| `UpToDate(ReleaseInfo)` | 远端版本不高于当前 | 再次检查、查看说明或发布页 |
| `UpdateAvailable(UpdateInfo)` | 发现更高版本且选包成功 | 下载或再次检查 |
| `Downloading(info, done, total)` | 开始下载 | 进度更新、取消；不接受重复检查 |
| `DownloadCancelled(info)` | 用户取消下载 | 重新下载或检查 |
| `DownloadFailed(info, error)` | 下载、校验、解压或暂存失败 | 查看错误并重新下载，或重新检查 |
| `ReadyToRestart(info, path)` | 下载校验和准备阶段完成 | 确认安装；取消确认保持状态；也可能被再次检查覆盖 |
| `Error(message)` | 检查阶段失败 | 再次检查 |

`UpdateInfo` 包含规范化的目标版本、说明、下载 URL、大小、摘要和安装类型。下载失败/取消保留同一份 `UpdateInfo`，所以“重新下载”直接使用原版本元数据，不会自动刷新最新版本。

取消机制：

1. `DownloadCancellation` 使用共享 `AtomicBool + tokio::sync::Notify`。
2. 发起 HTTP 请求和等待流式数据块时通过 `tokio::select!` 响应取消，网络停滞时也能唤醒取消分支。
3. UI 立即切换到 `DownloadCancelled`，并增加下载代次。
4. 旧任务的进度或完成结果必须通过代次校验，避免覆盖新下载的状态。
5. 重试会重新下载完整文件；没有保存 HTTP Range 偏移量。

取消检查主要覆盖网络读取及网络结束后的一个检查点。后续同步哈希、解压、写盘或 Windows 暂存阶段没有逐步取消检查；此时取消仍可阻止旧结果覆盖 UI，但不能保证后台立即停止或没有临时残留。

## 7. 下载、校验与文件准备

### 7.1 HTTP 与资源限制

| 参数 | 当前值 / 行为 |
| --- | --- |
| User-Agent | `tiny-shell/<当前版本>` |
| 连接超时 | 15 秒 |
| 读取超时 | 60 秒；不是整个大文件下载总时长限制 |
| 进度总大小 | 优先 HTTP `Content-Length`，缺省时使用元数据 `size` |
| 缓冲方式 | 按块读取网络流，但完整内容累积在内存 `Vec<u8>` 中 |
| 下载上限 | `1024 × 1024 × 1024` 字节，即 1 GiB；超过时失败 |
| 初始预留容量 | `min(total, 1 GiB)`，不会按无限声明长度预分配 |
| 进度显示 | `done × 100 / total`，夹紧到 100；总大小为 0 时显示 0 |

检查和下载分别创建 Reqwest client。该代码没有显式注入应用的 SSH/全局代理配置；不能从连接代理设置推断更新请求必经该代理。

### 7.2 顺序与落盘

1. 检查 HTTP 成功状态，读取全部响应；每块累加字节数、检查上限并发送进度。
2. 如果元数据 `size > 0`，严格核对实际字节数。该检查使用元数据大小，不以 HTTP 声明替代。
3. 对完整下载内容计算 SHA-256，与预期摘要比较；失败时不进入安装准备。
4. 把版本解析为规范 SemVer，准备 `<系统临时目录>/tiny-shell-update-<version>`。同版本目录已存在时先删除后重建，避免找到上次残留的旧 exe。
5. 安装器 / AppImage 直接写盘并 `sync_all`；ZIP 或 tar.gz 解压后按主程序名递归寻找目标文件。
6. Windows 便携版再把主 exe 暂存到当前安装目录；其他类型返回准备好的文件路径。

直接载荷临时名称：

| 类型 | 文件名 |
| --- | --- |
| Windows 安装版 | `tiny-shell-v<version>-setup.exe` |
| macOS 安装版 | `tiny-shell-v<version>[-rdp]-setup.pkg` |
| Linux AppImage | `tiny-shell-v<version>-linux-x86_64.AppImage` |

归档类型按 URL 是否以 `.tar.gz` 结尾判断，否则按 ZIP 读取。先找 `tiny-shell.exe` 或 `tiny-shell`；macOS 找不到主程序时另找 `.app`。当前归档扫描取第一个匹配项，没有包内容清单或“只能有一个主程序”的独立校验。

1 GiB 上限限制的是下载内容，不是解压后的总大小、文件数量或解压耗时。元数据和说明读取也没有同样的显式字节上限。

### 7.3 临时文件生命周期

- 下载状态和准备路径仅在内存中保存；重启应用不自动复用。
- 同版本解压目录会在下次准备时清理，但不会全面清理其他版本目录。
- Windows 辅助脚本与日志使用“PID + 纳秒时间 + 进程内计数器”后缀；这不是密码学随机文件创建机制，也没有跨进程文件锁。
- Unix 二进制同目录暂存使用 UUID 路径和 `create_new(true)`，避免覆盖已有暂存目标。
- Windows 安装版成功清理包和脚本，日志保留。其他路径的清理由各平台逻辑负责，不存在统一的缓存回收器。

## 8. 安装与重启的跨平台差异

### 8.1 共用确认边界

更新窗口和设置页均调用 `install_presentation`，然后通过统一 `ConfirmationDialog` 提醒保存工作、等待文件传输结束和连接将断开。

- Windows 安装版按钮为“立即安装”，说明将打开安装向导。
- 其他类型为“安装并重启”。
- 用户取消确认时不调用安装逻辑。
- `install_and_restart` 返回错误时，发起窗口使用 `Feedback::error` 显示原因，确认框保留。
- 安装调用是同步的。文件复制、macOS 提权安装等待及 AppImage 的一秒启动观察可能阻塞调用它的 UI 回调，不能把整个安装阶段描述为后台异步流程。
- 代码最终使用进程退出或 `exec` 交接，没有在此函数中自动等待 SFTP、保存远端编辑文件或恢复 SSH 连接；这些是确认提示要求用户先处理的工作。

### 8.2 Windows 安装版

安装器身份、交互决策与原生验收详情见 [Windows 安装版在线更新](../02-design/windows-installer-update.md)。实现流程如下：

1. Rust 核对安装器非空和目标版本合法，生成独立辅助脚本、更新日志与 `.setup.log` 路径。
2. 使用隐藏的 `powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File <script>` 启动辅助进程；启动成功后当前应用退出。
3. 脚本等待发起应用 PID 退出，再以正常窗口启动 Inno Setup。
4. 安装向导接收当前 exe 父目录，并负责覆盖安装和完成页是否启动 TinyShell。
5. 脚本只等待安装器本身退出，使用进程句柄和 `WaitForExit()`，避免等待安装器启动的新应用。
6. 只有退出码 0 且原目录主 exe 的 `ProductVersion` 等于目标版本时才记录完成并清理包与脚本。

安装参数契约：

| 参数 | 作用 |
| --- | --- |
| `/SP-` | 省略安装开始前的启动询问 |
| `/NORESTART` | 不自动重启操作系统 |
| `/CURRENTUSER` | 沿用当前用户安装模式 |
| `/RESTARTEXITCODE=3010` | 区分需要系统重启的结果 |
| `/DIR="<当前目录>"` | 将当前安装目录作为向导目标 |
| `/LOG="<详情日志>"` | 保留独立安装详情日志 |

没有使用 `/SILENT`、`/VERYSILENT` 或 `/SUPPRESSMSGBOXES`。`/DIR` 是传入的目标目录，正常向导仍可能允许用户改变选择；脚本最终仍核对原目录文件。

| 结果 | 处理 |
| --- | --- |
| `0` 且版本一致 | 成功；删除下载包和脚本；保留两份日志 |
| `2` / `5` | 记录取消或中止；保留文件；不自动重启应用；不能把 5 一律解释为用户主动取消 |
| `3010` | 记录需要系统重启，暂不验证版本、不报告已生效、保留文件 |
| 其他非零 / 无退出码 / 版本不一致 / 启动异常 | 记录失败，保留安装包，并尝试显示本地化系统提示 |

辅助脚本的路径和文字使用 PowerShell 单引号转义，正文带 UTF-8 BOM，支持 Windows PowerShell 5.1 读取中文。日志路径为 `%TEMP%\tiny-shell-update-<suffix>.log` 与同后缀 `.setup.log`，后者只有安装器开始处理参数后才产生。

安装包由 [tiny-shell.iss](../../scripts/windows/tiny-shell.iss) 定义：默认目录为当前用户的 `Programs\TinyShell`，`PrivilegesRequired=lowest`，安装主程序与 `tiny-shell-mcp.exe`，可选复制 runtime DLL；完成页 `[Run]` 负责启动。辅助进程不重复启动，也不在取消后强制启动旧版。

### 8.3 Windows 便携版

1. 下载 ZIP 并校验、解压。
2. 检查新主 exe 非空，复制到当前主程序旁的 `.tiny-shell-update-<PID>.exe`，核对复制字节数并刷新到磁盘。此时正式 exe 未被替换。
3. 用户确认后，检查暂存文件必须与正式 exe 同目录；生成隐藏 PowerShell 辅助脚本。
4. 辅助脚本等待旧 PID 退出，再等待 300 ms；核对暂存 exe 的 `ProductVersion`。
5. 将旧 exe 移为 `.tiny-shell-backup-<PID>.exe`，最多尝试 20 次，失败间隔 250 ms；再把暂存 exe 移到正式路径。
6. 核对安装后的文件大小，启动新主程序，删除备份及辅助脚本，保留更新日志。
7. 异常时尝试恢复备份并启动原程序；回滚失败继续写日志。

此路径只替换主 exe，不复制 ZIP 中的 MCP helper 或其他文件。`Start-Process` 成功后即清理备份，没有等待新应用完成初始化的健康握手。

一个重要例外：如果 Rust 启动便携版辅助脚本失败，当前实现会删除暂存 exe；UI 仍保留 `ReadyToRestart` 和通用重试提示。因此“安装启动失败总能用原文件重试”只在安装版路径成立，便携版可能需要重新检查并下载。

### 8.4 macOS `.app` 便携更新

1. 根据当前 exe 找到正在运行的 `.app`，从下载结果定位新 `.app`。
2. 将整个新 bundle 复制到原目录旁的 `<app-name>.new`，复制过程中保留旧 bundle，并保留软链接。
3. 将旧 bundle 改名为 `<app-name>.old`，再把 `.new` 改为正式名称。
4. 第二次改名失败时尝试恢复 `.old`；成功后执行 `codesign --force --deep --sign -` 临时重签名。
5. 重签失败仅记录警告；随后删除旧备份，调用 `open -n <app>` 再退出旧进程。

这是“先准备、再两次改名”的切换过程，不是一个包含所有步骤的原子事务。成功路径会替换整个 bundle，包括其中 helper 和动态库；没有新应用健康检查，`open` 启动错误也没有完整回传。

如果当前程序不在 `.app` 内，则复用 Unix 单二进制替换路径，见 8.6。

### 8.5 macOS `.pkg` 安装更新

1. 通过 `osascript` 执行带管理员权限的 `/usr/sbin/installer -pkg '<包路径>' -target /`，显示系统授权提示。
2. 等待命令完成，非成功状态返回包含 stderr 的错误。
3. 读取 `/Applications/TinyShell.app/Contents/Info.plist` 的 `CFBundleShortVersionString`，必须与目标版本逐字相等。
4. 成功后走通用 macOS 重启流程。

`.pkg` 安装位置由发布包固定为 `/Applications`；通用重启却根据当前 exe 所在 bundle 选择路径。因此复制到其他目录的 `.app` 加上遗留安装收据可能造成“安装目标”和“重启目标”不一致，复用时需要单独处理。

当前无此路径的自动旧包备份、失败回滚或明确成功清理下载包步骤。包安装成功后版本核对失败会报错，但不自动撤销系统安装器已经执行的变更。

### 8.6 Linux tar.gz / 普通 Unix 二进制

1. 解压后找到主程序。
2. 在当前 exe 同目录创建 UUID 命名的暂存文件，采用独占创建，复制内容、设 `0755` 并 `sync_all`。
3. `rename` 暂存文件覆盖当前 exe；失败时清理暂存文件。
4. Linux 使用 `exec` 重新执行程序；当前进程的旧 inode 可在替换过程中继续存在。

该流程只更新主程序，不更新 tar.gz 内的 `tiny-shell-mcp` 和 `lib/`。即使分类为 `LinuxPackage`，也不调用 apt/dpkg，不自动提权；安装目录不可写时返回错误。没有安装后的版本核对或自动备份回滚。

### 8.7 Linux AppImage

1. 重新确认有效的外层 `APPIMAGE` 路径，不修改只读挂载目录内的 exe。
2. 读取载荷头，要求 ELF、64 位、小端、`AI\x02` Type 2 标记及 x86_64 machine ID 62。
3. 在同目录创建独占 UUID 备份，保留旧文件权限；使用 Unix 暂存改名流程替换外层镜像。
4. 启动新的外层 AppImage，旧进程继续存活一秒。
5. 新进程一秒后仍运行：删除备份，再退出旧进程。
6. 启动失败、提前退出或无法观察进程：尝试恢复备份，向原窗口返回错误；回滚失败保留备份位置供排查。

整个镜像被替换，因此其中主程序、helper 和库一起更新。一秒观察用于捕获早期失败，不代表应用功能和会话恢复全部正常。

## 9. 发布端如何支撑客户端更新

### 9.1 版本资料一致性

发布操作规范以 [AGENTS.md](../../AGENTS.md) 为准。准备新版本时需要同步：

1. `Cargo.toml` 的 `[package].version`。
2. `Cargo.lock` 中 `tiny-shell` 的版本。
3. `docs/upgrade/vX.Y.Z/README.md`：用户变化、兼容性、升级要求、实际验证与纯文本线上说明。
4. `docs/upgrade/README.md`：当前版本和对应日期、详情链接、摘要。
5. `CHANGELOG.md`：版本条目与底部详情链接引用。
6. 最终目标提交上的 `vX.Y.Z` 标签，与包版本完全一致。

当前 `release_notes.py` 实际检查包版本、标签格式、详情标题、日期格式、索引及变更日志引用；普通导出还检查“版本概述”。它没有读取 `Cargo.lock`，也不验证所有发布文档章节、真实发布日期或构建结果。因此仓库规则中的锁文件一致性仍需额外核对，不能单凭脚本成功推定全部发布要求满足。

`--plain-text` 独立检查“线上更新说明”非空并拒绝部分 Markdown 标记和内部文档引用。`--check-current` 默认不检查该纯文本章节，必须另加 `--plain-text` 或执行发布流程中的纯文本导出步骤。

### 9.2 Release workflow

触发条件：推送 `v*` 标签，或 `workflow_dispatch` 手动执行。手动在非标签 ref 上运行只生成 workflow artifacts；发布任务要求 ref 以 `refs/tags/` 开头。

流程为 `validate → build → publish`：

1. `validate` 校验版本资料并尝试生成纯文本说明；默认运行发布工具 Python 测试。
2. 版本详情出现精确行 `> 发布测试策略：skip` 时，仅跳过该 Release workflow 的发布工具测试。它不跳过元数据校验和打包，也不代表独立 CI 质量门禁已经通过。
3. 四个构建目标：Windows x86_64、Ubuntu 24.04 Linux x86_64、macOS 14 Apple Silicon、macOS 15 Intel。
4. 构建前读取 Cargo 版本，标签构建严格核对标签版本；产物版本统一为 `v<Cargo version>`。
5. Windows 使用 `--no-default-features`，Linux 使用 `--features freerdp`，macOS 分别构建 RDP 与基础版。
6. 所有构建通过后，`publish` 导出 `release-notes.txt`、下载所有 workflow artifacts、生成清单。
7. `softprops/action-gh-release` 上传 `dist/**/*`，正文来自纯文本文件，附件匹配失败会报错。

同一 ref 使用 `release-${github.ref}` 并发组，`cancel-in-progress=false`。Release workflow 没有以独立 `ci.yml` 的质量任务作为显式依赖，所以“Release 构建成功”不能代替“Clippy / 测试通过”。

工作流未显式配置 `prerelease` 或频道字段。引入 beta/rc 自动分发时，需要同时设计发布元数据与客户端入口，而不是只改变版本后缀。

### 9.3 当前产物集合

| 平台 | 数量 | 包类型 |
| --- | --- | --- |
| Windows x86_64 | 2 | portable ZIP + setup EXE |
| Linux x86_64 | 2 | AppImage + tar.gz |
| macOS aarch64 | 4 | 基础版 ZIP / PKG + RDP 版 ZIP / PKG |
| macOS x86_64 | 4 | 基础版 ZIP / PKG + RDP 版 ZIP / PKG |
| 发布说明和清单 | 2 | `release-notes.txt` + `update-manifest.json` |

完整 Release 共 14 个附件，清单的 `assets` 只包含 12 个平台产物。

打包脚本：[Windows](../../scripts/package-windows.ps1)、[macOS](../../scripts/package-macos-app.sh)、[Linux AppImage](../../scripts/package-linux-appimage.sh)。Windows 安装包和 ZIP 包含 helper；macOS 整个 bundle 包含 helper，RDP 版包含相关动态库；Linux tar.gz 在流水线中同时打包主程序、helper 和 FreeRDP / WinPR 库并设置相对 rpath。

### 9.4 清单生成器的约束

`update_manifest.py` 会递归扫描产物目录：

- 校验标签和 `owner/repository` 格式。
- 忽略 `update-manifest.json` 和 `release-notes.txt`。
- 其余文件必须恰好匹配该标签的 12 个预期名称；缺少、多余或跨目录重名都失败。
- 读取实际文件大小，以 1 MiB 块计算 SHA-256。
- 按名称排序产物，使用稳定 JSON 排序、UTF-8 和末尾换行输出，便于重现和比较。
- 构造固定标签下载 URL，避免客户端检查后下载到漂移的另一版附件。

生成器会输出 `notes_url`，但没有要求对应 `release-notes.txt` 文件实际存在；当前工作流通过先导出说明来满足这一前提。单独复用生成器时必须补上该前提检查。

### 9.5 可复用操作顺序

以下为后续维护步骤，不是本次已经执行的发布动作：

1. 确认目标版本、真实祖先发布标签及实际变更范围。
2. 同步六项版本资料；检查锁文件中的主包版本。
3. 按授权范围完成测试、构建与手动验收，明确记录未执行项。
4. 执行不触发 Rust 编译的资料检查：

   ```bash
   python scripts/release_notes.py --check-current
   python scripts/release_notes.py --check-current --plain-text
   ```

5. 正式发布获得明确授权后，再按仓库规范提交、创建标签并推送。
6. 等待 Release 各平台产物完成；核对附件数、清单 URL / 大小 / SHA-256 和 Release 正文。
7. 使用旧版真实客户端验收下载及安装；新代码无法改变已经运行的旧版更新器。如果旧更新器无法完成升级，应提供手动覆盖安装路径。

有完整构建产物时，生成清单的命令模板为：

```bash
python scripts/release_notes.py --tag vX.Y.Z --plain-text --output dist/release-notes.txt
python scripts/update_manifest.py --dist dist --tag vX.Y.Z --repository OWNER/REPOSITORY --output dist/update-manifest.json
```

`vX.Y.Z` 和 `OWNER/REPOSITORY` 是待替换占位符。换仓库时还必须修改客户端的地址与校验常量，否则它会拒绝新仓库产物。

## 10. 当前边界与容易误解的地方

以下由代码路径推导，未在本次运行中重现；用于说明复用约束，不是已经修复的事项。

| 边界 | 当前影响 |
| --- | --- |
| 单文件替换与完整包内容不一致 | Windows 便携、Linux tar.gz、macOS 独立二进制只更新主程序；helper / 动态库可能继续是旧版本 |
| 等待安装状态可被检查覆盖 | `ReadyToRestart` 不在检查互斥范围内；再次检查后可能需要重新下载 |
| 下载缓存没有恢复索引 | 重启应用不会自动接续待安装任务，也没有断点续传 |
| 临时目录按版本共享 | 同一用户的多个实例下载同版本可能相互清理目录；没有统一进程间更新锁 |
| 安装过程并非全程异步 | 同步复制、提权等待及启动观察可能短暂或长时间占用 UI 回调 |
| 回滚能力不统一 | AppImage 有一秒观察回滚，Windows 便携有脚本异常回滚，macOS App 只有切换失败恢复，其他路径没有统一回滚协议 |
| Windows 便携脚本启动失败会清理暂存文件 | 通用“包已保留，可重试”文案不完全对应此分支 |
| 本地文件可能在校验后变化 | 下载时验摘要；安装脚本没有统一重新哈希，版本号/大小校验也不是签名认证 |
| macOS 收据与路径未严格绑定 | 被移动的 bundle 可能被当成安装版；安装与重启可能指向不同位置 |
| Linux 系统路径没有包管理器接入 | 不可写目录会失败，不能认为 `LinuxPackage` 自动获得管理员权限 |
| 元数据来源依赖 GitHub | API/HTML 回退仍在同一服务；HTML 结构变化可能使回退失效 |
| 没有独立签名协议 | 清单与文件同源；摘要能检查字节一致性，不能独立证明发布者身份 |
| 没有灰度、强制更新或最低版本策略 | 仅比较更高版本，不限制跨大版本升级，也不协调配置迁移 |
| 发布资料检查并非全覆盖 | 未自动检查锁文件，默认资料检查不验证纯文本章节；需按 9.1 补齐 |

## 11. 迁移到其他项目时如何复用

本节为建议，不是本项目现有承诺。可先复用协议、职责划分和失败语义，再根据目标项目包结构选择安装实现。

### 11.1 建议保留的分层

| 层 | 输入 / 输出 | 可复用原则 |
| --- | --- | --- |
| 发布元数据源 | 固定更新入口 → `ReleaseMetadata` | 将可用性失败与数据非法分开；来源回退不降低校验要求 |
| 版本与产物选择 | 当前版本 + 平台 + 安装身份 → `UpdateInfo` | 保持安装渠道、架构和版本分支，找不到匹配包时明确失败 |
| 下载准备 | `UpdateInfo` → 已验证的准备文件 | 下载和安装分离；限制资源；显示进度；支持取消与错误重试 |
| 平台安装器 | 准备文件 + 目标位置 → 交接/安装结果 | 单独处理文件锁、提权、重启、备份及日志 |
| UI 编排 | 状态机 → 文案与操作 | 等待安装不报安装完成；取消确认不改变运行文件；旧任务不得覆盖新状态 |
| 发布流水线 | 版本资料 + 真实产物 → 清单与说明 | 用同一批产物生成大小和摘要，发布前验证名称集合 |

迁移时可以把 GPUI 的 Context、窗口句柄和通知替换成目标应用的事件接口；保留检查结果、进度、取消令牌和安装类型这些领域数据，不把 UI 对象传入网络解析层。

### 11.2 必须替换或重新确认的参数

- 项目名、可执行文件名、仓库 owner/name、manifest/API/latest/notes 地址及仓库路径白名单。
- 平台架构映射、产物名称规则、完整预期集合和是否有基础/RDP 这类产品分支。
- Manifest schema、版本前缀及预发布规则；客户端和发布脚本必须同步演进。
- Windows AppId、卸载键、当前用户/所有用户安装方式、安装目录、ProductVersion 来源、安装器参数与退出码。
- macOS bundle 名、包 receipt ID、安装位置、plist 版本字段和签名方式。
- Linux 包格式、权限要求、AppImage 类型及是否真正接入系统包管理器。
- 临时目录前缀、应用多实例模型、缓存保留策略与安装日志位置。
- 网络代理、超时、最大下载体积，以及是否改成流式落盘和增量哈希。
- 确认文案、所有 UI 状态及本地化资源；安装阶段必须使用准确的成功定义。

不要把 TinyShell 当前的 12 项产物集合、`-rdp-` 标记或安装标识原样带到另一个应用中。

### 11.3 复用前优先补齐的能力

按目标产品需要决定，避免将尚未实现的能力写进交付承诺：

1. **多文件一致性**：应用带 helper、插件或动态库时，更新整个安装单元或明确兼容矩阵，不能只换主 exe。
2. **完整事务和健康确认**：准备、校验、切换、启动确认、清理分别建模；新进程确认健康后才删除备份。
3. **任务恢复与互斥**：持久化已验证载荷的状态，保护待安装状态，增加每次下载的独占目录与进程间安装锁。
4. **资源控制**：大包采用临时文件流式写入与哈希；独立限制解压大小、文件数、元数据和说明体积。
5. **信任边界**：若需要独立证明发布者身份，设计签名清单、公钥分发和轮换，明确重定向、说明 URL 和本地载荷复验策略。
6. **版本生命周期**：明确频道、重大版本升级、最低支持版本、迁移与回退条件；更新器不应代替业务数据迁移方案。
7. **一致的失败体验**：安装 helper 启动失败、用户取消、需要重启系统和新进程启动失败应分开处理，避免错误的成功或重试提示。

## 12. 验证、验收与排障索引

### 12.1 仓库已存在的回归代码

下表表示用例存在，不表示本次执行通过。

| 位置 | 主要覆盖 |
| --- | --- |
| `src/app/updater/mod.rs` 内测试 | 版本比较、摘要、平台选包、调度时间、临时目录、Windows 脚本、Linux AppImage 路径/替换/回滚 |
| `src/app/updater/release_source.rs` 内测试 | 清单校验、回退状态分类、限流信息、HTML 同条目摘要和说明兼容 |
| `src/app/updater/ui.rs` / `presentation.rs` / `src/app/settings/update.rs` 内测试 | 进度比例、等待安装状态和安装类型文案 |
| [test_update_manifest.py](../../scripts/test_update_manifest.py) | 真实文件摘要、稳定输出、缺少/多余/重名产物 |
| [test_release_notes.py](../../scripts/test_release_notes.py) | 版本一致性、索引与纯文本说明导出 |
| [test_windows_setup_update.py](../../scripts/test_windows_setup_update.py) | PowerShell 实际脚本正文配合边界替身，覆盖向导参数、取消、中止、失败、版本不符与需重启 |

后续获准验证时，可按范围选择：

```bash
python -m unittest scripts.test_update_manifest scripts.test_release_notes
python -m unittest scripts.test_windows_setup_update
cargo fmt --all -- --check
cargo test --workspace --locked --all-targets
cargo clippy --workspace --locked --all-targets -- -D warnings
```

Windows 安装脚本测试需要相应 PowerShell 环境，不能代替真实 Inno Setup 验收。遵循项目约定：本地 Cargo 测试、检查、构建、打包和运行仅在用户明确要求后执行，不因新增本文而自动运行。

### 12.2 后续手动验收清单

| 入口 / 场景 | 操作 | 重点预期 |
| --- | --- | --- |
| 设置 → 在线更新 | 依次设置启动、定时、禁用；重开应用和新增窗口 | 模式生效，禁用不阻止手动检查，启动不重复安排 |
| 更新通知 | 关闭通知后触发自动检查 | 更新状态仍更新，但不自动弹出窗口 |
| 来源回退 | 在隔离测试环境模拟清单 404、403、损坏 JSON，API 限流 | 可用性错误按规则回退；非法内容明确失败 |
| 下载校验 | 模拟错误大小、错误摘要、截断内容 | 不出现可安装成功状态，不覆盖运行程序 |
| 下载取消与重试 | 中途取消并立即重新下载 | 旧任务不覆盖新进度；从头下载；异常信息可见 |
| 等待安装 | 下载完成后取消确认、重新打开窗口、再检查 | 取消不退出；记录重查覆盖待安装状态这一现有限制 |
| Windows 安装版 | 中文/空格路径，向导完成、取消、需要重启及版本不符 | 参数正确；完成页启动一次；取消/失败保留包；双日志可关联 |
| Windows 便携版 | 同目录暂存、文件占用及启动 helper 失败 | 正常路径替换主 exe；异常回滚与暂存丢失情况如实呈现；核对 helper 兼容性 |
| macOS | 分别验证基础/RDP、ZIP/PKG，拒绝授权，非标准位置 bundle | 分支不串包；失败反馈准确；核对安装路径与重启路径 |
| Linux AppImage | 正常启动、继承无关 APPIMAGE、立即启动失败 | 只替换自己的外层镜像；早期失败恢复旧文件 |
| Linux tar.gz / 系统目录 | 可写/不可写目录更新 | 权限不足明确失败；检查旧 helper 和库的兼容性 |
| 所有平台升级后 | 查看版本并检查本地终端、SSH、SFTP、MCP、RDP | 版本符合目标，配置保留；对实际使用功能逐项验收 |

### 12.3 常见问题定位

| 表现 | 优先定位 |
| --- | --- |
| 有 Release 但应用不显示更新 | 当前版本是否更高；固定 latest 入口；清单有效性；安装类型与对应产物是否匹配 |
| API 限流后仍检查失败 | 清单为何不可用；HTML 是否含有效标签和同条目摘要；日志中的 reset 时间 |
| 更新说明空白 | 清单 `notes_url`、`release-notes.txt` 是否存在；可选说明读取失败只记日志 |
| 下载完成后重开应用仍是旧版 | 是否点击安装并确认；普通退出不会安装，下载状态不持久化 |
| Windows 安装后没启动 | 完成页是否勾选启动；这是向导的选择，不由辅助进程强制补启 |
| Windows 安装失败 | `%TEMP%` 中同 suffix 的 `.log` / `.setup.log`；退出码、原目录、ProductVersion；第二份日志可能尚未生成 |
| Linux 提示无法改名或写文件 | 当前 exe/外层 AppImage 所在目录是否可写；`LinuxPackage` 没有自动提权 |
| 主程序已升级但 MCP/RDP 异常 | 是否走仅替换主程序的便携路径；核对 helper / lib 与目标版本是否一致 |

分享日志前应按需脱敏本机路径；不要把用户配置、密码或令牌加入发布说明或排障资料。

### 12.4 本次文档整理的验证范围

- 已完成：逐段核对源码、发布脚本、配置、UI 文案与现有专项文档；记录版本和提交基线。
- 已通过：本文与文档索引的本地文件链接、章节锚点和 Markdown 围栏检查；Git 差异空白检查及新文档行尾空白检查。
- 未执行：Rust 编译、Cargo 测试、Clippy、打包、应用启动、真实下载安装、Python 功能回归及线上 Release 查询；本次请求为提炼文档，没有要求运行这些验证。
- 本文不改变应用行为、版本号或发布状态。后续实现变更时，应同步更新受影响章节和验证记录。

[返回文档索引](../README.md)
