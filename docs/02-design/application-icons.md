# 应用图标与平台导出

> 状态：Approved
> 最后更新：2026-10-09
> 关联：[文档索引](../README.md)、[生成脚本](../../scripts/generate-icons.ps1)、[素材回归测试](../../tests/app_icon_contract.rs)

## 品牌约束

统一使用用户于 2026-10-09 最终确认的无文字黑白终端窗口图标：黑色底、居中的白色终端窗口轮廓与粗线条 `>_`。只保留这一个终端标识，不显示 `TinyShell` 名称、`SSH` 文字、徽章、代码行或其他内容。去掉彩色控制点、服务器、连接虚线、立体渐变与阴影，不修改软件内部的功能按钮图标或 Sentry UI 主题。

本次用户明确选择终端窗口方案，进一步取代当天已落地的带 `TinyShell`/`SSH` 文字的黑白稿；该稿不再是当前品牌约束。[v1.6.9 发布详情](../upgrade/v1.6.9/README.md) 保留已发布的深蓝/青绿方案和验证记录。工作区替换不等于重新发布 v1.6.9，既有公开产物与标签保持不变。

平台需要不同的画布规则，不能把带透明留白和外框的单一 PNG 直接缩放为所有平台图标。

## 权威源图与消费路径

| 文件 | 职责 |
| --- | --- |
| `assets/icons/source/tiny-shell-brand.png` | 用户确认的黑白透明母图，Windows/Linux 从这里等比例导出 |
| `assets/icons/source/tiny-shell-macos.png` | 同一黑白母图的 macOS 输入副本；导出时合成至不透明黑底 |
| `assets/icons/tiny-shell.png` | 1024px Windows/Linux 窗口图标及 README 标识 |
| `assets/icons/tiny-shell.ico` | Windows EXE 与安装器，包含 16/20/24/32/40/48/64/128/256px |
| `assets/icons/tiny-shell.icns` | macOS App Bundle，包含标准与 Retina 尺寸 |
| `assets/icons/256x256/tiny-shell.png` | Linux Debian/AppImage 使用的 256px 图标 |

黑白母图由内置 `imagegen` 生成，直接采用用户最终确认的终端窗口方案，不再重新绘制。提示要点：简洁黑白扁平应用图标、居中的白色终端窗口轮廓与 `>_`、粗线条和清晰负空间；不带名称、文字、徽章、代码内容、控制点、装饰、渐变、阴影或额外外框，外部透明。

导出时采用中性灰阶色彩模式，消除生成图白色边缘中极轻微的 RGB 色偏，保留图形比例与排版。Windows/Linux 保留透明轮廓，macOS 按下述规则合成不透明底。两个平台输入副本保持与用户最终确认的原始生成 PNG 字节一致。

## Windows 与 Linux

先按 Alpha>0 找到透明母图的完整可见范围，再等比例缩放并居中。最长边两侧各预留 2% 安全空间，避免裁切和拉伸字体。

ICO 各尺寸从同一母图分别采样，避免连续缩放损失。继续要求可见主体占用足够画布、保留紧凑留白，不回退到此前图标偏小的比例。

16px 导出在采样后使用 1.2 倍灰阶对比补偿，让亚像素宽的窗口轮廓和提示符保持清晰；只调整这一档的亮度，不改变母图或图形几何。Windows/Linux 导出显式清空四个最外层角像素，避免 16px 的小数留白产生半透明采样残留；macOS 仍保持全不透明。

`build.rs` 已跟踪 `assets/icons/tiny-shell.ico`，图标变化会重新编译 Windows 资源。PNG 通过 `include_bytes!` 编入应用；覆盖源文件后需要重新构建并退出旧进程再启动。已安装的旧 EXE 和旧安装包不会随工作区素材变化自动更新。

当前 GPUI Windows 后端通过 `LoadImageW` 从 EXE 的资源 ID 1 加载原生窗口类图标。因此 Windows 尺寸修复需要更新 ICO 并完成 EXE 重新链接；仅修改窗口选项中的 PNG 不足以证明原生标题栏与任务栏已经使用新素材。

## macOS

