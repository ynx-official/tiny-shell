# 更新清单签名配置

> 状态：Review
> 最后更新：2026-10-29
> 关联：[线路设计](../02-design/update-routes.md)、[验证记录](../04-quality/update-routes.md)、[构建与打包](build-and-package.md)

## 发布前一次性配置

使用 TinyShell 独立的 Ed25519 密钥。仓库、文档和构建产物只允许包含公钥，不能包含私钥。此次实现没有生成生产密钥，也没有修改远端仓库设置。

在有 OpenSSL 的受控环境执行下列命令，将路径替换为仓库外新建的私有目录：

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

## 运行边界

本次仅添加脚本、工作流与客户端支持，未执行生产密钥生成、远端配置、构建、打包、标签推送或发布。首次发布前需配置上述变量和 Secret，并在用户授权的构建验证中测试正确签名、错误公钥、被修改清单和 GitHub 直连不可用的情况。

协议实现参考 [ring 的 Ed25519 API](https://docs.rs/ring/0.17.14/ring/signature/index.html)，实际运行结果以验证记录为准。
