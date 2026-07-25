use gpui::{Context, Entity, SharedString};
use gpui_component::input::InputState;
use rust_i18n::t;

use crate::{
    TinyShell, crypto,
    session::config::hardware_uuid,
    sync::{
        self, MergedSecrets, SecretScrubber, SyncBackendCredentials, SyncCredentials, SyncPayload,
        SyncResult, UploadMode,
    },
    terminal::BackendEvent,
};

use base64::Engine as _;

const PRIVACY_PASSWORD_MIN_LEN: usize = 8;

impl TinyShell {
    fn sync_input_value(input: &Entity<InputState>, cx: &Context<Self>) -> String {
        input.read(cx).value().trim().to_string()
    }

    fn sync_credentials(&self, cx: &Context<Self>) -> SyncCredentials {
        let backend = if self.config.sync_backend() == "s3" {
            SyncBackendCredentials::S3 {
                endpoint: Self::sync_input_value(&self.sync_s3_endpoint_input, cx),
                region: Self::sync_input_value(&self.sync_s3_region_input, cx),
                bucket: Self::sync_input_value(&self.sync_s3_bucket_input, cx),
                object_key: Self::sync_input_value(&self.sync_s3_object_key_input, cx),
                access_key: Self::sync_input_value(&self.sync_s3_access_key_input, cx),
                secret_key: self.sync_s3_secret_key_input.read(cx).value().to_string(),
                session_token: self
                    .sync_s3_session_token_input
                    .read(cx)
                    .value()
                    .to_string(),
            }
        } else {
            SyncBackendCredentials::WebDav {
                endpoint: Self::sync_input_value(&self.sync_endpoint_input, cx),
                username: Self::sync_input_value(&self.sync_username_input, cx),
                password: self.sync_webdav_password_input.read(cx).value().to_string(),
            }
        };
        SyncCredentials {
            backend,
            encryption_password: self
                .sync_encryption_password_input
                .read(cx)
                .value()
                .to_string(),
        }
    }

    /// 当前隐私信息加密密码（仅内存，不落盘明文）。
    fn sync_privacy_password(&self, cx: &Context<Self>) -> String {
        self.sync_privacy_password_input
            .read(cx)
            .value()
            .to_string()
    }

    fn begin_sync(
        &mut self,
        status: SharedString,
        cx: &mut Context<Self>,
    ) -> Option<SyncCredentials> {
        if self.sync_in_progress {
            return None;
        }
        let credentials = self.sync_credentials(cx);
        match &credentials.backend {
            SyncBackendCredentials::WebDav {
                endpoint, username, ..
            } => {
                self.config
                    .set_sync_connection(endpoint.clone(), username.clone());
            }
            SyncBackendCredentials::S3 {
                endpoint,
                region,
                bucket,
                object_key,
                ..
            } => {
                self.config.set_sync_s3_connection(
                    endpoint.clone(),
                    region.clone(),
                    bucket.clone(),
                    object_key.clone(),
                );
            }
        }
        if let Err(err) = self.config.save() {
            self.sync_status = format!("{}: {err:#}", t!("sync_failed")).into();
            cx.notify();
            return None;
        }
        self.sync_in_progress = true;
        self.sync_status = status;
        cx.notify();
        Some(credentials)
    }

    pub(crate) fn set_sync_backend(&mut self, backend: &str, cx: &mut Context<Self>) {
        self.config.set_sync_backend(backend);
        let _ = self.config.save();
        self.sync_status = t!("sync_not_run").into();
        cx.notify();
    }

    /// 切换"同步密码和密钥"开关。
    pub(crate) fn set_sync_include_secrets(&mut self, include: bool, cx: &mut Context<Self>) {
        self.config.set_sync_include_secrets(include);
        let _ = self.config.save();
        self.sync_status = t!("sync_not_run").into();
        cx.notify();
    }

