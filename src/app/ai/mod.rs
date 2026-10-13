mod composer;
mod markdown;
mod panel;
mod scroll;
mod tool_steps;
use crate::{
    TinyShell,
    ai::{BoundTarget, Service},
};
use gpui::{AppContext as _, Context, Window};
pub(crate) use panel::AiPanel;
use std::sync::{Arc, OnceLock};
static SERVICE: OnceLock<Result<Arc<Service>, String>> = OnceLock::new();
pub(crate) fn initialize(runtime: &tokio::runtime::Handle) {
    SERVICE.get_or_init(|| {
        let directory = crate::session::config::ConfigStore::config_directory()
            .map_err(|_| "AI_STORAGE_ERROR".to_owned())?;
        Ok(Service::new(
            directory,
            runtime.clone(),
            super::mcp::service().ok().map(|s| s.engine.clone()),
        ))
    });
}
pub(crate) fn service() -> Result<Arc<Service>, String> {
    SERVICE
        .get()
        .cloned()
        .unwrap_or_else(|| Err("AI_LOADING".into()))
}
pub(crate) fn icon() -> gpui_component::Icon {
    gpui_component::Icon::default().path("icons/ai-sparkles.svg")
}
pub(crate) fn target(
    session: &crate::session::config::Session,
    config: &crate::session::config::ConfigStore,
) -> BoundTarget {
    let t = crate::sftp::mcp_service::target(session, config);
    BoundTarget {
        id: t.id,
        name: t.name,
        host: t.host,
        port: t.port,
        user: t.user,
        digest: t.digest,
    }
}

pub(crate) fn current_target(
    session: &crate::session::config::Session,
    config: &crate::session::config::ConfigStore,
) -> Option<BoundTarget> {
    if session.connection_type != crate::session::config::ConnectionType::Ssh {
        return None;
    }
    let saved = config.get(&session.id)?;
    if saved.connection_type != crate::session::config::ConnectionType::Ssh
        || session.host != saved.host
        || session.port != saved.port
        || session.user != saved.user
        || session.proxy_type != saved.proxy_type
        || session.proxy_host != saved.proxy_host
        || session.proxy_port != saved.proxy_port
        || session.proxy_user != saved.proxy_user
    {
        return None;
    }
    // Terminals expand managed keys and may hold login-only credentials. Those are
    // execution details, while the engine authorizes the saved connection snapshot.
    Some(target(saved, config))
}
impl TinyShell {
    pub(crate) fn poll_ai(&mut self) -> bool {
        let stamp = service().map(|s| s.stamp()).unwrap_or(0);
        if stamp == self.ai_stamp {
            false
        } else {
            self.ai_stamp = stamp;
            true
        }
    }
    pub(crate) fn toggle_ai(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_open {
            self.ai_open = false;
        } else {
            if self.tool_panel.open {
                self.hide_tool_panel(window, cx);
            }
            if self.ai_panel.is_none() {
                let owner = cx.entity();
                self.ai_panel = Some(cx.new(|cx| AiPanel::new(owner, window, cx)));
            }
            self.ai_open = true;
        }
        cx.notify();
    }
    pub(crate) fn show_ai_settings(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index == 5 {
            self.mcp_settings_navigation = self.mcp_settings_navigation.wrapping_add(1);
        }
        self.settings_entry = Some(index);
        self.show_settings_window(window, cx);
        cx.notify();
    }
}
pub(crate) fn error_text(code: &str) -> String {
    use rust_i18n::t;
    let key = match code {
        "AI_SAVED" => "ai_saved",
        "AI_TEST_OK" => "ai_test_ok",
        "AI_NO_PROVIDER" => "ai_unconfigured",
        "AI_NO_MODEL" => "ai_no_model",
        "AI_KEY_REQUIRED" => "ai_key_required",
        "AI_LOADING" => "loading",
        "AI_BUSY" => "ai_busy",
        "AI_ACCESS_CHANGED" => "ai_access_changed",
        "AI_PROVIDER_DISABLED" => "ai_provider_disabled",
        "AI_HTTPS_REQUIRED" | "AI_LOCAL_ONLY" => "ai_https_required",
        "AI_BAD_URL" | "AI_BASE_URL_REQUIRED" => "ai_bad_url",
        "AI_ENDPOINT_KEY_REQUIRED" => "ai_endpoint_key_required",
        "AI_VAULT_UNAVAILABLE" => "ai_vault_unavailable",
        "AI_HISTORY_LOCKED" => "ai_history_locked",
        "AI_HISTORY_CORRUPT" => "ai_history_corrupt",
        "AI_STORAGE_ERROR" | "AI_HISTORY_LIMIT" => "ai_storage_error",
        "AI_CANCELLED" | "AI_INTERRUPTED" => "ai_interrupted",
        "AI_TOOL_LIMIT" => "ai_tool_limit",
        "AI_NETWORK_ERROR" | "AI_TIMEOUT" | "AI_STREAM_INTERRUPTED" => "ai_network_error",
        "AI_INVALID_NAME" => "ai_invalid_name",
        "AI_INVALID_MESSAGE" => "ai_invalid_message",
        _ => "ai_request_error",
    };
    if key == "ai_request_error" {
        format!("{} · {}", t!(key), code)
    } else {
        t!(key).to_string()
    }
}

