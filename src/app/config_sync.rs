use gpui::{App, Context, SharedString};
use rust_i18n::t;

use crate::{
    TinyShell,
    app::settings_window::SettingsInputs,
    crypto,
    session::config::hardware_uuid,
    sync::{
        self, MergedConfig, SyncBackendCredentials, SyncBackendKind, SyncCredentials,
        SyncErrorCategory, SyncFailure, SyncPayload, SyncResult, UploadMode,
    },
    terminal::BackendEvent,
};

use base64::Engine as _;

const PRIVACY_PASSWORD_MIN_LEN: usize = 8;

#[derive(Clone)]
pub(crate) struct SyncFormSnapshot {
    credentials: SyncCredentials,
    privacy_password: String,
}

impl SyncFormSnapshot {
    pub(crate) fn capture(backend: &str, inputs: &SettingsInputs, cx: &App) -> Self {
        let input_value = |input: &gpui::Entity<gpui_component::input::InputState>| {
            input.read(cx).value().trim().to_string()
        };
        let backend = if backend == "s3" {
            SyncBackendCredentials::S3 {
                endpoint: input_value(&inputs.sync_s3_endpoint),
                region: input_value(&inputs.sync_s3_region),
                bucket: input_value(&inputs.sync_s3_bucket),
                object_key: input_value(&inputs.sync_s3_object_key),
                access_key: input_value(&inputs.sync_s3_access_key),
                secret_key: inputs.sync_s3_secret_key.read(cx).value().to_string(),
                session_token: inputs.sync_s3_session_token.read(cx).value().to_string(),
            }
        } else {
            SyncBackendCredentials::WebDav {
                endpoint: input_value(&inputs.sync_endpoint),
                username: input_value(&inputs.sync_username),
                password: inputs.sync_webdav_password.read(cx).value().to_string(),
            }
        };
        Self {
            credentials: SyncCredentials { backend },
            privacy_password: inputs.sync_privacy_password.read(cx).value().to_string(),
        }
    }

    pub(crate) fn from_credentials(credentials: SyncCredentials) -> Self {
        Self {
            credentials,
            privacy_password: String::new(),
        }
    }
}

pub(crate) fn sync_failure_status(failure: &SyncFailure) -> String {
    let detail = match (failure.backend, failure.category) {
        (Some(SyncBackendKind::WebDav), SyncErrorCategory::EndpointRequired) => {
            t!("sync_webdav_endpoint_required").to_string()
        }
        (Some(SyncBackendKind::WebDav), SyncErrorCategory::EndpointInvalid) => {
            t!("sync_webdav_endpoint_invalid").to_string()
        }
        (Some(SyncBackendKind::WebDav), SyncErrorCategory::AuthenticationFailed) => {
            t!("sync_webdav_auth_failed").to_string()
        }
        (Some(SyncBackendKind::WebDav), SyncErrorCategory::NotFound) => {
            t!("sync_webdav_not_found").to_string()
        }
        (_, SyncErrorCategory::Conflict) => t!("sync_remote_conflict").to_string(),
        (_, SyncErrorCategory::RemoteMissing) => t!("sync_remote_missing").to_string(),
        _ => failure.detail.clone(),
    };
    format!("{}: {detail}", t!("sync_failed"))
}

fn upload_preflight_result(
    credentials: SyncCredentials,
    privacy_password: String,
    include_secrets: bool,
    merged: MergedConfig,
    etag: Option<String>,
) -> SyncResult {
    if merged.unavailable_secret_count > 0 {
        SyncResult::UploadPreflightBlocked {
            credentials,
            unavailable_session_secret_count: merged.unavailable_session_secret_count,
            unavailable_managed_key_secret_count: merged.unavailable_managed_key_secret_count,
        }
    } else {
        SyncResult::UploadPreflightReady {
            credentials,
            privacy_password,
            include_secrets,
            merged: Some(merged),
            etag,
        }
    }
}

impl TinyShell {
    fn begin_sync(
        &mut self,
        credentials: SyncCredentials,
        status: SharedString,
        cx: &mut Context<Self>,
    ) -> Option<SyncCredentials> {
        if self.sync_in_progress {
            return None;
        }
        if let Err(failure) = sync::validate_credentials(&credentials) {
            self.sync_status = sync_failure_status(&failure).into();
            cx.notify();
            return None;
        }
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

    pub(crate) fn verify_sync_connection(
        &mut self,
        form: SyncFormSnapshot,
        cx: &mut Context<Self>,
    ) {
        if self.sync_in_progress {
            return;
        }
        let SyncBackendCredentials::WebDav {
            endpoint,
            username,
            password,
        } = form.credentials.backend
        else {
            return;
        };

        self.sync_in_progress = true;
        self.sync_status = t!("sync_verifying_connection").into();
        cx.notify();

        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = match sync::verify_webdav_connection(&endpoint, &username, &password).await
            {
                Ok(()) => SyncResult::ConnectionVerified,
                Err(failure) => SyncResult::Failed(failure),
            };
            let _ = events.send(BackendEvent::SyncFinished(result));
        });
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

