# 应用图标与平台导出

> 状态：Approved
> 最后更新：2026-10-13
> 关联：[文档索引](../README.md)、[选择与完整提示](../../assets/icons/candidates/2026-10-13/manifest.json)、[生成脚本](../../scripts/generate-icons.ps1)、[素材回归测试](../../tests/app_icon_contract.rs)

## 品牌约束与选择依据

用户于 2026-10-13 上传八款候选对比图，并明确要求「使用这里的03 更新为新图标icon」。当前图标为附图上排第三款 03「TS 字母」：黑色圆角底块、白色几何 TS 字母组合、T 上横线与 S 之间的斜切负空间，以及 S 的斜向中段和宽底边。

此次选择以[用户提供的对比图](../../assets/icons/candidates/2026-10-13/selection-board.png)为准；另一张六款语义方案中的 03「汇流」不是本次选定的图标。图块内只保留 TS 标识，不包含候选编号、应用全称、SSH 徽章、终端窗口、提示符或其他装饰。保持黑白灰阶，不添加彩色点、渐变、阴影或材质；软件内部功能图标与 Sentry UI 主题沿用既有约定。

新标识替代 2026-10-10 选定的 A「圆润终端」。用户明确要求替换品牌图标是本次变更依据；原约束、历史验证及平台背景保留在[圆润终端归档](../99-archive/application-icons-rounded-terminal-2026-10-10.md)，三套旧候选继续保存在[历史候选记录](application-icon-candidates.md)。本次未修改版本号、提交或创建发布标签。

## 权威源图与消费路径

| 文件 | 职责 |
| --- | --- |
| `assets/icons/candidates/2026-10-13/selection-board.png` | 用户明确选择 03 的原始对比图 |
| `assets/icons/candidates/2026-10-13/ts-monogram.png` | 基于选定图案制作的原始透明母图 |
| `assets/icons/candidates/2026-10-13/manifest.json` | 选择、完整生成提示、母图摘要与验证范围 |
| `assets/icons/source/tiny-shell-brand.png` | Windows/Linux 使用的当前透明母图 |
| `assets/icons/source/tiny-shell-macos.png` | 同一母图的 macOS 输入副本，导出时合成至不透明黑底 |
| `assets/icons/tiny-shell.png` | 1024px 运行时窗口图标及英文 README 标识 |
| `assets/icons/tiny-shell.ico` | Windows EXE 与安装器，包含 16/20/24/32/40/48/64/128/256px |
| `assets/icons/tiny-shell.icns` | macOS App Bundle，包含标准与 Retina 尺寸 |
| `assets/icons/256x256/tiny-shell.png` | Linux Debian/AppImage 的 256px 图标及中文 README 头图输入 |
| `assets/readme/tiny-shell-header.svg` | 内嵌 PNG 与当前 256px 图标字节一致的中文 README 头图 |

使用内置 `imagegen`，以用户附图上排第三款为唯一目标，制作单个透明背景图标。提示要求保留原 TS 的组合、斜切间隙、比例和粗笔画，移除对比图文字与其他候选；黑色底块之外透明。完整提示见产物清单。

原始母图实际画布为 1254×1254px。两个平台输入副本与留存母图字节一致，SHA-256：`c2c315b60db222185b8f8d1662dfb7465cd5d1c9af2c14d9b819961c9fdb51cc`。导出脚本消除边缘极轻微的 RGB 色偏，生成中性灰阶素材。

## Windows 与 Linux

沿用现有脚本：按 Alpha>0 查找母图可见范围，等比例缩放居中，最长边两侧预留 2% 安全空间。所有 ICO 尺寸从母图独立采样，避免连续缩放损失。

16px 继续使用现有 1.2 倍灰阶对比补偿，保持字母细节在采样后的可读性；不改变图形几何。Windows/Linux 四个最外层角像素保持透明。

`build.rs` 已跟踪 ICO，运行时 PNG 通过 `include_bytes!` 编入应用；替换素材后需要重新构建并退出旧进程再启动，已安装的旧 EXE 与旧安装包不会自动变化。Windows 原生窗口类图标来自 EXE 资源，因此仅查看新 PNG 不代表任务栏或标题栏已使用新图标。

## macOS

沿用现有平台策略：正方形母图合成至全不透明的纯黑满画布，不额外烘焙圆角外壳或外部描边，由系统处理图标形状与材质。

16/32px 普通槽位 `ic04`/`ic05` 使用直通 Alpha 的 A/R/G/B 平面与 ICNS RLE 编码。Retina 和较大槽位 `ic11`、`ic12`、`ic07`、`ic13`、`ic08`、`ic14`、`ic09`、`ic10` 使用 PNG，保持原编码约定。

## 重新生成

在 Windows PowerShell 中执行，无需编译应用或安装图像处理依赖：

```powershell
./scripts/generate-icons.ps1
```

可通过 `-OutputDirectory target/icon-preview` 输出到独立目录；该参数只改变输出目录，输入仍来自当前 `assets/icons/source/`。脚本不修改 README 头图；正式图标更新时，必须另行同步其中内嵌的 256px PNG。

素材回归测试检查可见占比、透明四角、灰阶与黑白对比、ICNS 编码及不透明画布、底部无额外名称等约束。运行 `cargo test --locked --test app_icon_contract` 会触发编译，只能在用户明确要求测试时执行。

## 2026-10-13 验证范围

- 使用现有导出脚本生成 Windows/Linux PNG、ICO 和 macOS ICNS，并更新 README 头图。
- 不触发编译的 System.Drawing 像素与容器检查共 23 项通过：窗口与 Linux PNG、ICO 九档尺寸、ICNS 八个 PNG 槽位与两个 ARGB 槽位、README 内嵌图片、三个母图副本。
- 检查沿用现有素材测试的可见占比、透明边角、灰阶和对比度、82% 高度以下无额外白色内容、不透明 macOS 画布、ARGB RLE 边界与 32px ARGB/PNG 逐像素一致性等要求；未降低断言阈值。
- Windows/Linux 1024px PNG 与 ICNS 内实际 1024px PNG 已进行视觉复核，内容为选定 TS 字母标识。
- `cargo fmt --all -- --check`、`git diff --check` 通过；产物清单与当前、归档图标文档的本地链接全部有效。旧版测试和构建结果均不作为本次验证证据。

未执行 Rust 测试、Clippy、编译、应用启动与安装器打包：用户未要求，遵循项目本地验证约定。当前环境为 Windows，macOS/Linux 原生桌面外观尚未验证。

## 重点手动验收

1. 查看当前 1024px PNG、256px PNG 与 README 头图，确认图案为所选 TS 字母，斜切间隙、比例、留白正确，没有旧终端标识或候选编号。
2. 用户自行构建后，退出旧进程再启动，检查 Windows 标题栏、任务栏、开始菜单与安装器；16/32/48px 下 TS 应可辨认，边角透明且无白底方框。若系统显示旧图标，区分旧程序与系统图标缓存。
3. 在 macOS 检查 Dock、Finder 和安装后的 App，在 Linux 检查应用启动器及窗口；应使用相同 TS 标识，macOS 由系统处理外形。保留现有窗口与安装流程的检查。