用户反馈发生在 macOS Tahoe 26 或更新版本。该系统会调整旧图标的外形并添加材质；原母图的透明留白、非正方形外壳和自带描边可能与系统效果叠加。[Apple 旧图标适配说明](https://developer.apple.com/videos/play/wwdc2025/220/)

macOS 从正方形黑白母图导出，各尺寸合成至不透明满画布纯黑背景，不保留外层透明边距或额外圆角外壳；终端的白色轮廓属于内部标识而非外部图标罩子。系统正常的材质高光仍可能存在；本变更不尝试禁用 macOS 的系统效果。

16px/32px 普通尺寸分别使用 `ic04`/`ic05` 的 ARGB 数据：`ARGB` 标记后是直通 Alpha 的 A/R/G/B 四个平面，以 ICNS RLE 字面量包编码。Retina 与较大尺寸采用 PNG：`ic11`、`ic12`、`ic07`、`ic13`、`ic08`、`ic14`、`ic09`、`ic10`。不向小尺寸 ARGB 槽位填入 PNG。[编码兼容性参考](https://github.com/electron-userland/electron-builder-binaries/blob/master/packages/icons/CHANGELOG.md)

## 重新生成与验证

在 Windows PowerShell 中运行，无需安装图像处理依赖：

```powershell
./scripts/generate-icons.ps1
```

先输出到独立目录进行检查：

```powershell
./scripts/generate-icons.ps1 -OutputDirectory target/icon-preview
```

回归测试检查实际图片内容与容器编码，而非只检查扩展名：

```powershell
cargo test --locked --test app_icon_contract
```

包含窗口 PNG、Linux PNG 与 ICO 各尺寸的可见占比、透明四角、所有导出尺寸的黑白灰阶及对比度、ICNS RLE 解码边界、ARGB/PNG 色彩逐像素一致性、macOS 背景不透明性和纯黑四角。无文字版额外要求窗口图标、Linux PNG 与 macOS 1024px 图标的底部留白保持空白，防止旧版底部名称混入；这项检查不等于 OCR，徽章移除还需视觉复核。CI 的 macOS 门禁额外使用系统 `iconutil` 解码真实 ICNS。

## 验证边界

用户已确认无文字终端窗口设计并授权替换所有平台的应用素材。以下为版本升级前的图标落地验证结果，不沿用此前带文字黑白稿的构建或通过记录；正式发布结果以对应版本详情为准。

- 新增的底部名称回归检查在替换前失败，成功识别旧黑白稿底部残留的 `TinyShell`。
- 首次直接导出的现有检查另发现 16px 的两个问题：macOS 最亮像素仅为 217/255，Windows 一个角像素 Alpha 为 25/255。通过小尺寸对比补偿和透明角像素归零修正，未弱化原有对比度或透明性断言。
- `cargo test --locked --test app_icon_contract`：六项图标检查全部通过。
- `cargo fmt --all -- --check`、`cargo clippy --locked --all-targets -- -D warnings`：均通过。
- `cargo test --locked --all-targets`：532 项通过、0 项失败；保留项目已有的 1 项手动性能基准忽略标记，没有新增跳过项。
- Windows `cargo build --locked --release` 与 `cargo build --locked`：均通过。
- 仅映射 Windows debug/release EXE 的资源、不执行应用；从资源 ID 1 读取 16/32/48/64/256px 原生图标，与项目 ICO 原生解码像素逐点比较，两种构建的五个尺寸差异均为 0，确认内嵌图标已更新。
- 两个平台输入副本与用户最终确认的内置生图 PNG 字节一致（SHA-256：`37072005f3b84f26bc2573cfa72b1d63031f450498893fa2ecc04beac489177b`）。
- 生成脚本再次输出到独立目录，四个消费素材的 SHA-256 均与工作区素材一致，导出可重复。
- Windows/Linux PNG 与从 ICNS 提取的 macOS PNG 已进行视觉检查，只有终端窗口轮廓和 `>_`，没有名称或 SSH 徽章。

本机为 Windows，无法在此执行 macOS `iconutil` 或验证 macOS/Linux 原生桌面实际显示；常规原生图标 CI 尚未运行。平台特定的原生验证需要在对应系统执行，发布流水线的构建成功也不等于验证了原生桌面外观。

macOS 在旧版本上可能保留方形图标外形；本轮针对用户确认的 26+ 系统适配。若需针对旧版保持独立圆角外形，应增加受系统版本控制的资源方案，而不是给当前满画布图像重新套外框。
