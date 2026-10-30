# 更新清单签名配置

> 状态：Review
> 最后更新：2026-10-30
> 关联：[线路设计](../02-design/update-routes.md)、[验证记录](../04-quality/update-routes.md)、[构建与打包](build-and-package.md)

## 发布前一次性配置

使用 TinyShell 独立的 Ed25519 密钥。Git 跟踪的文件、文档和构建产物只允许包含公钥，不能包含私钥。首次实现未生成生产密钥；2026-10-30 用户明确授权生成并配置，同时要求保存在项目内，实际配置记录见下文。

一般在有 OpenSSL 的受控环境执行下列命令，将路径替换为仓库外新建的私有目录；本项目此次按用户明确要求采用已排除 Git 跟踪的项目内私有目录：

```text
python scripts/sign_update_manifest.py --generate <仓库外的私有目录>
```

脚本输出 `update-signing-seed.hex` 和 `update-public-key.hex`，不在终端打印密钥，不覆盖已有目录。Windows 上文件权限由运行账户和目录 ACL 决定，应使用仅密钥维护者可读的目录；生成后备份到现有安全凭据管理位置。

在 GitHub 仓库 Actions 配置中设置：

| 类型 | 名称 | 值 |
| --- | --- | --- |
| Repository variable | `TINY_SHELL_UPDATE_PUBLIC_KEY` | 公钥文件内的 64 位十六进制文本 |
| Repository secret | `TINY_SHELL_UPDATE_SIGNING_SEED` | 私钥种子文件内的 64 位十六进制文本 |

公钥在各平台编译时通过环境变量固化到应用；私钥只传给 publish job 的签名步骤。标签构建先验证公钥格式；发布签名前还会从种子推导公钥并严格比对，配置缺失或不匹配会停止发布。不要在同次工作流运行期间修改这两个设置。

源码构建可显式设置相同的公钥环境变量。未设置时仍可用 GitHub 权威元数据核验旧更新，但无法在直连失效时独立信任第三方线路。应用不会从用户配置或镜像响应加载信任公钥。

## 发布流程和协议

1. 既有流程构建完整平台产物，生成 `release-notes.txt`。
2. `scripts/update_manifest.py` 保持 schema 1，在兼容现有字段的同时添加 `notes`，使说明也受签名保护。
3. `scripts/sign_update_manifest.py` 使用 OpenSSL 的 Ed25519 对清单**原始完整字节**签名，并在写出前自验签。
4. `update-manifest.json.sig` 为 128 位十六进制签名，可带末尾换行。清单与签名一起作为发布附件上传。
5. 应用使用编译内置公钥校验对应标签下的签名，随后按清单大小和 SHA-256 校验安装包。

不要在签名后重新格式化 JSON、转换行尾或修改说明。旧客户端忽略新增 `notes`，仍可读取 schema 1 清单；旧无签名 Release 通过 GitHub 直连信任链兼容。公开 Release 和附件不得替换，修复必须使用新的版本。

该密钥需要保持稳定。直接替换公钥会使已安装客户端无法验证后续镜像清单；轮换应另行设计双公钥过渡并经过确认。签名不替代 macOS / Windows 平台代码签名，也不提供防旧版本重放的独立新鲜度证明。

## 当前生产配置

2026-10-30 已按用户授权生成 TinyShell 专用密钥，保存在项目根目录 `.private/update-signing/`。`.gitignore` 排除整个 `.private/`，并额外保留私钥文件名忽略规则；父目录、密钥目录及两份密钥文件均关闭权限继承并逐项核对，仅当前账户和 Windows SYSTEM 可访问。生成脚本的临时密钥文件也在该受限目录内处理。

GitHub 仓库已配置 `TINY_SHELL_UPDATE_PUBLIC_KEY` 变量及 `TINY_SHELL_UPDATE_SIGNING_SEED` Secret，远端公钥与本地文件匹配。私钥通过标准输入传给 GitHub CLI，没有作为命令行参数、日志或提交内容输出。公私钥文件均不纳入 Git，克隆项目不会带回这些本地文件，应按现有凭据保管流程备份整个密钥目录。

后续发版复用当前密钥，不能因重复执行发布准备而重新生成或覆盖。公钥与 Secret 在同次工作流运行期间保持稳定。

公钥原始 32 字节的 SHA-256 指纹为 `b6bac8083a8f0db61aef6518d403713cd5a777cac98b39681e4a264c7023da4a`。v2.3.0 已用该密钥签名并正式发布，线上清单的 Ed25519 验签、附件摘要及内嵌说明核对通过；此指纹不包含私钥。

## 运行边界

首次实现仅添加脚本、工作流与客户端支持。后续已完成生产密钥生成与远端配置，v2.3.0 的云端构建、签名和正式发布结果以[版本记录](../upgrade/v2.3.0/README.md)为准。用户要求本次跳过测试，未执行错误公钥、被修改清单和 GitHub 直连不可用等功能回归；实际发布附件的签名仍需核验。

协议实现参考 [ring 的 Ed25519 API](https://docs.rs/ring/0.17.14/ring/signature/index.html)，实际运行结果以验证记录为准。
