//! Followup preferences save independently of budget and notification drafts.
use super::{error_text, note, service};
use gpui::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    v_flex,
};
use rust_i18n::t;

pub(super) struct FollowupForm {
    enabled: bool,
    loaded: bool,
    error: Option<String>,
}

impl FollowupForm {
    pub(super) fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            enabled: true,
            loaded: false,
            error: None,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = (|| {
            let service = service()?;
            let mut execution = service.snapshot().settings.execution;
            execution.followup_suggestions = self.enabled;
            service
                .save_execution(execution)
                .map_err(|error| error.to_string())
        })();
        self.error = super::feedback::report(result, "feedback_saved", window, cx);
        cx.notify();
    }
}

impl Render for FollowupForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = service().map(|s| s.snapshot()).unwrap_or_default();
        if !self.loaded && snapshot.ready {
            self.enabled = snapshot.settings.execution.followup_suggestions;
            self.loaded = true;
        }
        let mut content = v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(
                Checkbox::new("ai-followup-enabled")
                    .label(t!("ai_followup_enabled").to_string())
                    .checked(self.enabled)
                    .disabled(!self.loaded || snapshot.busy)
                    .on_click(cx.listener(|this, checked, _, cx| {
                        this.enabled = *checked;
                        cx.notify();
                    })),
            )
            .child(note(t!("ai_followup_cost_hint").to_string(), cx))
            .child(
                Button::new("ai-save-followups")
                    .small()
                    .primary()
                    .label(t!("save").to_string())
                    .disabled(!self.loaded || snapshot.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            );
        if let Some(error) = &self.error {
            content = content.child(note(error_text(error), cx).text_color(cx.theme().danger));
        }
        if let Some(notice) = &snapshot.notice {
            content = content.child(note(error_text(notice), cx));
        }
        content
    }
}
