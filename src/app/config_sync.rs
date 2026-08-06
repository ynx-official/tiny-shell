use gpui::{App, Context, SharedString};
use rust_i18n::t;

use crate::{
    TinyShell,
    app::settings::form::SyncSettingsInputs,
    crypto,
    session::config::hardware_uuid,
    sync::{
        self, MergedConfig, PrivacyPasswordStatus, SyncBackendCredentials, SyncBackendKind,
        SyncCredentials, SyncErrorCategory, SyncFailure, SyncPayload, SyncResult,
        UploadBlockReason, UploadMode,
    },
    terminal::BackendEvent,
};

use base64::Engine as _;
use std::{future::Future, time::Duration};

const PRIVACY_PASSWORD_MIN_LEN: usize = 8;

pub(crate) fn automatic_sync_delay(interval_hours: u32, last_synced_at: i64, now: i64) -> Duration {
    if last_synced_at <= 0 {
        return Duration::ZERO;
    }
    let interval_seconds = i64::from(interval_hours.clamp(1, 8_760)).saturating_mul(3_600);
    let elapsed = now.saturating_sub(last_synced_at).max(0);
    Duration::from_secs(interval_seconds.saturating_sub(elapsed).max(0) as u64)
}

pub(crate) fn open_webdav_password(sealed: &str) -> anyhow::Result<String> {
    if sealed.is_empty() {
        return Ok(String::new());
    }
    crypto::open_with_hardware_key(sealed, &hardware_uuid())
}

fn seal_webdav_password(password: &str) -> anyhow::Result<String> {
    if password.is_empty() {
        return Ok(String::new());
    }
    crypto::seal_with_hardware_key(password, &hardware_uuid())
}

pub(crate) fn format_sync_timestamp(timestamp: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(timestamp, 0).map(|time| {
        time.with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    })
}

#[derive(Clone)]
pub(crate) struct SyncFormSnapshot {
    credentials: SyncCredentials,
    privacy_password: String,
}

impl SyncFormSnapshot {
    pub(crate) fn capture(backend: &str, inputs: &SyncSettingsInputs, cx: &App) -> Self {
        let input_value = |input: &gpui::Entity<gpui_component::input::InputState>| {
            input.read(cx).value().trim().to_string()
        };
        let backend = if backend == "s3" {
            SyncBackendCredentials::S3 {
                endpoint: input_value(&inputs.s3_endpoint),
                region: input_value(&inputs.s3_region),
                bucket: input_value(&inputs.s3_bucket),
                object_key: input_value(&inputs.s3_object_key),
                access_key: input_value(&inputs.s3_access_key),
                secret_key: inputs.s3_secret_key.read(cx).value().to_string(),
                session_token: inputs.s3_session_token.read(cx).value().to_string(),
            }
        } else {
            SyncBackendCredentials::WebDav {
                endpoint: input_value(&inputs.endpoint),
                username: input_value(&inputs.username),
                password: inputs.webdav_password.read(cx).value().to_string(),
            }
        };
        Self {
            credentials: SyncCredentials { backend },
            privacy_password: inputs.privacy_password.read(cx).value().to_string(),
        }
    }

    pub(crate) fn from_credentials(credentials: SyncCredentials) -> Self {
        Self {
            credentials,
            privacy_password: String::new(),
        }
    }

