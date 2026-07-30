# TinyShell v1.1.9

> 发布日期：2026-07-30

## 版本概述

本版本统一发布日志的数据来源和自动化流程：版本详情文档继续承载完整说明，根目录 `CHANGELOG.md` 提供精简索引，GitHub Actions 在构建和发布前校验版本资料，并自动将对应详情转换为 GitHub Release Notes。

## 改进与修复

- 新增符合 Keep a Changelog 结构的 `CHANGELOG.md`，集中展示版本日期、用户可见摘要和详情链接。
- 新增独立发布资料脚本，校验 Git 标签、`Cargo.toml`、版本详情、版本总览和 `CHANGELOG.md` 的版本及日期一致性。
- 将 GitHub Release 正文从“按提交标题自动分类”调整为“从版本详情文档提取”，避免提交语言、粒度和格式差异影响用户看到的更新说明。
- 为发布资料脚本增加单元测试，覆盖正常生成、标签版本不一致和缺少 CHANGELOG 条目等失败路径。
- 在 CI 和标签发布工作流中增加发布资料门禁；资料不完整或版本不一致时，在多平台构建和创建 Release 前失败。
- 移除中英文 README 中容易过期的重复版本摘要，仅保留指向 `CHANGELOG.md` 的导航入口。

## 行为与界面变化

- 应用运行行为和用户界面没有变化。
- 推送发布标签后，GitHub Release Notes 自动使用 `docs/upgrade/<tag>/README.md` 中从“版本概述”开始的用户可见章节。
- “验证结果”“变更依据”和返回导航不会进入 GitHub Release 正文。
- `CHANGELOG.md` 只作为精简索引和一致性校验项，不复制完整版本详情。

## 配置与数据兼容性

- 应用配置、连接、分组、密钥、同步、SFTP、会话和快捷键数据格式均未变化。
- 不需要迁移本地配置或用户数据，可以回退到 `v1.1.8`。
- 发布自动化使用 Python 标准库，不增加应用运行时依赖。

## 升级说明

可以直接覆盖安装，不需要额外迁移或配置操作。本版本主要调整仓库发布流程，已安装用户的使用方式保持不变。

## 破坏性变更与已知问题

- 本版本没有应用功能、配置或数据层面的破坏性变更。
- 当前本地 Windows 环境未安装 `python` 或 `py` 命令，发布资料脚本的单元测试和实际提取由 GitHub Actions 的 Python 3.13 环境执行。

## 验证结果

已在 Windows 本地完成以下验证：

- `cargo check`：通过，并将 `Cargo.lock` 中的 `tiny-shell` 版本同步为 `1.1.9`。
- `cargo check --locked`：通过。
- `cargo fmt --all -- --check`：通过。
- `cargo clippy --locked --all-targets -- -D warnings`：通过，无 warning。
- `cargo test --locked --all-targets`：通过，152 项测试全部成功。
- `cargo build --locked --release`：通过，生成 Windows 优化构建。
- `git diff --check`：通过。

Python 发布资料测试未在本地执行，因为当前环境没有可用的 Python 解释器；`.github/workflows/ci.yml` 与 `.github/workflows/release.yml` 均固定使用 Python 3.13 执行测试和资料校验。macOS 与 Linux 构建由标签触发的 GitHub Actions 发布流水线验证。

## 变更依据

- 当前标签：[v1.1.9](https://github.com/ynx-official/tiny-shell/releases/tag/v1.1.9)
- 最近祖先版本：`v1.1.8`
- 代码比较：[v1.1.8...v1.1.9](https://github.com/ynx-official/tiny-shell/compare/v1.1.8...v1.1.9)

[返回版本总览](../README.md)