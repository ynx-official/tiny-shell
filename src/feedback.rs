use gpui::{App, Entity, SharedString};

use crate::TinyShell;

mod presentation;
pub(crate) use presentation::{Feedback, FeedbackKind};

impl Feedback {
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
        cx.defer(move |cx| Self::show_for_owner_now(&owner, cx, kind, message, true));
    }

    /// Backend events follow a session's workspace, never an unrelated auxiliary window.
    pub(crate) fn show_for_workspace(
        owner: &Entity<TinyShell>,
        cx: &mut App,
        kind: FeedbackKind,
        message: impl Into<SharedString>,
    ) {
        let owner = owner.clone();
        let message = message.into();
        cx.defer(move |cx| Self::show_for_owner_now(&owner, cx, kind, message, false));
    }

    /// Resolve the target after the current entity update has completed. GPUI forbids reading an
    /// entity while it is already being updated, and save callbacks commonly run in that state.
    fn show_for_owner_now(
        owner: &Entity<TinyShell>,
        cx: &mut App,
        kind: FeedbackKind,
        message: SharedString,
        prefer_connection_manager: bool,
    ) {
        let owner_state = owner.read(cx);
        let owner_id = owner_state.session_owner_id;
        let connection_manager = owner_state.auxiliary_windows.connection_manager.handle;
        if prefer_connection_manager
            && let Some(target) = connection_manager
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