    pub(crate) fn with_privacy_password(
        credentials: SyncCredentials,
        privacy_password: String,
    ) -> Self {
        Self {
            credentials,
            privacy_password,
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
    t!("sync_failed", error = detail).to_string()
}

fn privacy_password_verification_result(
    credentials: SyncCredentials,
    password: String,
    downloaded: sync::SyncOperationResult<(SyncPayload, Option<String>)>,
) -> SyncResult {
    match downloaded {
        Ok((payload, _)) => match payload.privacy_password_status(&password) {
            Ok(PrivacyPasswordStatus::NotConfigured) => {
                SyncResult::PrivacyPasswordInitializationReady {
                    credentials,
                    password,
                }
            }
            Ok(status) => SyncResult::PrivacyPasswordChecked { password, status },
            Err(error) => {
                SyncResult::Failed(SyncFailure::other(Some(credentials.backend.kind()), error))
            }
        },
        Err(failure) if failure.category == SyncErrorCategory::RemoteMissing => {
            SyncResult::PrivacyPasswordInitializationReady {
                credentials,
                password,
            }
        }
        Err(failure) => SyncResult::Failed(failure),
    }
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
            reason: UploadBlockReason::UnavailableSecrets {
                session_secret_count: merged.unavailable_session_secret_count,
                managed_key_secret_count: merged.unavailable_managed_key_secret_count,
            },
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
    fn spawn_sync_operation<F>(&mut self, operation: F)
    where
        F: Future<Output = SyncResult> + Send + 'static,
    {
        let cancellation = self.async_runtime.supervisor.start("sync-operation");
        let events = self.async_runtime.events_tx.clone();
        self.runtime.spawn(async move {
            let result = operation.await;
            if !cancellation.is_cancelled() {
                let _ = events.send(BackendEvent::SyncFinished {
                    result: Box::new(result),
                    task_id: cancellation.id(),
                });
            }
        });
    }

    fn begin_sync(
        &mut self,
        credentials: SyncCredentials,
        status: SharedString,
        cx: &mut Context<Self>,
    ) -> Option<SyncCredentials> {
        if self.sync_runtime.in_progress {
            return None;
        }
        if let Err(failure) = sync::validate_credentials(&credentials) {
            self.sync_runtime
                .set_failed(sync_failure_status(&failure).into());
            cx.notify();
            return None;
        }
        match &credentials.backend {
            SyncBackendCredentials::WebDav {
                endpoint,
                username,
                password,
            } => {
                let sealed_password = match seal_webdav_password(password) {
                    Ok(sealed_password) => sealed_password,
                    Err(error) => {
                        self.sync_runtime
                            .set_failed(t!("sync_failed", error = format!("{error:#}")).into());
                        cx.notify();
                        return None;
                    }
                };
                self.config
                    .set_sync_connection(endpoint.clone(), username.clone());
                self.config.set_sync_webdav_password_sealed(sealed_password);
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
        if let Err(err) =
            crate::app::config_persistence::save_full(&self.config_repository, &self.config)
        {
            self.sync_runtime.status = t!("sync_failed", error = format!("{err:#}")).into();
            cx.notify();
            return None;
        }
        self.sync_runtime.in_progress = true;
        self.sync_runtime.clear_failure();
        self.sync_runtime.status = status;
        cx.notify();
        Some(credentials)
    }

    pub(crate) fn verify_sync_connection(
        &mut self,
        form: SyncFormSnapshot,
        cx: &mut Context<Self>,
    ) {
        if self.sync_runtime.in_progress {
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

        self.sync_runtime.in_progress = true;
        self.sync_runtime.status = t!("sync_verifying_connection").into();
        cx.notify();

        self.spawn_sync_operation(async move {
            match sync::verify_webdav_connection(&endpoint, &username, &password).await {
                Ok(()) => SyncResult::ConnectionVerified,
                Err(failure) => SyncResult::Failed(failure),
            }
        });
    }

    pub(crate) fn set_sync_backend(&mut self, backend: &str, cx: &mut Context<Self>) {
        let previous_backend = self.config.sync_backend().to_string();
        let previous_enabled = self.config.sync_enabled();
        self.config.set_sync_backend(backend);
        if backend == "s3" {
            self.config.set_sync_enabled(false);
        }
        match crate::app::config_persistence::save_full(&self.config_repository, &self.config) {
            Ok(()) => {
                self.sync_runtime.status = if backend == "s3" {
                    t!("sync_disabled").into()
                } else {
                    t!("sync_not_run").into()
                };
                self.schedule_automatic_sync(false, cx);
            }
            Err(error) => {
                self.config.set_sync_backend(&previous_backend);
                self.config.set_sync_enabled(previous_enabled);
                self.sync_runtime
                    .set_failed(t!("sync_failed", error = format!("{error:#}")).into());
            }
        }
        cx.notify();
    }

    pub(crate) fn set_automatic_sync_enabled(
        &mut self,
        enabled: bool,
        form: SyncFormSnapshot,
        cx: &mut Context<Self>,
    ) {
        let previous_config = self.config.clone();
        if enabled {
            if !matches!(
                form.credentials.backend,
                SyncBackendCredentials::WebDav { .. }
            ) {
                self.sync_runtime.status = t!("sync_webdav_required_for_auto").into();
                cx.notify();
                return;
            }
            if let Err(failure) = sync::validate_credentials(&form.credentials) {
                self.sync_runtime
                    .set_failed(sync_failure_status(&failure).into());
                cx.notify();
                return;
            }
            let sealed_privacy_password = if self.config.sync_include_secrets() {
                match seal_privacy_password(&form.privacy_password) {
                    Ok(sealed_password) => Some(sealed_password),
                    Err(error) => {
                        self.sync_runtime
                            .set_failed(t!("sync_failed", error = format!("{error:#}")).into());
                        cx.notify();
                        return;
                    }
                }
            } else {
                None
            };
            if let SyncBackendCredentials::WebDav {
                endpoint,
                username,
                password,
            } = form.credentials.backend
            {
                let sealed_password = match seal_webdav_password(&password) {
                    Ok(sealed_password) => sealed_password,
                    Err(error) => {
                        self.sync_runtime
                            .set_failed(t!("sync_failed", error = format!("{error:#}")).into());
                        cx.notify();
                        return;
                    }
                };
                self.config.set_sync_connection(endpoint, username);
                self.config.set_sync_webdav_password_sealed(sealed_password);
            }
            if let Some((sealed, hash)) = sealed_privacy_password {
                self.config.set_sync_secrets_password_sealed(sealed);
                self.config.set_sync_secrets_password_hash(hash);
            }
        }

        self.config.set_sync_enabled(enabled);
        match crate::app::config_persistence::save_full(&self.config_repository, &self.config) {
            Ok(()) => {
                self.sync_runtime.status = if enabled {
                    t!("sync_status_enabled").into()
                } else {
                    t!("sync_disabled").into()
                };
                self.schedule_automatic_sync(enabled, cx);
            }
            Err(error) => {
                self.config = previous_config;
                self.sync_runtime
                    .set_failed(t!("sync_failed", error = format!("{error:#}")).into());
            }
        }
        cx.notify();
    }

    fn automatic_sync_form(&self) -> anyhow::Result<SyncFormSnapshot> {
        if self.config.sync_backend() != "webdav" {
            anyhow::bail!("automatic sync requires WebDAV");
        }
        let privacy_password = if self.config.sync_include_secrets() {
            crypto::open_with_hardware_key(
                self.config.sync_secrets_password_sealed(),
                &hardware_uuid(),
            )?
        } else {
            String::new()
        };
        Ok(SyncFormSnapshot {
            credentials: SyncCredentials {
                backend: SyncBackendCredentials::WebDav {
                    endpoint: self.config.sync_endpoint().to_string(),
                    username: self.config.sync_username().to_string(),
                    password: open_webdav_password(self.config.sync_webdav_password_sealed())?,
                },
            },
            privacy_password,
        })
    }

    fn run_automatic_sync(&mut self, cx: &mut Context<Self>) {
        if !self.config.sync_enabled() || self.sync_runtime.in_progress {
            return;
        }
        match self.automatic_sync_form() {
            Ok(form) => self.upload_sync_config(form, cx),
            Err(error) => {
                self.sync_runtime
                    .set_failed(t!("sync_failed", error = format!("{error:#}")).into());
                cx.notify();
            }
        }
    }

    pub(crate) fn schedule_automatic_sync(
        &mut self,
        run_immediately: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.config.sync_enabled() || self.config.sync_backend() != "webdav" {
            self.sync_runtime.start_schedule();
            self.async_runtime.supervisor.cancel("automatic-sync");
            return;
        }
        let generation = self.sync_runtime.start_schedule();
        let cancellation = self.async_runtime.supervisor.start("automatic-sync");

        let interval =
            Duration::from_secs(u64::from(self.config.sync_interval_hours()).saturating_mul(3_600));
        let initial_delay = if run_immediately {
            Duration::ZERO
        } else {
            automatic_sync_delay(
                self.config.sync_interval_hours(),
                self.config.sync_last_synced_at(),
                chrono::Utc::now().timestamp(),
            )
        };

        cx.spawn(async move |this, cx| {
            if cancellation.is_cancelled() {
                return;
            }
            cx.background_executor().timer(initial_delay).await;
            loop {
                if cancellation.is_cancelled() {
                    break;
                }
                let Ok(is_current_schedule) = this.update(cx, |this, cx| {
                    if !this.sync_runtime.is_current_schedule(generation)
                        || !this.config.sync_enabled()
                        || this.config.sync_backend() != "webdav"
                    {
                        return false;
                    }
                    this.run_automatic_sync(cx);
                    true
                }) else {
                    break;
                };
                if !is_current_schedule {
                    break;
                }
                cx.background_executor().timer(interval).await;
            }
        })
        .detach();
    }

    /// 切换“同步密码和密钥”开关。远端已有加密信息时需先校验；首次同步可直接初始化。
    pub(crate) fn set_sync_include_secrets(
        &mut self,
        include: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let previous = self.config.sync_include_secrets();
        self.config.set_sync_include_secrets(include);
        match crate::app::config_persistence::save_full(&self.config_repository, &self.config) {
            Ok(()) => {
                self.sync_runtime.status = t!("sync_not_run").into();
                cx.notify();
                true
            }
            Err(error) => {
                self.config.set_sync_include_secrets(previous);
                self.sync_runtime
                    .set_failed(t!("sync_failed", error = format!("{error:#}")).into());
                cx.notify();
                false
            }
        }
    }

    pub(crate) fn verify_sync_secrets_password(
        &mut self,
        form: SyncFormSnapshot,
        password: String,
        cx: &mut Context<Self>,
    ) {
        if self.sync_runtime.in_progress {
            return;
        }
        if password.chars().count() < PRIVACY_PASSWORD_MIN_LEN {
            if let Some(dialog) = self.sync_runtime.secrets_password_dialog.as_mut() {
                dialog.status = crate::app::SyncSecretsPasswordDialogStatus::PasswordRequired;
                dialog.message = Some(t!("sync_privacy_password_required").into());
            }
            cx.notify();
            return;
        }

        if let Some(dialog) = self.sync_runtime.secrets_password_dialog.as_mut() {
            dialog.status = crate::app::SyncSecretsPasswordDialogStatus::Verifying;
            dialog.message = Some(t!("sync_secret_toggle_verifying").into());
        }
        let Some(credentials) = self.begin_sync(
            form.credentials,
            t!("sync_secret_toggle_verifying").into(),
            cx,
        ) else {
            let message = self.sync_runtime.status.clone();
            if let Some(dialog) = self.sync_runtime.secrets_password_dialog.as_mut() {
                dialog.status = crate::app::SyncSecretsPasswordDialogStatus::Failed;
                dialog.message = Some(message);
            }
            cx.notify();
            return;
        };

        self.spawn_sync_operation(async move {
            let downloaded = sync::download(credentials.clone(), &password).await;
            privacy_password_verification_result(credentials, password, downloaded)
        });
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
            self.sync_runtime.in_progress = false;
            self.sync_runtime.status = t!("sync_privacy_password_required").into();
            cx.notify();
            return;
        }

        let local_sessions = self.config.sessions().to_vec();
        let local_deleted_sessions = self.config.deleted_sessions().to_vec();
        let local_connection_groups = self.config.connection_groups().to_vec();
        let local_deleted_connection_groups = self.config.deleted_connection_groups().to_vec();
        let local_keys = self.config.managed_keys().to_vec();
        let local_commands = self
            .config
            .quick_command_categories()
            .unwrap_or_default()
            .to_vec();
        self.spawn_sync_operation(async move {
            match sync::download(credentials.clone(), &privacy_password).await {
                Ok((payload, etag)) => match payload.privacy_password_status(&privacy_password) {
                    Ok(PrivacyPasswordStatus::Missing) => SyncResult::UploadPreflightBlocked {
                        credentials,
                        reason: UploadBlockReason::PasswordRequired,
                    },
                    Ok(PrivacyPasswordStatus::Mismatch) => SyncResult::UploadPreflightBlocked {
                        credentials,
                        reason: UploadBlockReason::PasswordMismatch,
                    },
                    Ok(PrivacyPasswordStatus::Verified | PrivacyPasswordStatus::NotConfigured) => {
                        let merged = sync::merge_payload_with_deleted(
                            sync::MergeLocal {
                                sessions: &local_sessions,
                                deleted_sessions: &local_deleted_sessions,
                                connection_groups: &local_connection_groups,
                                deleted_connection_groups: &local_deleted_connection_groups,
                                keys: &local_keys,
                                commands: &local_commands,
                            },
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
                    Err(error) => SyncResult::Failed(SyncFailure::other(
                        Some(credentials.backend.kind()),
                        error,
                    )),
                },
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
            }
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
        let payload = match merged.as_ref() {
            Some(merged) => SyncPayload::new_with_deleted(crate::sync::SyncPayloadInput {
                device_id: self.config.sync_device_id().to_string(),
                sessions: merged.sessions.clone(),
                deleted_sessions: merged.deleted_sessions.clone(),
                connection_groups: merged.connection_groups.clone(),
                deleted_connection_groups: merged.deleted_connection_groups.clone(),
                managed_keys: merged.managed_keys.clone(),
                quick_command_categories: merged.quick_command_categories.clone(),
                include_secrets,
                privacy_password: privacy_password.clone(),
            }),
            None => SyncPayload::new_with_deleted(crate::sync::SyncPayloadInput {
                device_id: self.config.sync_device_id().to_string(),
                sessions: self.config.sessions().to_vec(),
                deleted_sessions: self.config.deleted_sessions().to_vec(),
                connection_groups: self.config.connection_groups().to_vec(),
                deleted_connection_groups: self.config.deleted_connection_groups().to_vec(),
                managed_keys: self.config.managed_keys().to_vec(),
                quick_command_categories: self
                    .config
                    .quick_command_categories()
                    .unwrap_or_default()
                    .to_vec(),
                include_secrets,
                privacy_password: privacy_password.clone(),
            }),
        };
        let payload = match payload {
            Ok(payload) => payload,
            Err(error) => {
                self.sync_runtime.in_progress = false;
                self.sync_runtime
                    .set_failed(t!("sync_failed", error = format!("{error:#}")).into());
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
        self.sync_runtime.in_progress = true;
        self.sync_runtime.status = t!("sync_uploading").into();
        cx.notify();

        self.spawn_sync_operation(async move {
            match sync::upload(credentials, payload, mode).await {
                Ok(etag) => SyncResult::Uploaded {
                    etag,
                    privacy_password: uploaded_privacy_password,
                    merged,
                },
                Err(failure) => SyncResult::Failed(failure),
            }
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
        let local_deleted_sessions = self.config.deleted_sessions().to_vec();
        let local_connection_groups = self.config.connection_groups().to_vec();
        let local_deleted_connection_groups = self.config.deleted_connection_groups().to_vec();
        let local_keys = self.config.managed_keys().to_vec();
        let local_commands = self
            .config
            .quick_command_categories()
            .unwrap_or_default()
            .to_vec();
        self.spawn_sync_operation(async move {
            match sync::download(credentials.clone(), &privacy_password).await {
                Ok((payload, etag)) => match payload.privacy_password_status(&privacy_password) {
                    Ok(password_status) => {
                        let MergedConfig {
                            sessions,
                            deleted_sessions,
                            deleted_connection_groups,
                            connection_groups,
                            managed_keys,
                            quick_command_categories,
                            decrypted_count,
                            unavailable_secret_count,
                            ..
                        } = match password_status {
                            PrivacyPasswordStatus::Verified
                            | PrivacyPasswordStatus::NotConfigured => {
                                sync::merge_payload_with_deleted(
                                    sync::MergeLocal {
                                        sessions: &local_sessions,
                                        deleted_sessions: &local_deleted_sessions,
                                        connection_groups: &local_connection_groups,
                                        deleted_connection_groups: &local_deleted_connection_groups,
                                        keys: &local_keys,
                                        commands: &local_commands,
                                    },
                                    payload,
                                    &privacy_password,
                                )
                            }
                            PrivacyPasswordStatus::Missing | PrivacyPasswordStatus::Mismatch => {
                                sync::merge_public_payload_with_deleted(
                                    sync::MergeLocal {
                                        sessions: &local_sessions,
                                        deleted_sessions: &local_deleted_sessions,
                                        connection_groups: &local_connection_groups,
                                        deleted_connection_groups: &local_deleted_connection_groups,
                                        keys: &local_keys,
                                        commands: &local_commands,
                                    },
                                    payload,
                                )
                            }
                        };
                        SyncResult::Downloaded {
                            credentials,
                            password_status,
                            sessions,
                            deleted_sessions,
                            deleted_connection_groups,
                            connection_groups,
                            managed_keys,
                            quick_command_categories,
                            etag,
                            decrypted_count,
                            unavailable_secret_count,
                        }
                    }
                    Err(error) => SyncResult::Failed(SyncFailure::other(
                        Some(credentials.backend.kind()),
                        error,
                    )),
                },
                Err(failure) => SyncResult::Failed(failure),
            }
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
            self.sync_runtime.in_progress = false;
            self.sync_runtime.status = t!("sync_reset_no_local_secrets").into();
            cx.notify();
            return;
        }

        let payload = match SyncPayload::new_with_deleted(crate::sync::SyncPayloadInput {
            device_id: self.config.sync_device_id().to_string(),
            sessions: self.config.sessions().to_vec(),
            deleted_sessions: self.config.deleted_sessions().to_vec(),
            connection_groups: self.config.connection_groups().to_vec(),
            deleted_connection_groups: self.config.deleted_connection_groups().to_vec(),
            managed_keys: self.config.managed_keys().to_vec(),
            quick_command_categories: self
                .config
                .quick_command_categories()
                .unwrap_or_default()
                .to_vec(),
            include_secrets: true,
            privacy_password: new_password.clone(),
        }) {
            Ok(payload) => payload,
            Err(err) => {
                self.sync_runtime.in_progress = false;
                self.sync_runtime.status = t!("sync_failed", error = format!("{err:#}")).into();
                cx.notify();
                return;
            }
        };

        self.spawn_sync_operation(async move {
            match sync::upload(credentials, payload, UploadMode::Force).await {
                Ok(etag) => SyncResult::PrivacyPasswordReset { new_password, etag },
                Err(failure) => SyncResult::Failed(failure),
            }
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
    fn automatic_sync_delay_runs_immediately_without_previous_success() {
        assert_eq!(automatic_sync_delay(24, 0, 1_700_000_000), Duration::ZERO);
    }

    #[test]
    fn automatic_sync_delay_uses_remaining_interval() {
        assert_eq!(
            automatic_sync_delay(6, 1_700_000_000, 1_700_003_600),
            Duration::from_secs(18_000)
        );
        assert_eq!(
            automatic_sync_delay(6, 1_700_000_000, 1_700_021_600),
            Duration::ZERO
        );
    }

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
                deleted_sessions: Vec::new(),
                deleted_connection_groups: Vec::new(),
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
                reason: UploadBlockReason::UnavailableSecrets {
                    session_secret_count: 1,
                    managed_key_secret_count: 2,
                },
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
                deleted_sessions: Vec::new(),
                deleted_connection_groups: Vec::new(),
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

    #[test]
    fn missing_remote_initializes_privacy_password_instead_of_failing() {
        let credentials = webdav_credentials();
        let result = privacy_password_verification_result(
            credentials,
            "privacy-password".into(),
            Err(SyncFailure {
                backend: Some(SyncBackendKind::WebDav),
                category: SyncErrorCategory::RemoteMissing,
                detail: "remote configuration is missing".into(),
            }),
        );

        assert!(matches!(
            result,
            SyncResult::PrivacyPasswordInitializationReady { password, .. }
                if password == "privacy-password"
        ));
    }

    #[test]
    fn remote_without_password_verifier_initializes_secret_sync() {
        let Ok(payload) = SyncPayload::new(
            "device-1".into(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            "",
        ) else {
            panic!("public-only sync payload should be constructible");
        };
        let result = privacy_password_verification_result(
            webdav_credentials(),
            "privacy-password".into(),
            Ok((payload, Some("etag-1".into()))),
        );

        assert!(matches!(
            result,
            SyncResult::PrivacyPasswordInitializationReady { password, .. }
                if password == "privacy-password"
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
