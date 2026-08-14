use gpui::Context;
use rust_i18n::t;

use crate::{
    TinyShell,
    app::{config_persistence, config_sync},
    session::config::{
        ConfigStore, DeletedConnectionGroup, DeletedSession, ManagedKey, QuickCommandCategory,
        Session,
    },
    sync::{
        ConflictResolution, MergedConfig, PrivacyPasswordStatus, SyncCredentials, SyncFailure,
        SyncResult, UploadBlockReason,
    },
};

/// Configuration payload extracted from a successful sync download.
///
/// Bundling these fields removes the long argument lists that previously
/// required `#[allow(clippy::too_many_arguments)]`.
pub(crate) struct SyncDownloadedConfig {
    pub(crate) sessions: Vec<Session>,
    pub(crate) deleted_sessions: Vec<DeletedSession>,
    pub(crate) connection_groups: Vec<String>,
    pub(crate) deleted_connection_groups: Vec<DeletedConnectionGroup>,
    pub(crate) managed_keys: Vec<ManagedKey>,
    pub(crate) quick_command_categories: Vec<QuickCommandCategory>,
    pub(crate) etag: Option<String>,
}

struct SyncDownloadSuccess {
    credentials: SyncCredentials,
    target: crate::sync::state::SyncTargetKey,
    payload: crate::sync::protocol::V3SyncPayload,
    password_status: PrivacyPasswordStatus,
    config: SyncDownloadedConfig,
    decrypted_count: u32,
    unavailable_secret_count: u32,
}

impl TinyShell {
    pub(crate) fn handle_sync_finished(
        &mut self,
        result: SyncResult,
        task_id: u64,
        cx: &mut Context<Self>,
    ) {
        if !self
            .async_runtime
            .supervisor
            .finish("sync-operation", task_id)
        {
            tracing::debug!(task_id, "ignoring stale sync result");
            return;
        }
        let close_sync_terminal = !matches!(&result, SyncResult::UploadPreflightReady(_));
        let close_sync_was_running = self.close_sync_running;
        self.sync_runtime.in_progress = false;
        match result {
            SyncResult::Uploaded {
                target,
                payload,
                etag,
                privacy_password,
                merged,
            } => {
                self.handle_sync_uploaded(target, payload, etag, privacy_password, merged, cx);
            }
            SyncResult::UploadPreflightReady(plan) => {
                self.handle_sync_upload_preflight_ready(plan, cx);
            }
            SyncResult::ReconciliationConflicts(pending) => {
                self.handle_sync_reconciliation_conflicts(pending, cx);
            }
            SyncResult::UploadPreflightBlocked {
                credentials,
                reason,
            } => {
                self.handle_sync_upload_preflight_blocked(credentials, reason, cx);
            }
            SyncResult::Downloaded {
                credentials,
                target,
                payload,
                password_status,
                sessions,
                deleted_sessions,
                connection_groups,
                deleted_connection_groups,
                managed_keys,
                quick_command_categories,
                etag,
                decrypted_count,
                unavailable_secret_count,
            } => {
                self.handle_sync_downloaded(
                    SyncDownloadSuccess {
                        credentials,
                        target,
                        payload,
                        password_status,
                        config: SyncDownloadedConfig {
                            sessions,
                            deleted_sessions,
                            connection_groups,
                            deleted_connection_groups,
                            managed_keys,
                            quick_command_categories,
                            etag,
                        },
                        decrypted_count,
                        unavailable_secret_count,
                    },
                    cx,
                );
            }
            SyncResult::PrivacyPasswordReset {
                target,
                payload,
                new_password,
                etag,
            } => {
                self.handle_sync_privacy_password_reset(target, payload, new_password, etag, cx);
            }
            SyncResult::PrivacyPasswordInitializationReady {
                credentials,
                password,
            } => {
                self.handle_sync_privacy_password_initialization_ready(credentials, password, cx);
            }
            SyncResult::PrivacyPasswordChecked { password, status } => {
                self.handle_sync_privacy_password_checked(password, status, cx);
            }
            SyncResult::ConnectionVerified => {
                self.handle_sync_connection_verified(cx);
            }
            SyncResult::Failed(error) => {
                self.handle_sync_failed(error, cx);
            }
        }
        if close_sync_was_running && close_sync_terminal && !self.sync_runtime.in_progress {
            self.complete_close_sync(cx);
        } else if !close_sync_was_running && !self.sync_runtime.in_progress {
            self.continue_queued_close_sync(cx);
        }
        if !self.sync_runtime.in_progress && self.sync_runtime.take_pending_automatic() {
            self.request_automatic_sync(cx);
        }
    }

