//! Native notifications carry only an opaque, live pause identity and generic localized text.
use rust_i18n::t;
use std::{
    collections::VecDeque,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone)]
pub(super) struct Activation {
    pub conversation: String,
    pub token: String,
}
#[derive(Default)]
struct State {
    activations: VecDeque<Activation>,
    notice: Option<&'static str>,
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static STAMP: AtomicU64 = AtomicU64::new(1);
fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}
pub(crate) fn supported() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}
pub(super) fn stamp() -> u64 {
    STAMP.load(Ordering::Relaxed)
}
pub(crate) fn notice() -> Option<&'static str> {
    state().lock().ok().and_then(|s| s.notice)
}
fn report(key: &'static str) {
    if let Ok(mut s) = state().lock() {
        s.notice = Some(key);
    }
    STAMP.fetch_add(1, Ordering::Relaxed);
}
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn activate(activation: Option<Activation>) {
    if let Some(activation) = activation {
        if let Ok(mut s) = state().lock() {
            s.activations.push_back(activation);
        }
        STAMP.fetch_add(1, Ordering::Relaxed);
    }
}
pub(super) fn take_activations() -> Vec<Activation> {
    state()
        .lock()
        .map(|mut s| s.activations.drain(..).collect())
        .unwrap_or_default()
}
pub(crate) fn authorize() {
    #[cfg(target_os = "macos")]
    if let Ok(service) = super::service() {
        service.runtime.spawn(async {
            if mac_usernotifications::check_bundle().is_err() {
                report("ai_notification_bundle_required");
                return;
            }
            match mac_usernotifications::request_auth().await {
                Ok(true) => report("ai_notification_authorized"),
                Ok(false) => report("ai_notification_denied"),
                Err(_) => report("ai_notification_failed"),
            }
        });
    }
    #[cfg(target_os = "windows")]
    if let Ok(service) = super::service() {
        service.runtime.spawn_blocking(|| {
            report(if register_windows_identity().is_ok() {
                "ai_notification_authorized"
            } else {
                "ai_notification_failed"
            });
        });
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    report("ai_notification_unsupported");
}
pub(crate) fn test() {
    // A test is an explicit user action, so macOS may request permission here.
    send(None, true);
}
pub(super) fn paused(conversation: String, token: String) {
    send(
        Some(Activation {
            conversation,
            token,
        }),
        false,
    );
}
fn send(activation: Option<Activation>, test: bool) {
    let title = t!("ai_notification_title").to_string();
    let body = if test {
        t!("ai_notification_test_body")
    } else {
        t!("ai_notification_pause_body")
    }
    .to_string();
    #[cfg(target_os = "windows")]
    if let Ok(service) = super::service() {
        service.runtime.spawn_blocking(move || {
            if register_windows_identity().is_err() {
                report("ai_notification_failed");
                return;
            }
            let toast = tauri_winrt_notification::Toast::new("TinyShell.Desktop")
                .title(&title)
                .text1(&body)
                .sound(None)
                .on_activated(move |_| {
                    activate(activation.clone());
                    Ok(())
                });
            report(if toast.show().is_ok() {
                "ai_notification_sent"
            } else {
                "ai_notification_failed"
            });
        });
    }
    #[cfg(target_os = "macos")]
    if let Ok(service) = super::service() {
        service.runtime.spawn(async move {
            use mac_usernotifications::{AuthorizationStatus, Notification};
            if mac_usernotifications::check_bundle().is_err() {
                report("ai_notification_bundle_required");
                return;
            }
            if test && !matches!(mac_usernotifications::request_auth().await, Ok(true)) {
                report("ai_notification_denied");
                return;
            }
            let Ok(settings) = mac_usernotifications::get_notification_settings().await else {
                report("ai_notification_failed");
                return;
            };
            if !matches!(
                settings.authorization_status,
                AuthorizationStatus::Authorized | AuthorizationStatus::Provisional
            ) {
                report("ai_notification_denied");
                return;
            }
            let notification = Notification::new()
                .title(title)
                .message(body)
                .timeout(std::time::Duration::from_secs(3600));
            let Ok(handle) = notification.send().await else {
                report("ai_notification_failed");
                return;
            };
            report("ai_notification_sent");
            if activation.is_some()
                && let Ok(response) = handle.response().await
                && response.close_reason.is_none()
            {
                activate(activation);
            }
        });
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (activation, title, body);
        report("ai_notification_unsupported");
    }
}

#[cfg(target_os = "windows")]
fn register_windows_identity() -> std::io::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    // Per-user registration also supports portable builds without impersonating another app.
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey("Software\\Classes\\AppUserModelId\\TinyShell.Desktop")?;
    key.set_value("DisplayName", &"TinyShell")?;
    Ok(())
}
