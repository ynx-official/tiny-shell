# TinyShell v1.6.2

> 发布日期：2026-08-23

## 版本概述

本版本修复 v1.6.1 引入的 macOS 发布流水线问题。macOS 基础版和 RDP 版的应用打包脚本现在能够正确传递 `.app` 路径，避免 `dylibbundler` 的诊断输出干扰后续压缩和安装包生成，使正式发布可以完整生成 macOS 产物。

## 改进与修复

- 修复 macOS 基础版和 RDP 版在 `Package (macOS / Linux)` 阶段失败的问题。
- 保持打包脚本 stdout 只返回生成的 `.app` 路径，将 `dylibbundler` 的进度和诊断信息转移到 stderr。
- 恢复 macOS Apple Silicon 与 Intel 两个架构的 `.zip` 便携包和 `.pkg` 安装包生成流程。

## 行为与界面变化

应用界面、快捷键和运行时工作流没有变化。本版本主要影响 macOS 正式发布产物的生成与交付。

## 配置与数据兼容性

配置、会话、同步、工作区和更新清单格式没有变化，无需迁移。

## 升级说明

用户可以直接覆盖安装 v1.6.2。需要 RDP 的 macOS 用户继续选择带 `-rdp-` 标识的安装包；不需要 RDP 的用户选择基础版安装包。

## 破坏性变更与已知问题

没有破坏性变更。未发现由本版本修复引入的已知问题；macOS RDP 版仍使用随产物携带的 FreeRDP 运行库，基础版仍不包含 FreeRDP 后端。

## 验证结果

- `bash -n scripts/package-macos-app.sh`：通过。
- `git diff --check`：通过。
- `cargo clippy --locked --all-targets -- -D warnings`：通过。
- `cargo test --locked --all-targets`：通过，449 个测试通过、1 个忽略；构建契约测试通过。
- `cargo build --locked --release`：在版本号升级前的同一源码上通过；版本号升级后的本机重建因 Cargo 并发锁状态无进展而中止。
- `cargo check`：通过，并确认 `Cargo.lock` 中的 `tiny-shell` 版本为 `1.6.2`。
- `cargo fmt --all -- --check`：未通过；目标祖先提交已有 `src/app/updater/mod.rs` 的格式差异，本版本未混入无关格式化修改。
- `python3 scripts/release_notes.py --check-current`：通过。
- `python3 -m unittest scripts/test_release_notes.py scripts/test_update_manifest.py`：本机 Python 3.9 不支持项目使用的 `Path.write_text(..., newline=...)` 参数，未完成；发布 workflow 使用 Python 3.13。
- macOS、Windows、Linux 的正式产物构建将在 `v1.6.2` 标签流水线中完成最终验证。

## 变更依据

- 目标标签：`v1.6.2`。
- 最近祖先版本：`v1.6.1`。
- 比较链接：[v1.6.1...v1.6.2](https://github.com/ynx-official/tiny-shell/compare/v1.6.1...v1.6.2)

[返回版本总览](../README.md)