    fn handle_sync_uploaded(
        &mut self,
        target: crate::sync::state::SyncTargetKey,
        payload: crate::sync::protocol::V3SyncPayload,
        etag: Option<String>,
        privacy_password: Option<String>,
        merged: Option<MergedConfig>,
        cx: &mut Context<Self>,
    ) {
        let previous_config = self.config.clone();
        let previous_managed_keys = self.managed_keys.clone();
        let reconciliation_is_stale = self.sync_runtime.reconciliation_is_stale();
        if !reconciliation_is_stale && let Some(merged) = merged {
            self.config.replace_sessions(merged.sessions);
            self.config
                .replace_deleted_sessions(merged.deleted_sessions);
            self.config
                .replace_connection_groups(merged.connection_groups);
            self.config
                .replace_deleted_connection_groups(merged.deleted_connection_groups);
            self.config.replace_managed_keys(merged.managed_keys);
            self.managed_keys = self.config.managed_keys().to_vec();
            self.config
                .set_quick_command_categories(merged.quick_command_categories);
        }
        self.config.set_sync_etag(etag.clone());
        self.config
            .set_sync_last_synced_at(chrono::Utc::now().timestamp());
        let password_result = privacy_password.map_or(Ok(()), |password| {
            config_sync::seal_privacy_password(&password).map(|(sealed, hash)| {
                self.config.set_sync_secrets_password_sealed(sealed);
                self.config.set_sync_secrets_password_hash(hash);
            })
        });
        match password_result
            .and_then(|()| config_persistence::save_full(&self.config_repository, &self.config))
        {
            Ok(()) => {
                let baseline = crate::sync::state::SyncBaseline::from_remote_payload(
                    &target,
                    payload,
                    etag.clone(),
                    chrono::Utc::now().timestamp(),
                );
                match crate::sync::state::SyncStateRepository::new()
                    .and_then(|repository| repository.save(&target, baseline))
                {
                    Ok(()) => {
                        self.sync_runtime.record_success();
                        self.sync_runtime.status = t!("sync_upload_complete").into();
                        self.schedule_automatic_sync(false, cx);
                    }
                    Err(err) => {
                        self.sync_runtime
                            .set_failed(t!("sync_failed", error = format!("{err:#}")).into());
                    }
                }
            }
            Err(err) => {
                self.config = previous_config;
                self.managed_keys = previous_managed_keys;
                self.sync_runtime
                    .set_failed(t!("sync_failed", error = format!("{err:#}")).into());
            }
        }
    }

    fn handle_sync_upload_preflight_ready(
        &mut self,
        plan: crate::sync::UploadPreflightReady,
        cx: &mut Context<Self>,
    ) {
        if self.sync_runtime.reconciliation_is_stale() {
            self.sync_runtime.abandon_reconciliation();
            self.sync_runtime.status = t!("sync_status_pending_changes").into();
            return;
        }
        let crate::sync::UploadPreflightReady {
            credentials,
            privacy_password,
            include_secrets,
            merged,
            remote_payload,
            etag,
        } = plan;
        if let (Some(remote_payload), Some(merged)) = (&remote_payload, &merged) {
            let Ok(candidate) = crate::sync::protocol::V3SyncPayload::from_config(
                self.config.sync_device_id().to_string(),
                merged,
                include_secrets,
                &privacy_password,
                Some(remote_payload),
            ) else {
                self.sync_runtime
                    .set_failed(t!("sync_failed", error = "serialize merged payload").into());
                return;
            };
            if candidate.is_content_equivalent(remote_payload) {
                self.handle_sync_reconciled(
                    crate::sync::state::SyncTargetKey::from_credentials(&credentials.backend),
                    remote_payload.clone(),
                    etag,
                    Some(merged.clone()),
                    cx,
                );
                return;
            }
        }
        self.continue_sync_upload(
            credentials,
            privacy_password,
            include_secrets,
            merged,
            etag,
            cx,
        );
    }

