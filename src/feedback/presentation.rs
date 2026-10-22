use gpui::{App, SharedString, Window};
use gpui_component::{WindowExt as _, notification::Notification};
use rust_i18n::t;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeedbackKind {
    Info,
    Success,
    Warning,
    Error,
}

impl FeedbackKind {
    fn title(self) -> String {
        t!(match self {
            Self::Info => "feedback_info",
            Self::Success => "feedback_success",
            Self::Warning => "feedback_warning",
            Self::Error => "feedback_error",
        })
        .to_string()
    }
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
        window.push_notification(notification.title(kind.title()), cx);
    }

    /// Explicit copy buttons share feedback; automatic terminal/clipboard sync stays silent.
    pub(crate) fn copy(window: &mut Window, cx: &mut App, text: String) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        Self::success(window, cx, t!("feedback_copied").to_string());
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
}
