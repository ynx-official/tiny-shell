# Rust 1.96.1 升级验证

> 状态：Review  
> 最后更新：2026-10-10  
> 关联：[构建与打包](../05-operations/build-and-package.md)、[MCP 功能设计](../02-design/mcp-integration.md)、[文档索引](../README.md)

本报告记录工具链升级时的代码快照。随后新增的 MCP 实现与测试以 [MCP 实施与验收](mcp-implementation.md) 为准，不能把本报告的 533 项测试视为 MCP 功能验证。

项目开发、CI 和发布工具链统一固定为 Rust 1.96.1，`Cargo.toml` 的最低版本声明同步更新。Windows 的全部 533 项测试、严格 Clippy、代码覆盖率测试和 release 构建已通过。现有测试的总体行覆盖率为 29.61%，原生 UI 与真实网络操作仍存在覆盖缺口。

## 版本选择与变更范围

截至 2026-10-10，官方最新稳定版本为 Rust 1.99.0，发布于 2026-10-01。[官方发布公告](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/)

用户选择固定 Rust 1.96.1，接近本机此前使用的 1.96.0；1.96.1 包含编译器、Cargo HTTP 与其内置 libssh2 的修复。[Rust 1.96.1 发布说明](https://blog.rust-lang.org/2026/06/30/Rust-1.96.1/)

变更包括：

- `Cargo.toml` 最低版本设为 1.96.1，新增 `rust-toolchain.toml`，固定版本并配置 rustfmt 与 Clippy。
- CI 的四处工具链安装和发布流水线的一处安装均固定为 1.96.1。
- 同步项目规范、中英文 README、构建说明和 MCP 设计中的 Rust 要求。
- 修正两处构建脚本 Clippy 提示和 44 处源码提示，使用等价的 let-chain 与 `is_multiple_of` 写法。保持条件求值顺序和短路行为，测试及断言没有修改。

应用版本保持 1.6.10，`Cargo.lock` 的依赖解析与升级前一致，没有更新 GPUI 或增加新的 vendor 补丁。尚未提交、推送或发布应用版本。

Rust 1.88 的实际检查曾被 `smol_str 0.3.6` 的最低版本与数组长度推断阻止；处理这一处后，GPUI 又因 `str::ceil_char_boundary` 在该工具链中未稳定而无法编译。最终采用用户选择的 1.96.1，不继续维护这些兼容补丁。

## 验证环境

- 本机平台：Windows x86_64，MSVC 目标。
- `rustc 1.96.1 (31fca3adb 2026-06-26)`。
- `cargo 1.96.1 (356927216 2026-06-26)`。
- 覆盖率工具：cargo-llvm-cov 0.9.1 与 Rust 1.96.1 的 llvm-tools-preview。
- Rust 构建与测试使用原锁文件及 `--locked --offline`；普通、覆盖率和历史工具链验证缓存分开存放于 `target/`。
- Python 发布工具测试使用本机 Python 3.14.6。

## 实际验证结果

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | 最终源码通过；没有放宽 warning 门禁 |
| `cargo test --locked --offline --all-targets -- --include-ignored` | 533 通过，0 失败，0 忽略 |
| 同一组测试的 LLVM 覆盖率构建 | 533 通过，0 失败，0 忽略 |
| Windows `cargo build --locked --offline --release` | 通过，完整构建用时 5 分 41 秒 |
| Windows 发布参数 `cargo build --locked --offline --release --no-default-features` | 通过，使用缓存后用时 1 分 9 秒 |
| Python 发布说明与更新清单测试 | 8 项通过 |
| `python scripts/release_notes.py --check-current` | 通过，应用版本仍为 v1.6.10 |
| Cargo 最低版本、项目工具链与五处流水线安装的一致性 | 通过，均为 1.96.1 |
| 四个平台的已解析依赖最低 Rust 声明检查 | 均未发现要求高于 1.96.1 的依赖 |

Rust 测试按目标分布如下；显式运行了原有手动终端性能基准。

| 测试目标 | 通过数量 |
| --- | --- |
| 主程序单元测试 | 511 |
| 应用图标契约 | 6 |
| 构建脚本契约 | 1 |
| UI 国际化契约 | 2 |
| 统一确认弹窗示例 | 11 |
| 常用命令标题示例 | 2 |
| 合计 | 533 |

平台依赖声明检查覆盖 Windows x86_64 的 660 个包、Linux x86_64 的 741 个包、macOS Apple Silicon 的 673 个包及 macOS Intel 的 674 个包。该检查验证声明和解析结果，不能代替对应平台实际编译。

## 覆盖率结果与解释

报告包含当前 Windows 测试执行到的项目 Rust 代码及构建脚本，排除第三方依赖和已有 vendor 源码。项目源码中的内联测试函数仍计入 LLVM 的统计，因此这些数字不能作为独立的生产代码覆盖率。

| 指标 | 已覆盖 | 总数 | 覆盖率 |
| --- | --- | --- | --- |
| 行 | 18,743 | 63,295 | 29.61% |
| 函数 | 1,937 | 6,193 | 31.28% |
| 区域 | 28,697 | 98,655 | 29.09% |

| 模块 | 行覆盖率 |
| --- | --- |
| `src/crypto/` | 92.54% |
| `src/sync/` | 84.36% |
| `src/docker.rs` | 80.35% |
| `src/session/` | 76.72% |
| `src/system/` | 53.32% |
| `src/terminal/` | 48.23% |
| `src/backend/` | 24.29% |
| `src/sftp/` | 19.07% |
| `src/app/` | 15.35% |
| `build.rs` | 9.19% |

稳定工具链没有启用分支覆盖率或 MC/DC，JSON 中对应的零计数表示未采集，不能解读为实测 0% 覆盖。当前数据反映现有测试：原生启动、窗口交互、真实 SSH / SFTP I/O 和部分对话框缺少运行覆盖。

[HTML 覆盖率报告](../../target/rust-1.96.1-validation/coverage/html/index.html)可逐文件查看命中与未命中行；[JSON 汇总](../../target/rust-1.96.1-validation/coverage-summary.json)与[模块汇总](../../target/rust-1.96.1-validation/coverage-modules.json)供后续比较。报告位于本机被忽略的 `target/` 中，不提交到仓库；删除构建缓存后需要重新生成。

## 验证记录与复现

完整记录保存在本机：

- [最终普通测试日志](../../target/rust-1.96.1-validation/tests.log)。
- [最终严格 Clippy 日志](../../target/rust-1.96.1-validation/clippy.log)。
- [最终覆盖率测试日志](../../target/rust-1.96.1-validation/coverage-tests.log)。
- [release 构建日志](../../target/rust-1.96.1-validation/release.log)。
- [Windows 发布参数构建日志](../../target/rust-1.96.1-validation/release-windows-native-rdp.log)。
- [验证输入的 SHA-256 快照](../../target/rust-1.96.1-validation/input-hashes.json)。

普通测试和 Clippy 使用 `--target-dir target/rust-1.96.1`，覆盖率通过 `CARGO_LLVM_COV_TARGET_DIR` 使用 `target/rust-1.96.1-coverage`。两个测试构建都传递 `--include-ignored`，不会保留原有手动性能基准的默认忽略状态。

最终复核了 226 个构建和验证输入的 SHA-256，与最终测试前快照一致。Windows release EXE 的 ProductVersion 为 1.6.10。

覆盖率报告通过 `cargo llvm-cov report --include-build-script` 生成 JSON 和 HTML，并使用文件名过滤排除 `vendor/`；没有删除未覆盖的应用源文件来提高比例。常规复现命令见[构建与打包](../05-operations/build-and-package.md#代码覆盖率)。

## 尚未验证的范围

- macOS / Linux 原生测试、release 构建及 FreeRDP 联调：本机为 Windows；流水线配置已更新，但尚未通过推送触发运行。
- Windows 实际窗口交互、真实服务器 SSH / SFTP 和安装包执行：本次验证没有进行这些操作。
- MCP 运行时与客户端接入：MCP 仍处于功能设计阶段，没有对应的已实现代码可测试。

后续补充覆盖可优先针对原生窗口生命周期、SSH / SFTP 服务边界与故障恢复；这些需要独立的 UI 或隔离服务测试，不能用现有单元测试通过数量代替。