    fn handle_sync_reconciliation_conflicts(
        &mut self,
        pending: crate::sync::PendingSyncConflicts,
        cx: &mut Context<Self>,
    ) {
        if self.sync_runtime.reconciliation_is_stale() {
            self.sync_runtime.abandon_reconciliation();
            self.sync_runtime.status = t!("sync_status_pending_changes").into();
            return;
        }
        let count = pending.three_way.conflicts.len();
        self.sync_runtime.pending_conflicts = Some(pending);
        self.sync_runtime.failed = false;
        self.sync_runtime.status = t!("sync_conflicts_pending", count = count).into();
        cx.notify();
    }

    pub(crate) fn resolve_sync_conflict(
        &mut self,
        index: usize,
        resolution: ConflictResolution,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .sync_runtime
            .pending_conflicts
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no pending synchronization conflicts"))
            .and_then(|pending| pending.three_way.resolve(index, resolution));
        self.finish_sync_conflict_action(result, cx);
    }

    pub(crate) fn resolve_all_sync_conflicts(
        &mut self,
        resolution: ConflictResolution,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .sync_runtime
            .pending_conflicts
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no pending synchronization conflicts"))
            .and_then(|pending| pending.three_way.resolve_all(resolution));
        self.finish_sync_conflict_action(result, cx);
    }

    pub(crate) fn copy_sync_conflict_as_new_connection(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let suffix = t!("sync_conflict_copy_suffix").to_string();
        let result = self
            .sync_runtime
            .pending_conflicts
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no pending synchronization conflicts"))
            .and_then(|pending| pending.three_way.copy_local_session(index, &suffix));
        self.finish_sync_conflict_action(result, cx);
    }

    fn finish_sync_conflict_action(&mut self, result: anyhow::Result<()>, cx: &mut Context<Self>) {
        if let Err(error) = result {
            self.sync_runtime
                .set_failed(t!("sync_failed", error = format!("{error:#}")).into());
            cx.notify();
            return;
        }
        let remaining = self
            .sync_runtime
            .pending_conflicts
            .as_ref()
            .map_or(0, |pending| pending.three_way.conflicts.len());
        if remaining > 0 {
            self.sync_runtime.status = t!("sync_conflicts_pending", count = remaining).into();
            cx.notify();
            return;
        }
        let Some(pending) = self.sync_runtime.pending_conflicts.take() else {
            return;
        };
        self.handle_sync_upload_preflight_ready(
            crate::sync::UploadPreflightReady {
                credentials: pending.credentials,
                privacy_password: pending.privacy_password,
                include_secrets: pending.include_secrets,
                merged: Some(pending.three_way.merged),
                remote_payload: Some(pending.remote_payload),
                etag: pending.etag,
            },
            cx,
        );
    }

    fn handle_sync_reconciled(
        &mut self,
        target: crate::sync::state::SyncTargetKey,
        payload: crate::sync::protocol::V3SyncPayload,
        etag: Option<String>,
        merged: Option<MergedConfig>,
        cx: &mut Context<Self>,
    ) {
        if let Some(merged) = merged {
            self.config.replace_sessions(merged.sessions);
            self.config
                .replace_deleted_sessions(merged.deleted_sessions);
            self.config
                .replace_connection_groups(merged.connection_groups);
            self.config
                .replace_deleted_connection_groups(merged.deleted_connection_groups);
            self.config.replace_managed_keys(merged.managed_keys);
            self.managed_keys = self.config.managed_keys().to_vec();
            self.config
                .set_quick_command_categories(merged.quick_command_categories);
        }
        self.config.set_sync_etag(etag.clone());
        self.config
            .set_sync_last_synced_at(chrono::Utc::now().timestamp());
        match config_persistence::save_full(&self.config_repository, &self.config).and_then(|()| {
            crate::sync::state::SyncStateRepository::new().and_then(|repository| {
                repository.save(
                    &target,
                    crate::sync::state::SyncBaseline::from_remote_payload(
                        &target,
                        payload,
                        etag,
                        chrono::Utc::now().timestamp(),
                    ),
                )
            })
        }) {
            Ok(()) => {
                self.sync_runtime.record_success();
                self.sync_runtime.status = t!("sync_upload_complete").into();
                self.schedule_automatic_sync(false, cx);
            }
            Err(error) => self
                .sync_runtime
                .set_failed(t!("sync_failed", error = format!("{error:#}")).into()),
        }
    }

