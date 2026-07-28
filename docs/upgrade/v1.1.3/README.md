# TinyShell v1.1.3

> 发布日期：2026-07-25

## 版本概述

修复跨平台发布流程解析 Cargo 包版本失败的问题。

## 更新内容

- 兼容 `cargo pkgid` 的不同输出格式。
- 避免 Windows 发布流水线因版本解析差异中断。
- 保持发布标签、应用版本和更新器识别结果一致。

## 升级说明

该版本未记录配置格式破坏性变更，可直接覆盖安装。升级前建议关闭正在运行的 TinyShell 窗口，并保留现有配置备份。

## 变更依据

- 当前标签：[v1.1.3](https://github.com/ynx-official/tiny-shell/releases/tag/v1.1.3)
- 最近祖先版本：`v1.1.2`
- 代码比较：[v1.1.2...v1.1.3](https://github.com/ynx-official/tiny-shell/compare/v1.1.2...v1.1.3)

[返回版本总览](../README.md)
