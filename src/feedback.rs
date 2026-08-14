use gpui::{App, Entity, SharedString, Window};
use gpui_component::{WindowExt as _, notification::Notification};

use crate::TinyShell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeedbackKind {
    Info,
    Success,
    Warning,
    Error,
}

/// Unified transient feedback for user-triggered actions.
///
/// Policy:
/// - `Success`: a requested mutation completed successfully.
/// - `Info`: a neutral, completed event that is useful to surface.
/// - `Warning`: the action cannot proceed or completed with a recoverable caveat.
/// - `Error`: the requested action failed.
///
/// Long-running states such as connecting, syncing, or downloading should stay in the
/// owning view's status/progress UI. Only the final outcome should be sent here.
pub(crate) struct Feedback;

impl Feedback {
    pub(crate) fn show(
        window: &mut Window,
        cx: &mut App,
        kind: FeedbackKind,
        message: impl Into<SharedString>,
    ) {
        let message = message.into();
        let notification = match kind {
            FeedbackKind::Info => Notification::info(message),
            FeedbackKind::Success => Notification::success(message),
            FeedbackKind::Warning => Notification::warning(message),
            FeedbackKind::Error => Notification::error(message),
        };
        window.push_notification(notification, cx);
    }

    pub(crate) fn success(window: &mut Window, cx: &mut App, message: impl Into<SharedString>) {
        Self::show(window, cx, FeedbackKind::Success, message);
    }

    pub(crate) fn info(window: &mut Window, cx: &mut App, message: impl Into<SharedString>) {
        Self::show(window, cx, FeedbackKind::Info, message);
    }

    pub(crate) fn warning(window: &mut Window, cx: &mut App, message: impl Into<SharedString>) {
        Self::show(window, cx, FeedbackKind::Warning, message);
    }

    pub(crate) fn error(window: &mut Window, cx: &mut App, message: impl Into<SharedString>) {
        Self::show(window, cx, FeedbackKind::Error, message);
    }

    pub(crate) fn success_for_owner(
        owner: &Entity<TinyShell>,
        cx: &mut App,
        message: impl Into<SharedString>,
    ) {
        Self::show_for_owner(owner, cx, FeedbackKind::Success, message);
    }

    pub(crate) fn error_for_owner(
        owner: &Entity<TinyShell>,
        cx: &mut App,
        message: impl Into<SharedString>,
    ) {
        Self::show_for_owner(owner, cx, FeedbackKind::Error, message);
    }

    /// Route feedback to the TinyShell workspace that owns an auxiliary window.
    /// This is used when the auxiliary window is about to close, or when opening it failed and
    /// there is no native child window available to render the notification.
    pub(crate) fn show_for_owner(
        owner: &Entity<TinyShell>,
        cx: &mut App,
        kind: FeedbackKind,
        message: impl Into<SharedString>,
    ) {
        let message = message.into();
        let owner = owner.clone();
        cx.defer(move |cx| Self::show_for_owner_now(&owner, cx, kind, message));
    }

    /// Resolve the target after the current entity update has completed. GPUI forbids reading an
    /// entity while it is already being updated, and save callbacks commonly run in that state.
    fn show_for_owner_now(
        owner: &Entity<TinyShell>,
        cx: &mut App,
        kind: FeedbackKind,
        message: SharedString,
    ) {
        let owner_state = owner.read(cx);
        let owner_id = owner_state.session_owner_id;
        let connection_manager = owner_state.auxiliary_windows.connection_manager.handle;
        if let Some(target) = connection_manager
            && cx.windows().contains(&target)
            && target
                .update(cx, |_, window, cx| {
                    Self::show(window, cx, kind, message.clone());
                })
                .is_ok()
        {
            return;
        }
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
                Self::show(window, cx, kind, message);
            });
        }
    }
}