    fn handle_sync_upload_preflight_blocked(
        &mut self,
        credentials: SyncCredentials,
        reason: UploadBlockReason,
        cx: &mut Context<Self>,
    ) {
        self.sync_runtime.status = match &reason {
            UploadBlockReason::PasswordRequired => t!("sync_upload_password_required").into(),
            UploadBlockReason::PasswordMismatch => t!("sync_privacy_password_incorrect").into(),
            UploadBlockReason::UnavailableSecrets {
                session_secret_count,
                managed_key_secret_count,
            } => t!(
                "sync_upload_secrets_blocked",
                sessions = *session_secret_count,
                keys = *managed_key_secret_count
            )
            .into(),
        };
        if self.close_sync_running {
            return;
        }
        if let Some(handle) = self.auxiliary_windows.settings.handle {
            let owner = cx.entity();
            let form = config_sync::SyncFormSnapshot::from_credentials(credentials);
            let _ = handle.update(cx, move |_, window, cx| {
                owner.update(cx, |this, cx| {
                    this.show_sync_upload_secrets_blocked_dialog(
                        form.clone(),
                        reason.clone(),
                        window,
                        cx,
                    );
                });
            });
        }
    }

    fn handle_sync_downloaded(&mut self, result: SyncDownloadSuccess, cx: &mut Context<Self>) {
        let SyncDownloadSuccess {
            credentials,
            target,
            payload,
            password_status,
            config,
            decrypted_count,
            unavailable_secret_count,
        } = result;
        if self.sync_runtime.reconciliation_is_stale() {
            self.sync_runtime.abandon_reconciliation();
            self.sync_runtime.status = t!("sync_status_pending_changes").into();
            return;
        }
        let session_count = config.sessions.len();
        let group_count = config.connection_groups.len();
        let key_count = config.managed_keys.len();
        let command_count = config
            .quick_command_categories
            .iter()
            .map(|category| category.commands.len())
            .sum::<usize>();
        let remote_etag = config.etag.clone();
        let (previous_config, previous_managed_keys) = self.apply_sync_downloaded_config(config);
        match config_persistence::save_full(&self.config_repository, &self.config) {
            Ok(()) => {
                let summary = t!(
                    "sync_download_summary",
                    sessions = session_count,
                    groups = group_count,
                    keys = key_count,
                    commands = command_count
                )
                .to_string();
                self.sync_runtime.status = match password_status {
                    PrivacyPasswordStatus::Mismatch => {
                        format!("{summary}; {}", t!("sync_privacy_password_incorrect")).into()
                    }
                    PrivacyPasswordStatus::Missing => {
                        format!("{summary}; {}", t!("sync_privacy_password_missing")).into()
                    }
                    _ if unavailable_secret_count > 0 => format!(
                        "{summary}; {}",
                        t!("sync_secrets_unavailable", count = unavailable_secret_count)
                    )
                    .into(),
                    _ if decrypted_count > 0 => format!(
                        "{summary}; {}",
                        t!("sync_secrets_decrypted", count = decrypted_count)
                    )
                    .into(),
                    _ => summary.into(),
                };
                if password_status == PrivacyPasswordStatus::Mismatch
                    && let Some(handle) = self.auxiliary_windows.settings.handle
                {
                    let owner = cx.entity();
                    let form = config_sync::SyncFormSnapshot::from_credentials(credentials);
                    let _ = handle.update(cx, move |_, window, cx| {
                        owner.update(cx, |this, cx| {
                            this.show_sync_upload_secrets_blocked_dialog(
                                form.clone(),
                                UploadBlockReason::PasswordMismatch,
                                window,
                                cx,
                            );
                        });
                    });
                }
                if let Err(err) =
                    crate::sync::state::SyncStateRepository::new().and_then(|repository| {
                        let baseline = crate::sync::state::SyncBaseline::from_remote_payload(
                            &target,
                            payload.clone(),
                            remote_etag.clone(),
                            chrono::Utc::now().timestamp(),
                        );
                        repository.save(&target, baseline)
                    })
                {
                    self.sync_runtime
                        .set_failed(t!("sync_failed", error = format!("{err:#}")).into());
                } else {
                    self.sync_runtime.record_success();
                    self.schedule_automatic_sync(false, cx);
                }
            }
            Err(err) => {
                self.config = previous_config;
                self.managed_keys = previous_managed_keys;
                self.sync_runtime
                    .set_failed(t!("sync_failed", error = format!("{err:#}")).into());
            }
        }
    }

