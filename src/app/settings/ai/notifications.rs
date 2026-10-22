//! Notification preferences own their draft; saving preserves the latest run budgets.
use super::{error_text, note, service};
use gpui::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex, v_flex,
};
use rust_i18n::t;

pub(super) struct NotificationForm {
    notifications: bool,
    loaded: bool,
    error: Option<String>,
}
impl NotificationForm {
    pub(super) fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            notifications: false,
            loaded: false,
            error: None,
        }
    }
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = (|| {
            let service = service()?;
            let mut execution = service.snapshot().settings.execution;
            execution.background_notifications = self.notifications;
            service.save_execution(execution).map_err(|e| e.to_string())
        })();
        self.error = super::feedback::report(result, "feedback_saved", window, cx);
        cx.notify();
    }
}
impl Render for NotificationForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = service().map(|s| s.snapshot()).unwrap_or_default();
        if !self.loaded && snapshot.ready {
            self.notifications = snapshot.settings.execution.background_notifications;
            self.loaded = true;
        }
        let mut content = v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(
                Checkbox::new("ai-background-notifications")
                    .label(t!("ai_background_notifications").to_string())
                    .checked(self.notifications)
                    .disabled(
                        !self.loaded
                            || snapshot.busy
                            || !crate::app::ai::notifications::supported(),
                    )
                    .on_click(cx.listener(|this, enabled, _, cx| {
                        this.notifications = *enabled;
                        if *enabled {
                            crate::app::ai::notifications::authorize();
                        }
                        cx.notify();
                    })),
            )
            .child(note(t!("ai_notification_privacy").to_string(), cx))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("ai-test-notification")
                            .small()
                            .outline()
                            .label(t!("ai_test_notification").to_string())
                            .disabled(!crate::app::ai::notifications::supported())
                            .on_click(|_, _, _| crate::app::ai::notifications::test()),
                    )
                    .child(
                        Button::new("ai-save-notifications")
                            .small()
                            .primary()
                            .label(t!("save").to_string())
                            .disabled(!self.loaded || snapshot.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            );
        if let Some(error) = &self.error {
            content = content.child(note(error_text(error), cx).text_color(cx.theme().danger));
        }
        if let Some(notice) = &snapshot.notice {
            content = content.child(note(error_text(notice), cx));
        }
        if let Some(notice) = crate::app::ai::notifications::notice() {
            content = content.child(note(t!(notice).to_string(), cx));
        }
        content
    }
}
