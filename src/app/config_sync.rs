use gpui::{Context, Entity, SharedString};
use gpui_component::input::InputState;
use rust_i18n::t;

use crate::{
    TinyShell,
    sync::{self, SyncBackendCredentials, SyncCredentials, SyncPayload, SyncResult},
    terminal::BackendEvent,
};

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

    pub(crate) fn upload_sync_config(&mut self, cx: &mut Context<Self>) {
        let Some(credentials) = self.begin_sync(t!("sync_uploading").into(), cx) else {
            return;
        };
        let payload = SyncPayload::new(
            self.config.sync_device_id().to_string(),
            self.config.sessions().to_vec(),
        );
        let expected_etag = self.config.sync_etag().map(str::to_string);
        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = match sync::upload(credentials, payload, expected_etag).await {
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
        let events = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = match sync::download(credentials).await {
                Ok((payload, etag)) => SyncResult::Downloaded { payload, etag },
                Err(err) => SyncResult::Failed(format!("{err:#}")),
            };
            let _ = events.send(BackendEvent::SyncFinished(result));
        });
    }
}