    fn apply_sync_downloaded_config(
        &mut self,
        config: SyncDownloadedConfig,
    ) -> (ConfigStore, Vec<ManagedKey>) {
        let previous_config = self.config.clone();
        let previous_managed_keys = self.managed_keys.clone();
        self.config.replace_sessions(config.sessions);
        self.config
            .replace_deleted_sessions(config.deleted_sessions);
        self.config
            .replace_connection_groups(config.connection_groups);
        self.config
            .replace_deleted_connection_groups(config.deleted_connection_groups);
        self.config.replace_managed_keys(config.managed_keys);
        self.managed_keys = self.config.managed_keys().to_vec();
        self.config
            .set_quick_command_categories(config.quick_command_categories);
        self.config.set_sync_etag(config.etag);
        self.config
            .set_sync_last_synced_at(chrono::Utc::now().timestamp());
        (previous_config, previous_managed_keys)
    }

    fn handle_sync_privacy_password_reset(
        &mut self,
        target: crate::sync::state::SyncTargetKey,
        payload: crate::sync::protocol::V3SyncPayload,
        new_password: String,
        etag: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match config_sync::seal_privacy_password(&new_password) {
            Ok((sealed, hash)) => {
                self.config.set_sync_secrets_password_sealed(sealed);
                self.config.set_sync_secrets_password_hash(hash);
                self.config.set_sync_etag(etag.clone());
                let previous_synced_at = self.config.sync_last_synced_at();
                self.config
                    .set_sync_last_synced_at(chrono::Utc::now().timestamp());
                match config_persistence::save_full(&self.config_repository, &self.config) {
                    Ok(()) => {
                        let baseline = crate::sync::state::SyncBaseline::from_remote_payload(
                            &target,
                            payload,
                            etag.clone(),
                            chrono::Utc::now().timestamp(),
                        );
                        match crate::sync::state::SyncStateRepository::new()
                            .and_then(|repository| repository.save(&target, baseline))
                        {
                            Ok(()) => {
                                self.sync_runtime.status = t!("sync_reset_complete").into();
                                self.schedule_automatic_sync(false, cx);
                            }
                            Err(err) => {
                                self.sync_runtime.set_failed(
                                    t!("sync_failed", error = format!("{err:#}")).into(),
                                );
                            }
                        }
                    }
                    Err(err) => {
                        self.config.set_sync_last_synced_at(previous_synced_at);
                        self.sync_runtime.status = t!(
                            "sync_failed",
                            error = format!("{err:#}; {}", t!("sync_reset_local_save_failed"))
                        )
                        .into();
                    }
                }
            }
            Err(err) => {
                self.sync_runtime
                    .set_failed(t!("sync_failed", error = format!("{err:#}")).into());
            }
        }
    }

