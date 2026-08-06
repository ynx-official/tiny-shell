use gpui::{App, AppContext as _, Entity, Window};
use gpui_component::input::InputState;
use rust_i18n::t;

use crate::{TinyShell, session::config::ConfigStore};

#[derive(Clone)]
pub(crate) struct ProxySettingsInputs {
    pub(crate) host: Entity<InputState>,
    pub(crate) port: Entity<InputState>,
    pub(crate) user: Entity<InputState>,
    pub(crate) password: Entity<InputState>,
}

#[derive(Clone)]
pub(crate) struct SyncSettingsInputs {
    pub(crate) endpoint: Entity<InputState>,
    pub(crate) username: Entity<InputState>,
    pub(crate) webdav_password: Entity<InputState>,
    pub(crate) s3_endpoint: Entity<InputState>,
    pub(crate) s3_region: Entity<InputState>,
    pub(crate) s3_bucket: Entity<InputState>,
    pub(crate) s3_object_key: Entity<InputState>,
    pub(crate) s3_access_key: Entity<InputState>,
    pub(crate) s3_secret_key: Entity<InputState>,
    pub(crate) s3_session_token: Entity<InputState>,
    pub(crate) privacy_password: Entity<InputState>,
    pub(crate) interval_hours: Entity<InputState>,
}

#[derive(Clone)]
pub(crate) struct UpdateSettingsInputs {
    pub(crate) interval_hours: Entity<InputState>,
}

#[derive(Clone)]
pub(crate) struct SettingsInputs {
    pub(crate) proxy: ProxySettingsInputs,
    pub(crate) sync: SyncSettingsInputs,
    pub(crate) update: UpdateSettingsInputs,
}

impl SettingsInputs {
    pub(crate) fn from_main(owner: &TinyShell) -> Self {
        owner.settings_inputs.clone()
    }

    pub(crate) fn new(config: &ConfigStore, window: &mut Window, cx: &mut App) -> Self {
        let proxy = ProxySettingsInputs {
            host: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("proxy_host").to_string())
                    .default_value(config.global_proxy_host())
            }),
            port: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("proxy_port").to_string())
                    .default_value(
                        config
                            .global_proxy_port()
                            .map(|port| port.to_string())
                            .unwrap_or_default(),
                    )
            }),
            user: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("proxy_user").to_string())
                    .default_value(config.global_proxy_user())
            }),
            password: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("proxy_password").to_string())
                    .masked(true)
                    .default_value(config.global_proxy_password())
            }),
        };
        let sync = SyncSettingsInputs {
            endpoint: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_endpoint_placeholder").to_string())
                    .default_value(config.sync_endpoint())
            }),
            username: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_username").to_string())
                    .default_value(config.sync_username())
            }),
            webdav_password: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_webdav_password").to_string())
                    .masked(true)
                    .default_value(
                        crate::app::config_sync::open_webdav_password(
                            config.sync_webdav_password_sealed(),
                        )
                        .unwrap_or_default(),
                    )
            }),
            s3_endpoint: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_s3_endpoint_placeholder").to_string())
                    .default_value(config.sync_s3_endpoint())
            }),
            s3_region: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_s3_region_placeholder").to_string())
                    .default_value(config.sync_s3_region())
            }),
            s3_bucket: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_s3_bucket").to_string())
                    .default_value(config.sync_s3_bucket())
            }),
            s3_object_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_s3_object_key_placeholder").to_string())
                    .default_value(config.sync_s3_object_key())
            }),
            s3_access_key: cx.new(|cx| {
                InputState::new(window, cx).placeholder(t!("sync_s3_access_key").to_string())
            }),
            s3_secret_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_s3_secret_key").to_string())
                    .masked(true)
            }),
            s3_session_token: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_s3_session_token").to_string())
                    .masked(true)
            }),
            privacy_password: cx.new(|cx| {
                let mut state = InputState::new(window, cx)
                    .placeholder(t!("sync_privacy_password").to_string())
                    .masked(true);
                if !config.sync_secrets_password_sealed().is_empty() {
                    let hardware_uuid = crate::session::config::hardware_uuid();
                    if let Ok(plaintext) = crate::crypto::open_with_hardware_key(
                        config.sync_secrets_password_sealed(),
                        &hardware_uuid,
                    ) {
                        state = state.default_value(&plaintext);
                    }
                }
                state
            }),
            interval_hours: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_interval_placeholder").to_string())
                    .default_value(config.sync_interval_hours().to_string())
            }),
        };
        let update = UpdateSettingsInputs {
            interval_hours: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_interval_placeholder").to_string())
                    .default_value(config.update_interval_hours().to_string())
            }),
        };
        Self {
            proxy,
            sync,
            update,
        }
    }

    pub(crate) fn all_inputs(&self) -> impl Iterator<Item = Entity<InputState>> {
        [
            self.proxy.host.clone(),
            self.proxy.port.clone(),
            self.proxy.user.clone(),
            self.proxy.password.clone(),
            self.sync.endpoint.clone(),
            self.sync.username.clone(),
            self.sync.webdav_password.clone(),
            self.sync.s3_endpoint.clone(),
            self.sync.s3_region.clone(),
            self.sync.s3_bucket.clone(),
            self.sync.s3_object_key.clone(),
            self.sync.s3_access_key.clone(),
            self.sync.s3_secret_key.clone(),
            self.sync.s3_session_token.clone(),
            self.sync.privacy_password.clone(),
            self.sync.interval_hours.clone(),
            self.update.interval_hours.clone(),
        ]
        .into_iter()
    }
}
