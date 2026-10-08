# 应用图标与平台导出

> 状态：Approved
> 最后更新：2026-10-08  
> 关联：[文档索引](../README.md)、[生成脚本](../../scripts/generate-icons.ps1)、[素材回归测试](../../tests/app_icon_contract.rs)

## 品牌约束

保持深海军蓝背景、青绿强调色和白色终端符号。准确保留 `TinyShell`（`Tiny` 青绿、`Shell` 白色）和 `SSH`，保留终端、连接与服务器图形。

平台需要不同的画布规则，不能把带透明留白和外框的单一 PNG 直接缩放为所有平台图标。

## 权威源图与消费路径

| 文件 | 职责 |
| --- | --- |
| `assets/icons/source/tiny-shell-brand.png` | 原有透明品牌母图，Windows/Linux 从这里等比例导出；保留原始内部像素与排版 |
| `assets/icons/source/tiny-shell-macos.png` | macOS 专用正方形、不透明、背景铺满画布的母图 |
| `assets/icons/tiny-shell.png` | 1024px Windows/Linux 窗口图标及 README 标识 |
| `assets/icons/tiny-shell.ico` | Windows EXE 与安装器，包含 16/20/24/32/40/48/64/128/256px |
| `assets/icons/tiny-shell.icns` | macOS App Bundle，包含标准与 Retina 尺寸 |
| `assets/icons/256x256/tiny-shell.png` | Linux Debian/AppImage 使用的 256px 图标 |

macOS 母图使用内置 `imagegen` 编辑原品牌图。提示约束为：保留内部元素、颜色和准确文字，仅移除最外层圆角外壳、描边和外部阴影；将深蓝背景延伸至正方形画布四边与四角，由系统应用最终外形遮罩。

## Windows 与 Linux

先按 Alpha>0 找到透明母图的完整可见范围，再等比例缩放并居中。最长边两侧各预留 2% 安全空间，避免裁切和拉伸字体。

此前 32px 图标可见部分约为 25×27px；新导出为 29×31px。16px 从 12×14px 增为 14×15px。ICO 各尺寸从同一母图分别采样，避免连续缩放损失。

`build.rs` 已跟踪 `assets/icons/tiny-shell.ico`，图标变化会重新编译 Windows 资源。PNG 通过 `include_bytes!` 编入应用；覆盖源文件后需要重新构建并退出旧进程再启动。已安装的旧 EXE 和旧安装包不会随工作区素材变化自动更新。

当前 GPUI Windows 后端通过 `LoadImageW` 从 EXE 的资源 ID 1 加载原生窗口类图标。因此 Windows 尺寸修复需要更新 ICO 并完成 EXE 重新链接；仅修改窗口选项中的 PNG 不足以证明原生标题栏与任务栏已经使用新素材。

## macOS

用户反馈发生在 macOS Tahoe 26 或更新版本。该系统会调整旧图标的外形并添加材质；原母图的透明留白、非正方形外壳和自带描边可能与系统效果叠加。[Apple 旧图标适配说明](https://developer.apple.com/videos/play/wwdc2025/220/)

新的 macOS 母图为正方形满画布深蓝背景，不预先烘焙外层圆角框、透明边距或外部阴影。保留终端窗口和 SSH 徽章的内部层级。系统正常的材质高光仍可能存在；本变更不尝试禁用 macOS 的系统效果。

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

包含窗口 PNG 与 ICO 各尺寸的可见占比、透明四角、ICNS RLE 解码边界、ARGB/PNG 色彩逐像素一致性、macOS 背景不透明性和四角深蓝色。CI 的 macOS 门禁额外使用系统 `iconutil` 解码真实 ICNS。

## 验证边界

本地格式检查、Clippy（warnings 视为错误）与 `cargo test --locked --all-targets` 已通过；新增四项图标测试先在旧素材上失败，换入新素材后通过。`cargo build --locked --release` 与 `cargo build --locked` 均通过。

通过 Windows `LoadLibraryExW`（只映射资源，不执行应用）和 `LoadImageW` 从 release EXE 的资源 ID 1 加载 16/32/48/64px 图标，与项目 ICO 逐像素比较，四档差异均为 0；可见范围分别为 14×15、29×31、43×46、57×61px。

生成脚本再次输出至独立目录后，四个消费素材的 SHA-256 与已安装素材一致，导出可重复。

用户已确认本次图标优化无问题并批准正式发布。本机为 Windows；代理已验证文件像素、容器与 Windows 构建。CI 的原生 macOS 解码门禁需后续运行，不能视为已通过。代理未在 macOS 26+ 或 macOS 12–15 实机运行应用；颜色、拼写与画布调整已通过本地视觉检查。

macOS 在旧版本上可能保留方形图标外形；本轮针对用户确认的 26+ 系统适配。若需针对旧版保持独立圆角外形，应增加受系统版本控制的资源方案，而不是给当前满画布图像重新套外框。