    fn handle_sync_privacy_password_initialization_ready(
        &mut self,
        credentials: SyncCredentials,
        password: String,
        cx: &mut Context<Self>,
    ) {
        if self.set_sync_include_secrets(true, cx) {
            if let Some(dialog) = self.sync_runtime.secrets_password_dialog.take() {
                let token = dialog.token;
                let input = dialog.settings_password_input;
                let input_password = password.clone();
                let view = cx.entity();
                let _ = dialog.window.update(cx, move |_, window, cx| {
                    input.update(cx, |input, cx| {
                        input.set_value(input_password.clone(), window, cx);
                    });
                    view.update(cx, |this, cx| {
                        this.dismiss_dialog(token, window, cx);
                    });
                });
            }
            let form = config_sync::SyncFormSnapshot::with_privacy_password(credentials, password);
            self.upload_sync_config(form, cx);
        } else {
            let message = self.sync_runtime.status.clone();
            if let Some(dialog) = self.sync_runtime.secrets_password_dialog.as_mut() {
                dialog.status = crate::app::SyncSecretsPasswordDialogStatus::Failed;
                dialog.message = Some(message);
            }
        }
    }

    fn handle_sync_privacy_password_checked(
        &mut self,
        password: String,
        status: PrivacyPasswordStatus,
        cx: &mut Context<Self>,
    ) {
        match status {
            PrivacyPasswordStatus::Verified => {
                if self.set_sync_include_secrets(true, cx) {
                    if let Some(dialog) = self.sync_runtime.secrets_password_dialog.take() {
                        let token = dialog.token;
                        let input = dialog.settings_password_input;
                        let view = cx.entity();
                        let _ = dialog.window.update(cx, move |_, window, cx| {
                            input.update(cx, |input, cx| {
                                input.set_value(password.clone(), window, cx);
                            });
                            view.update(cx, |this, cx| {
                                this.dismiss_dialog(token, window, cx);
                            });
                        });
                    }
                } else {
                    let message = self.sync_runtime.status.clone();
                    if let Some(dialog) = self.sync_runtime.secrets_password_dialog.as_mut() {
                        dialog.status = crate::app::SyncSecretsPasswordDialogStatus::Failed;
                        dialog.message = Some(message);
                    }
                }
            }
            PrivacyPasswordStatus::Mismatch => {
                if let Some(dialog) = self.sync_runtime.secrets_password_dialog.as_mut() {
                    dialog.status = crate::app::SyncSecretsPasswordDialogStatus::PasswordMismatch;
                    dialog.message = Some(t!("sync_secret_toggle_password_incorrect").into());
                }
            }
            PrivacyPasswordStatus::Missing => {
                if let Some(dialog) = self.sync_runtime.secrets_password_dialog.as_mut() {
                    dialog.status = crate::app::SyncSecretsPasswordDialogStatus::PasswordRequired;
                    dialog.message = Some(t!("sync_secret_toggle_password_required").into());
                }
            }
            PrivacyPasswordStatus::NotConfigured => {
                if let Some(dialog) = self.sync_runtime.secrets_password_dialog.as_mut() {
                    dialog.status =
                        crate::app::SyncSecretsPasswordDialogStatus::RemotePasswordNotConfigured;
                    dialog.message = Some(t!("sync_secret_toggle_remote_password_missing").into());
                }
            }
        }
    }

    fn handle_sync_connection_verified(&mut self, _cx: &mut Context<Self>) {
        self.sync_runtime.status = t!("sync_connection_verified").into();
    }

    fn handle_sync_failed(&mut self, error: SyncFailure, cx: &mut Context<Self>) {
        let should_retry = matches!(
            error.category,
            crate::sync::SyncErrorCategory::Other | crate::sync::SyncErrorCategory::Conflict
        );
        self.sync_runtime.record_failure();
        self.sync_runtime.status = config_sync::sync_failure_status(&error).into();
        if self
            .sync_runtime
            .secrets_password_dialog
            .as_ref()
            .is_some_and(|dialog| {
                dialog.status == crate::app::SyncSecretsPasswordDialogStatus::Verifying
            })
        {
            let message = self.sync_runtime.status.clone();
            if let Some(dialog) = self.sync_runtime.secrets_password_dialog.as_mut() {
                dialog.status = crate::app::SyncSecretsPasswordDialogStatus::Failed;
                dialog.message = Some(message);
            }
        }
        if should_retry {
            self.schedule_automatic_sync_retry(cx);
        }
    }
}
