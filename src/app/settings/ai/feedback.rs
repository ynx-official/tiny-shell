//! Report the exact request's completion in the window that initiated it.
use crate::{
    ai::OperationReceipt,
    app::ai::error_text,
    feedback::{Feedback, FeedbackKind},
};
use gpui::{App, Window};
use rust_i18n::t;

fn failure_kind(code: &str) -> FeedbackKind {
    match code {
        "AI_NO_PROVIDER"
        | "AI_NO_MODEL"
        | "AI_DEFAULT_REQUIRED"
        | "AI_DEFAULT_REPLACE"
        | "AI_KEY_REQUIRED"
        | "AI_LOADING"
        | "AI_BUSY"
        | "AI_PROVIDER_DISABLED"
        | "AI_HTTPS_REQUIRED"
        | "AI_LOCAL_ONLY"
        | "AI_BAD_URL"
        | "AI_BASE_URL_REQUIRED"
        | "AI_ENDPOINT_KEY_REQUIRED"
        | "AI_INVALID_BUDGET"
        | "AI_SYNC_PASSWORD_REQUIRED"
        | "AI_KEY_LOCKED" => FeedbackKind::Warning,
        _ => FeedbackKind::Error,
    }
}

pub(super) fn report(
    result: Result<OperationReceipt, String>,
    success_key: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Option<String> {
    match result {
        Err(error) => {
            Feedback::show(window, cx, failure_kind(&error), error_text(&error));
            Some(error)
        }
        Ok(receipt) => {
            let target = window.window_handle();
            cx.spawn(async move |cx| {
                let result = receipt.wait().await;
                // A closed window is not replaced with another workspace: feedback belongs
                // to this action's origin. The write still completes if the view disappears.
                let _ = target.update(cx, |_, window, cx| match result {
                    Ok(()) => Feedback::success(window, cx, t!(success_key).to_string()),
                    Err(error) => {
                        Feedback::show(window, cx, failure_kind(&error), error_text(&error))
                    }
                });
            })
            .detach();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correctable_input_warns_but_failed_writes_and_requests_are_errors() {
        for code in [
            "AI_INVALID_BUDGET",
            "AI_DEFAULT_REQUIRED",
            "AI_BUSY",
            "AI_KEY_REQUIRED",
        ] {
            assert_eq!(failure_kind(code), FeedbackKind::Warning);
        }
        for code in ["AI_STORAGE_ERROR", "AI_INTERRUPTED", "HTTP_500", "unknown"] {
            assert_eq!(failure_kind(code), FeedbackKind::Error);
        }
    }
}