    pub(crate) fn upload_sync_config(&mut self, form: SyncFormSnapshot, cx: &mut Context<Self>) {
        let SyncFormSnapshot {
            credentials,
            privacy_password,
        } = form;
        let Some(credentials) =
            self.begin_sync(credentials, t!("sync_preparing_upload").into(), cx)
        else {
            return;
        };

        let include_secrets = self.config.sync_include_secrets();
        if include_secrets && privacy_password.chars().count() < PRIVACY_PASSWORD_MIN_LEN {
            self.sync_in_progress = false;
            self.sync_status = t!("sync_privacy_password_required").into();
            cx.notify();
            return;
        }

        let local_sessions = self.config.sessions().to_vec();
        let local_connection_groups = self.config.connection_groups().to_vec();
        let local_keys = self.config.managed_keys().to_vec();
        let local_commands = self
            .config
            .quick_command_categories()
            .unwrap_or_default()
            .to_vec();
        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = match sync::download(credentials.clone(), &privacy_password).await {
                Ok((payload, etag)) => {
                    let merged = sync::merge_payload(
                        &local_sessions,
                        &local_connection_groups,
                        &local_keys,
                        &local_commands,
                        payload,
                        &privacy_password,
                    );
                    upload_preflight_result(
                        credentials,
                        privacy_password,
                        include_secrets,
                        merged,
                        etag,
                    )
                }
                Err(failure) if failure.category == SyncErrorCategory::RemoteMissing => {
                    SyncResult::UploadPreflightReady {
                        credentials,
                        privacy_password,
                        include_secrets,
                        merged: None,
                        etag: None,
                    }
                }
                Err(failure) => SyncResult::Failed(failure),
            };
            let _ = events.send(BackendEvent::SyncFinished(result));
        });
    }

    pub(crate) fn continue_sync_upload(
        &mut self,
        credentials: SyncCredentials,
        privacy_password: String,
        include_secrets: bool,
        merged: Option<MergedConfig>,
        etag: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let remote_exists = merged.is_some();
        if let Some(merged) = merged {
            self.config.replace_sessions(merged.sessions);
            self.config
                .replace_connection_groups(merged.connection_groups);
            self.config.replace_managed_keys(merged.managed_keys);
            self.managed_keys = self.config.managed_keys().to_vec();
            self.config
                .set_quick_command_categories(merged.quick_command_categories);
        }
        self.config.set_sync_etag(etag.clone());
        if let Err(error) = self.config.save() {
            self.sync_in_progress = false;
            self.sync_status = format!("{}: {error:#}", t!("sync_failed")).into();
            cx.notify();
            return;
        }

        let payload = match SyncPayload::new(
            self.config.sync_device_id().to_string(),
            self.config.sessions().to_vec(),
            self.config.connection_groups().to_vec(),
            self.config.managed_keys().to_vec(),
            self.config
                .quick_command_categories()
                .unwrap_or_default()
                .to_vec(),
            include_secrets,
            &privacy_password,
        ) {
            Ok(payload) => payload,
            Err(error) => {
                self.sync_in_progress = false;
                self.sync_status = format!("{}: {error:#}", t!("sync_failed")).into();
                cx.notify();
                return;
            }
        };
        let mode = if remote_exists && etag.is_none() {
            UploadMode::Force
        } else {
            UploadMode::conditional(etag)
        };
        let uploaded_privacy_password = include_secrets.then_some(privacy_password);
        self.sync_in_progress = true;
        self.sync_status = t!("sync_uploading").into();
        cx.notify();

        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = match sync::upload(credentials, payload, mode).await {
                Ok(etag) => SyncResult::Uploaded {
                    etag,
                    privacy_password: uploaded_privacy_password,
                },
                Err(failure) => SyncResult::Failed(failure),
            };
            let _ = events.send(BackendEvent::SyncFinished(result));
        });
    }

    pub(crate) fn download_sync_config(&mut self, form: SyncFormSnapshot, cx: &mut Context<Self>) {
        let SyncFormSnapshot {
            credentials,
            privacy_password,
        } = form;
        let Some(credentials) = self.begin_sync(credentials, t!("sync_downloading").into(), cx)
        else {
            return;
        };

        let local_sessions = self.config.sessions().to_vec();
        let local_connection_groups = self.config.connection_groups().to_vec();
        let local_keys = self.config.managed_keys().to_vec();
        let local_commands = self
            .config
            .quick_command_categories()
            .unwrap_or_default()
            .to_vec();
        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = match sync::download(credentials, &privacy_password).await {
                Ok((payload, etag)) => {
                    let MergedConfig {
                        sessions,
                        connection_groups,
                        managed_keys,
                        quick_command_categories,
                        decrypted_count,
                        unavailable_secret_count,
                        ..
                    } = sync::merge_payload(
                        &local_sessions,
                        &local_connection_groups,
                        &local_keys,
                        &local_commands,
                        payload,
                        &privacy_password,
                    );
                    SyncResult::Downloaded {
                        sessions,
                        connection_groups,
                        managed_keys,
                        quick_command_categories,
                        etag,
                        decrypted_count,
                        unavailable_secret_count,
                    }
                }
                Err(failure) => SyncResult::Failed(failure),
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
        form: SyncFormSnapshot,
        cx: &mut Context<Self>,
    ) {
        let Some(credentials) =
            self.begin_sync(form.credentials, t!("sync_reset_uploading").into(), cx)
        else {
            return;
        };

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
            self.sync_in_progress = false;
            self.sync_status = t!("sync_reset_no_local_secrets").into();
            cx.notify();
            return;
        }

        let payload = match SyncPayload::new(
            self.config.sync_device_id().to_string(),
            self.config.sessions().to_vec(),
            self.config.connection_groups().to_vec(),
            self.config.managed_keys().to_vec(),
            self.config
                .quick_command_categories()
                .unwrap_or_default()
                .to_vec(),
            true,
            &new_password,
        ) {
            Ok(payload) => payload,
            Err(err) => {
                self.sync_in_progress = false;
                self.sync_status = format!("{}: {err:#}", t!("sync_failed")).into();
                cx.notify();
                return;
            }
        };

        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = match sync::upload(credentials, payload, UploadMode::Force).await {
                Ok(etag) => SyncResult::PrivacyPasswordReset { new_password, etag },
                Err(failure) => SyncResult::Failed(failure),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s3_http_status_is_not_mapped_to_webdav_authentication() {
        let failure = SyncFailure {
            backend: Some(SyncBackendKind::S3),
            category: SyncErrorCategory::AuthenticationFailed,
            detail: "S3 upload failed: HTTP 401 Unauthorized".to_string(),
        };

        let status = sync_failure_status(&failure);

        assert!(status.contains("S3 upload failed: HTTP 401 Unauthorized"));
        assert!(!status.contains(t!("sync_webdav_auth_failed").as_ref()));
    }

    #[test]
    fn upload_preflight_blocks_when_any_remote_secret_is_unavailable() {
        let result = upload_preflight_result(
            webdav_credentials(),
            "wrong-password".into(),
            true,
            MergedConfig {
                sessions: Vec::new(),
                connection_groups: Vec::new(),
                managed_keys: Vec::new(),
                quick_command_categories: Vec::new(),
                decrypted_count: 0,
                unavailable_secret_count: 3,
                unavailable_session_secret_count: 1,
                unavailable_managed_key_secret_count: 2,
            },
            Some("etag-1".into()),
        );

        assert!(matches!(
            result,
            SyncResult::UploadPreflightBlocked {
                unavailable_session_secret_count: 1,
                unavailable_managed_key_secret_count: 2,
                ..
            }
        ));
    }

    #[test]
    fn upload_preflight_continues_with_merged_config_and_remote_etag() {
        let result = upload_preflight_result(
            webdav_credentials(),
            "privacy-password".into(),
            true,
            MergedConfig {
                sessions: Vec::new(),
                connection_groups: vec!["remote".into()],
                managed_keys: Vec::new(),
                quick_command_categories: Vec::new(),
                decrypted_count: 2,
                unavailable_secret_count: 0,
                unavailable_session_secret_count: 0,
                unavailable_managed_key_secret_count: 0,
            },
            Some("etag-2".into()),
        );

        assert!(matches!(
            result,
            SyncResult::UploadPreflightReady {
                merged: Some(MergedConfig {
                    connection_groups,
                    ..
                }),
                etag: Some(etag),
                ..
            } if connection_groups == ["remote"] && etag == "etag-2"
        ));
    }

    fn webdav_credentials() -> SyncCredentials {
        SyncCredentials {
            backend: SyncBackendCredentials::WebDav {
                endpoint: "https://dav.example.test/config/".into(),
                username: "alice".into(),
                password: "webdav-password".into(),
            },
        }
    }
}
