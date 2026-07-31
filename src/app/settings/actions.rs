use gpui::{AnyWindowHandle, App, Context, KeyDownEvent, Window};

use crate::{
    TinyShell, app::settings_window::ProxySettingsInputs, session::config::UpdateCheckMode,
};

pub(crate) fn parse_hour_interval(value: &str) -> Option<u32> {
    value
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|hours| (1..=8_760).contains(hours))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProxyKind {
    Socks5,
    Http,
}

impl ProxyKind {
    pub(crate) fn from_config(value: &str) -> Self {
        match value {
            "http" => Self::Http,
            _ => Self::Socks5,
        }
    }

    pub(crate) const fn config_value(self) -> &'static str {
        match self {
            Self::Socks5 => "socks5",
            Self::Http => "http",
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ProxyFormValues {
    pub(crate) kind: ProxyKind,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) user: String,
    pub(crate) password: String,
}

impl ProxyFormValues {
    pub(crate) fn parse(
        kind: ProxyKind,
        host: &str,
        port: &str,
        user: &str,
        password: &str,
    ) -> Option<Self> {
        let host = host.trim();
        if host.is_empty() {
            return None;
        }
        let port = port.trim().parse().ok()?;
        if port == 0 {
            return None;
        }
        Some(Self {
            kind,
            host: host.to_string(),
            port,
            user: user.trim().to_string(),
            password: password.to_string(),
        })
    }

    pub(crate) fn capture(kind: ProxyKind, inputs: &ProxySettingsInputs, cx: &App) -> Option<Self> {
        Self::parse(
            kind,
            inputs.host.read(cx).value().as_ref(),
            inputs.port.read(cx).value().as_ref(),
            inputs.user.read(cx).value().as_ref(),
            inputs.password.read(cx).value().as_ref(),
        )
    }
}

impl TinyShell {
    pub(crate) fn handle_settings_keybinding_input(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(action) = self.recording_action.clone() else {
            return;
        };

        window.prevent_default();
        cx.stop_propagation();

        if event.keystroke.key == "escape" {
            self.cancel_settings_keybinding_recording(cx);
            return;
        }

        let Some(new_key) = crate::app::keybinding_recorder::normalize_recorded_keystroke(event)
        else {
            return;
        };

        if let Some((_, conflict_label)) =
            crate::app::keybinding_recorder::find_conflict(&self.config, &action, &new_key)
        {
            let formatted = crate::app::keybinding_recorder::format_keystroke(&new_key);
            self.recording_action = None;
            self.keybind_error = Some((
                action,
                rust_i18n::t!("keybind_conflict", key = formatted, action = conflict_label)
                    .to_string(),
            ));
            cx.notify();
            return;
        }

        self.recording_action = None;
        self.keybind_error = None;
        self.config.set_key_binding(&action, &new_key);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(crate) fn cancel_settings_keybinding_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording_action.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn set_proxy_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.config.set_use_proxy(enabled);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(crate) fn set_environment_proxy_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.config.set_read_env_proxy(enabled);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(crate) fn set_update_mode(
        &mut self,
        mode: UpdateCheckMode,
        window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        self.config.set_update_check_mode(mode);
        self.mark_config_preferences_dirty();
        self.schedule_automatic_update_checks(window, false, cx);
        cx.notify();
    }

    pub(crate) fn set_update_notifications(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.config.set_update_notify(enabled);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(crate) fn set_proxy_kind(&mut self, kind: ProxyKind, cx: &mut Context<Self>) {
        self.global_proxy_type = kind.config_value().to_string();
        cx.notify();
    }

    pub(crate) fn save_proxy_settings(
        &mut self,
        inputs: &ProxySettingsInputs,
        cx: &mut Context<Self>,
    ) -> bool {
        let kind = ProxyKind::from_config(&self.global_proxy_type);
        let Some(values) = ProxyFormValues::capture(kind, inputs, cx) else {
            return false;
        };

        self.config
            .set_global_proxy_type(values.kind.config_value().to_string());
        self.config.set_global_proxy_host(values.host);
        self.config.set_global_proxy_port(Some(values.port));
        self.config.set_global_proxy_user(values.user);
        self.config.set_global_proxy_password(values.password);
        self.mark_config_preferences_dirty();
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{ProxyFormValues, ProxyKind, parse_hour_interval};

    #[test]
    fn hour_interval_accepts_supported_range_only() {
        assert_eq!(parse_hour_interval(" 24 "), Some(24));
        assert_eq!(parse_hour_interval("1"), Some(1));
        assert_eq!(parse_hour_interval("8760"), Some(8_760));
        assert_eq!(parse_hour_interval("0"), None);
        assert_eq!(parse_hour_interval("8761"), None);
        assert_eq!(parse_hour_interval("invalid"), None);
    }

    #[test]
    fn proxy_form_requires_host_and_valid_port() {
        assert!(ProxyFormValues::parse(ProxyKind::Socks5, "", "1080", "", "").is_none());
        assert!(
            ProxyFormValues::parse(ProxyKind::Socks5, "localhost", "invalid", "", "").is_none()
        );
        assert!(ProxyFormValues::parse(ProxyKind::Http, "localhost", "0", "", "").is_none());
    }

    #[test]
    fn proxy_form_normalizes_text_values() {
        let values = ProxyFormValues::parse(
            ProxyKind::Http,
            " proxy.example.com ",
            "8080",
            " user ",
            " secret ",
        )
        .expect("valid proxy form");

        assert_eq!(values.kind, ProxyKind::Http);
        assert_eq!(values.host, "proxy.example.com");
        assert_eq!(values.port, 8080);
        assert_eq!(values.user, "user");
        assert_eq!(values.password, " secret ");
    }
}
