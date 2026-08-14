use gpui::{App, Entity, SharedString, Window};
use gpui_component::{WindowExt as _, notification::Notification};

use crate::TinyShell;

/// Unified transient feedback for user-triggered actions.
///
/// Policy:
/// - `success`: a requested mutation completed successfully.
/// - `info`: a neutral, completed event that is useful to surface.
/// - `warning`: the action cannot proceed or completed with a recoverable caveat.
/// - `error`: the requested action failed.
///
/// Long-running states such as connecting, syncing, or downloading should stay in the
/// owning view's status/progress UI. Only the final outcome should be sent here.
pub(crate) struct Feedback;

impl Feedback {
    pub(crate) fn success(
        window: &mut Window,
        cx: &mut App,
        message: impl Into<SharedString>,
    ) {
        window.push_notification(Notification::success(message), cx);
    }

    pub(crate) fn info(
        window: &mut Window,
        cx: &mut App,
        message: impl Into<SharedString>,
    ) {
        window.push_notification(Notification::info(message), cx);
    }

    pub(crate) fn warning(
        window: &mut Window,
        cx: &mut App,
        message: impl Into<SharedString>,
    ) {
        window.push_notification(Notification::warning(message), cx);
    }

    pub(crate) fn error(
        window: &mut Window,
        cx: &mut App,
        message: impl Into<SharedString>,
    ) {
        window.push_notification(Notification::error(message), cx);
    }

    /// Surface success in the owning TinyShell workspace when an auxiliary window is about to
    /// close. Pushing into the closing window would make the notification disappear immediately.
    pub(crate) fn success_for_owner(
        owner: &Entity<TinyShell>,
        cx: &mut App,
        message: impl Into<SharedString>,
    ) {
        let message = message.into();
        let owner_id = owner.read(cx).session_owner_id;
        let target = {
            let registry = crate::app::window_registry();
            let guard = match registry.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    tracing::warn!("window registry lock was poisoned while showing feedback");
                    poisoned.into_inner()
                }
            };
            guard
                .iter()
                .filter(|entry| {
                    cx.windows().contains(&entry.window_handle)
                        && entry.entity.read(cx).session_owner_id == owner_id
                })
                .max_by_key(|entry| entry.activation_seq)
                .map(|entry| entry.window_handle)
        };

        if let Some(target) = target {
            let _ = target.update(cx, move |_, window, cx| {
                Self::success(window, cx, message);
            });
        }
    }
}