pub(crate) fn ssh_status_key(
    policy: Option<tiny_shell_mcp::engine::AiPolicy>,
    has_target: bool,
    target_current: bool,
    supports_tools: bool,
) -> &'static str {
    match policy {
        None => "ai_ssh_unavailable",
        Some(p) if !p.enabled => "ai_ssh_disabled",
        Some(_) if !has_target => "ai_no_ssh_target",
        Some(_) if !target_current => "ai_ssh_target_changed",
        Some(_) if !supports_tools => "ai_model_no_tools",
        Some(p) => crate::app::settings::mcp::mode_key(p.mode),
    }
}

#[cfg(test)]
mod permission_status_tests {
    use super::*;
    use tiny_shell_mcp::{engine::AiPolicy, policy::Mode};

    #[test]
    fn runtime_credentials_do_not_change_the_saved_authorization_identity() {
        use crate::session::config::{AuthMethod, ConfigStore, ManagedKey, Session};
        let mut config = ConfigStore::in_memory();
        let saved = Session::password(
            "example.invalid".into(),
            22,
            "fixture".into(),
            String::new(),
        );
        config.upsert(saved.clone());
        let mut runtime = saved.clone();
        runtime.password = "ephemeral-password".into();
        assert_eq!(
            current_target(&runtime, &config).unwrap().digest,
            target(&saved, &config).digest,
            "a login-only password must not change the AI authorization identity"
        );
        let key = ManagedKey {
            id: uuid::Uuid::new_v4().to_string(),
            name: "fixture".into(),
            key_type: "unknown".into(),
            fingerprint: String::new(),
            inline_content: "synthetic-key".into(),
            passphrase: "synthetic-passphrase".into(),
            created_at: 0,
        };
        config.upsert_managed_key(key.clone());
        let mut saved = Session::password(
            "example.invalid".into(),
            22,
            "fixture".into(),
            String::new(),
        );
        saved.auth = AuthMethod::Key;
        saved.managed_key_id = Some(key.id.clone());
        config.upsert(saved.clone());
        let saved = config.get(&saved.id).unwrap();
        let mut runtime = saved.clone();
        runtime.private_key_inline = key.inline_content;
        runtime.passphrase = key.passphrase;
        assert_eq!(
            current_target(&runtime, &config).unwrap().digest,
            target(saved, &config).digest,
            "the terminal's expanded managed key must retain the saved authorization identity"
        );
    }

    #[test]
    fn current_target_does_not_silently_rebind_a_changed_or_unsaved_endpoint() {
        use crate::session::config::{ConfigStore, ConnectionType, Session};
        let mut config = ConfigStore::in_memory();
        let active = Session::password(
            "example.invalid".into(),
            22,
            "fixture".into(),
            String::new(),
        );
        assert!(current_target(&active, &config).is_none());
        config.upsert(active.clone());
        assert!(current_target(&active, &config).is_some());
        let mut changed = active.clone();
        changed.host = "changed.invalid".into();
        config.upsert(changed);
        assert!(current_target(&active, &config).is_none());
        let mut changed = active.clone();
        changed.proxy_host = "proxy-changed.invalid".into();
        config.upsert(changed);
        assert!(current_target(&active, &config).is_none());
        let mut changed = active.clone();
        changed.connection_type = ConnectionType::Rdp;
        config.upsert(changed);
        assert!(current_target(&active, &config).is_none());
    }

    #[test]
    fn ssh_status_distinguishes_global_access_target_and_model() {
        let enabled = Some(AiPolicy {
            enabled: true,
            mode: Mode::Controlled,
        });
        assert_eq!(ssh_status_key(None, true, true, true), "ai_ssh_unavailable");
        assert_eq!(
            ssh_status_key(Some(AiPolicy::default()), true, true, true),
            "ai_ssh_disabled"
        );
        assert_eq!(
            ssh_status_key(enabled, false, false, true),
            "ai_no_ssh_target"
        );
        assert_eq!(
            ssh_status_key(enabled, true, false, true),
            "ai_ssh_target_changed"
        );
        assert_eq!(
            ssh_status_key(enabled, true, true, false),
            "ai_model_no_tools"
        );
        assert_eq!(ssh_status_key(enabled, true, true, true), "mcp_controlled");
    }
}
