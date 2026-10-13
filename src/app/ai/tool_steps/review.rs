//! Inline review uses the engine's prepared operation, never model-authored summaries.
use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _,
    Window, div,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use rust_i18n::t;
use std::rc::Rc;
use tiny_shell_mcp::{contracts::Call, engine::Review, tasks::State};

pub(crate) type LoadReview = Rc<dyn Fn(&str) -> Result<Review, String>>;
type DecideReview = Rc<dyn Fn(&Review, bool, &mut Window, &mut App) -> Result<(), String>>;
type AuthenticateReview = Rc<dyn Fn(&Review, &mut Window, &mut App)>;

#[derive(Clone)]
pub(crate) struct ReviewActions {
    pub load: LoadReview,
    pub decide: DecideReview,
    pub authenticate: AuthenticateReview,
}

pub(crate) struct ReviewScope {
    client: String,
    target_id: String,
    target_digest: String,
}
impl ReviewScope {
    pub fn new(conversation: &str, target_id: &str, target_digest: &str) -> Self {
        Self {
            client: format!("ai:{conversation}"),
            target_id: target_id.into(),
            target_digest: target_digest.into(),
        }
    }
    pub fn matches(&self, task: &str, review: &Review) -> bool {
        review.id == task
            && review.client == self.client
            && review.target.id == self.target_id
            && review.target.digest == self.target_digest
            && review.state == State::AwaitingUser
    }
}

pub(crate) fn same_operation(displayed: &Review, current: &Review) -> bool {
    displayed.id == current.id
        && displayed.client == current.client
        && displayed.target.id == current.target.id
        && displayed.target.digest == current.target.digest
        && displayed.state == current.state
        && displayed.error.as_ref().map(|e| &e.code) == current.error.as_ref().map(|e| &e.code)
        && displayed.preview == current.preview
        && serde_json::to_value(&displayed.call)
            .ok()
            .zip(serde_json::to_value(&current.call).ok())
            .is_some_and(|(a, b)| a == b)
}

pub(super) struct ReviewView {
    task: String,
    actions: ReviewActions,
    submitted: Option<bool>,
    error: Option<String>,
}
impl ReviewView {
    pub fn new(task: String, actions: ReviewActions) -> Self {
        Self {
            task,
            actions,
            submitted: None,
            error: None,
        }
    }
    fn decide(
        &mut self,
        review: &Review,
        approved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.submitted.is_some() {
            return;
        }
        match (self.actions.decide)(review, approved, window, cx) {
            Ok(()) => {
                self.submitted = Some(approved);
                self.error = None;
            }
            Err(code) => self.error = Some(code),
        }
        cx.notify();
        cx.refresh_windows();
    }
}

impl Render for ReviewView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = v_flex().w_full().min_w_0().gap_2().px_3().pb_3();
        if let Some(approved) = self.submitted {
            return body.child(
                div().text_xs().child(
                    t!(if approved {
                        "mcp_approval_sent"
                    } else {
                        "mcp_rejected"
                    })
                    .to_string(),
                ),
            );
        }
        let review = match (self.actions.load)(&self.task) {
            Ok(review) => review,
            Err(code) => {
                return body.child(unavailable(&code, cx));
            }
        };
        body = body.child(div().text_xs().whitespace_normal().child(format!(
            "{} · {}@{}:{}",
            review.target.name, review.target.user, review.target.host, review.target.port
        )));
        let auth_required = review
            .error
            .as_ref()
            .is_some_and(|e| e.code == "AUTH_REQUIRED");
        let mut can_approve = !auth_required && review.error.is_none();
        if auth_required {
            body = body.child(
                div()
                    .text_xs()
                    .whitespace_normal()
                    .child(t!("ai_review_auth_required").to_string()),
            );
        } else {
            match &review.call {
                Call::RunCommand(command) => {
                    if let Some(cwd) = &command.cwd {
                        body = body.child(
                            div()
                                .debug_selector(|| "ai-review-cwd".into())
                                .text_xs()
                                .whitespace_normal()
                                .child(t!("mcp_working_directory", path = cwd).to_string()),
                        );
                    }
                    body = body.child(super::view::raw_block(
                        format!("ai-review-command-{}", review.id),
                        t!("ai_step_run_command").to_string(),
                        command.command.clone(),
                        cx,
                    ));
                }
                Call::WriteFile(file) => {
                    body = body.child(super::view::raw_block(
                        format!("ai-review-path-{}", review.id),
                        t!("ai_review_path").to_string(),
                        file.path.clone(),
                        cx,
                    ));
                    if let Some(old) = review.preview["old_content"].as_str() {
                        body = body.child(super::view::raw_block(
                            format!("ai-review-old-{}", review.id),
                            t!("mcp_old_content").to_string(),
                            old.into(),
                            cx,
                        ));
                    } else {
                        can_approve = false;
                        body = body.child(unavailable("PREVIEW_UNAVAILABLE", cx));
                    }
                    body = body.child(super::view::raw_block(
                        format!("ai-review-new-{}", review.id),
                        t!("mcp_new_content").to_string(),
                        file.content.clone(),
                        cx,
                    ));
                }
                _ => {
                    can_approve = false;
                    body = body.child(unavailable("UNSUPPORTED_REVIEW", cx));
                }
            }
        }
        if let Some(code) = self.error.as_deref().or(review
            .error
            .as_ref()
            .filter(|_| !auth_required)
            .map(|e| e.code.as_str()))
        {
            body = body.child(
                div()
                    .debug_selector(|| "ai-review-error".into())
                    .text_xs()
                    .whitespace_normal()
                    .text_color(cx.theme().danger)
                    .child(t!("mcp_action_failed", code = code).to_string()),
            );
        }
        let mut buttons = h_flex().gap_2().flex_wrap();
        if auth_required {
            let authenticate = self.actions.authenticate.clone();
            let auth = review.clone();
            buttons = buttons.child(
                Button::new("ai-review-authenticate")
                    .debug_selector(|| "ai-review-authenticate".into())
                    .small()
                    .primary()
                    .label(t!("mcp_authenticate").to_string())
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        authenticate(&auth, window, cx);
                    }),
            );
        } else if can_approve {
            body = body.child(
                div()
                    .text_xs()
                    .whitespace_normal()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("ai_review_once").to_string()),
            );
            let approve = review.clone();
            buttons = buttons.child(
                Button::new("ai-review-approve")
                    .debug_selector(|| "ai-review-approve".into())
                    .small()
                    .primary()
                    .label(t!("mcp_approve").to_string())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.decide(&approve, true, window, cx);
                    })),
            );
        }
        body.child(
            buttons.child(
                Button::new("ai-review-deny")
                    .debug_selector(|| "ai-review-deny".into())
                    .small()
                    .outline()
                    .label(t!("mcp_deny").to_string())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.decide(&review, false, window, cx);
                    })),
            ),
        )
    }
}

fn unavailable(code: &str, cx: &App) -> gpui::Div {
    div()
        .debug_selector(|| "ai-review-unavailable".into())
        .text_xs()
        .whitespace_normal()
        .text_color(cx.theme().warning)
        .child(t!("ai_review_unavailable", code = code).to_string())
}

#[path = "review_tests.rs"]
#[cfg(test)]
mod tests;