    pub(crate) fn upload_sync_config(&mut self, cx: &mut Context<Self>) {
        let Some(credentials) = self.begin_sync(t!("sync_uploading").into(), cx) else {
            return;
        };

        let include_secrets = self.config.sync_include_secrets();
        let privacy_password = self.sync_privacy_password(cx);

        // 勾选了同步密码但隐私密码不达标：中止上传，避免把脱敏数据当加密数据传错
        if include_secrets && privacy_password.len() < PRIVACY_PASSWORD_MIN_LEN {
            self.sync_in_progress = false;
            self.sync_status = t!("sync_privacy_password_required").into();
            cx.notify();
            return;
        }

        // 首次启用密码同步：把隐私密码硬件绑定加密后落盘 + 记录哈希
        if include_secrets && self.config.sync_secrets_password_sealed().is_empty() {
            match seal_privacy_password(&privacy_password) {
                Ok((sealed, hash)) => {
                    self.config.set_sync_secrets_password_sealed(sealed);
                    self.config.set_sync_secrets_password_hash(hash);
                    let _ = self.config.save();
                }
                Err(err) => {
                    self.sync_in_progress = false;
                    self.sync_status = format!("{}: {err:#}", t!("sync_failed")).into();
                    cx.notify();
                    return;
                }
            }
        }

        let sessions = self.config.sessions().to_vec();
        let managed_keys = self.config.managed_keys().to_vec();
        let scrub_result =
            SecretScrubber::scrub(sessions, managed_keys, include_secrets, &privacy_password);
        let (scrubbed_sessions, scrubbed_keys) = match scrub_result {
            Ok(v) => v,
            Err(err) => {
                self.sync_in_progress = false;
                self.sync_status = format!("{}: {err:#}", t!("sync_failed")).into();
                cx.notify();
                return;
            }
        };

        let payload = SyncPayload::new(
            self.config.sync_device_id().to_string(),
            scrubbed_sessions,
            scrubbed_keys,
        );
        let mode = UploadMode::conditional(self.config.sync_etag().map(str::to_string));
        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = match sync::upload(credentials, payload, mode).await {
                Ok(etag) => SyncResult::Uploaded { etag },
                Err(err) => SyncResult::Failed(format!("{err:#}")),
            };
            let _ = events.send(BackendEvent::SyncFinished(result));
        });
    }

    pub(crate) fn download_sync_config(&mut self, cx: &mut Context<Self>) {
        let Some(credentials) = self.begin_sync(t!("sync_downloading").into(), cx) else {
            return;
        };

        let privacy_password = self.sync_privacy_password(cx);
        // 下载需要在异步任务里做字段级合并，先把本地副本克隆进去
        let local_sessions = self.config.sessions().to_vec();
        let local_keys = self.config.managed_keys().to_vec();
        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = match sync::download(credentials).await {
                Ok((payload, etag)) => {
                    // 字段级合并：远端空字段保留本地，远端密文解密覆盖
                    match SecretScrubber::merge(
                        &local_sessions,
                        payload.sessions,
                        &local_keys,
                        payload.managed_keys,
                        &privacy_password,
                    ) {
                        Ok(MergedSecrets {
                            sessions,
                            managed_keys,
                            decrypted_count,
                        }) => SyncResult::Downloaded {
                            sessions,
                            managed_keys,
                            etag,
                            decrypted_count,
                        },
                        Err(err) => SyncResult::Failed(format!("{err:#}")),
                    }
                }
                Err(err) => SyncResult::Failed(format!("{err:#}")),
            };
            let _ = events.send(BackendEvent::SyncFinished(result));
        });
    }

    /// 本地强行重置隐私密码：用本机当前明文配置 + 新密码重新加密，强制覆盖云端。
    ///
    /// 调用前 UI 已校验两次输入一致且长度达标。
    pub(crate) fn reset_sync_privacy_password(
        &mut self,
        new_password: String,
        cx: &mut Context<Self>,
    ) {
        if self.sync_in_progress {
            self.sync_status = t!("sync_failed").into();
            cx.notify();
            return;
        }

        // 校验本地有可同步的隐私信息
        let has_secrets = self.config.sessions().iter().any(|s| {
            !s.password.is_empty()
                || !s.passphrase.is_empty()
                || !s.private_key_inline.is_empty()
                || !s.proxy_password.is_empty()
        }) || self
            .config
            .managed_keys()
            .iter()
            .any(|k| !k.inline_content.is_empty() || !k.passphrase.is_empty());
        if !has_secrets {
            self.sync_status = t!("sync_reset_no_local_secrets").into();
            cx.notify();
            return;
        }

        let credentials = self.sync_credentials(cx);
        let sessions = self.config.sessions().to_vec();
        let managed_keys = self.config.managed_keys().to_vec();
        let scrub_result = SecretScrubber::scrub(sessions, managed_keys, true, &new_password);
        let (scrubbed_sessions, scrubbed_keys) = match scrub_result {
            Ok(v) => v,
            Err(err) => {
                self.sync_status = format!("{}: {err:#}", t!("sync_failed")).into();
                cx.notify();
                return;
            }
        };

        let payload = SyncPayload::new(
            self.config.sync_device_id().to_string(),
            scrubbed_sessions,
            scrubbed_keys,
        );

        self.sync_in_progress = true;
        self.sync_status = t!("sync_reset_uploading").into();
        cx.notify();

        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = match sync::upload(credentials, payload, UploadMode::Force).await {
                Ok(_) => SyncResult::PrivacyPasswordReset { new_password },
                Err(err) => SyncResult::Failed(format!("{err:#}")),
            };
            let _ = events.send(BackendEvent::SyncFinished(result));
        });
    }
}

/// 用硬件 UUID 绑定加密隐私密码，并返回 Argon2id 哈希用于校验输入一致性。
pub(crate) fn seal_privacy_password(privacy_password: &str) -> anyhow::Result<(String, String)> {
    let hw = hardware_uuid();
    let sealed = crypto::seal_with_hardware_key(privacy_password, &hw)?;
    // 用与字段级加密相同的 KDF 派生一次哈希；salt 固定为设备 id 的前 16 字节
    // 以避免额外随机性来源，仅用于"输入是否与上次一致"的弱校验，不用于解密。
    let mut salt = [0u8; 16];
    let hw_bytes = hw.as_bytes();
    let n = hw_bytes.len().min(16);
    salt[..n].copy_from_slice(&hw_bytes[..n]);
    let key = crypto::derive_key(privacy_password, &salt)?;
    let hash = base64::engine::general_purpose::STANDARD.encode(key);
    Ok((sealed, hash))
}
