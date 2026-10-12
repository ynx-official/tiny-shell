//! Run budgets are global AI preferences, independent of a provider draft.
use super::{error_text, field, note, service};
use crate::ai::ExecutionSettings;
use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Window, div,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use rust_i18n::t;

pub(super) struct ExecutionForm {
    calls: Entity<InputState>,
    requests: Entity<InputState>,
    notifications: bool,
    loaded: bool,
    advanced: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}
impl ExecutionForm {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let calls = cx.new(|cx| InputState::new(window, cx));
        let requests = cx.new(|cx| InputState::new(window, cx));
        let _subscriptions = [&calls, &requests]
            .into_iter()
            .map(|input| cx.subscribe(input, |_, _, _: &InputEvent, cx| cx.notify()))
            .collect();
        Self {
            calls,
            requests,
            notifications: false,
            loaded: false,
            advanced: false,
            error: None,
            _subscriptions,
        }
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let result = (|| {
            let execution = ExecutionSettings {
                tool_calls: self
                    .calls
                    .read(cx)
                    .value()
                    .trim()
                    .parse()
                    .map_err(|_| "AI_INVALID_BUDGET".to_owned())?,
                model_requests: self
                    .requests
                    .read(cx)
                    .value()
                    .trim()
                    .parse()
                    .map_err(|_| "AI_INVALID_BUDGET".to_owned())?,
                background_notifications: self.notifications,
            };
            service()?
                .save_execution(execution)
                .map_err(|e| e.to_string())
        })();
        self.error = result.err();
        cx.notify();
    }
}
impl Render for ExecutionForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = service().map(|s| s.snapshot()).unwrap_or_default();
        if !self.loaded && snapshot.ready {
            let execution = &snapshot.settings.execution;
            self.calls.update(cx, |i, cx| {
                i.set_value(execution.tool_calls.to_string(), window, cx)
            });
            self.requests.update(cx, |i, cx| {
                i.set_value(execution.model_requests.to_string(), window, cx)
            });
            self.notifications = execution.background_notifications;
            self.loaded = true;
        }
        let mut content = v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(div().child(t!("ai_execution_settings").to_string()))
            .child(field(
                "ai_tool_budget",
                Input::new(&self.calls)
                    .small()
                    .disabled(!self.loaded || snapshot.busy)
                    .into_any_element(),
                cx,
            ))
            .child(note(t!("ai_tool_budget_hint").to_string(), cx))
            .child(
                Button::new("ai-budget-advanced")
                    .ghost()
                    .small()
                    .label(t!("ai_budget_advanced").to_string())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.advanced = !this.advanced;
                        cx.notify();
                    })),
            );
        if self.advanced {
            content = content
                .child(field(
                    "ai_model_budget",
                    Input::new(&self.requests)
                        .small()
                        .disabled(!self.loaded || snapshot.busy)
                        .into_any_element(),
                    cx,
                ))
                .child(note(t!("ai_model_budget_hint").to_string(), cx));
        }
        content = content
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
                        Button::new("ai-save-execution")
                            .small()
                            .primary()
                            .label(t!("save").to_string())
                            .disabled(!self.loaded || snapshot.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
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
