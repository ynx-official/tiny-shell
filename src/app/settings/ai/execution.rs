//! Run budgets own their draft; saving preserves the latest notification preferences.
use super::{error_text, field, note, service};
use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Window,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use rust_i18n::t;

pub(super) struct BudgetForm {
    calls: Entity<InputState>,
    requests: Entity<InputState>,
    loaded: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}
impl BudgetForm {
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
            loaded: false,
            error: None,
            _subscriptions,
        }
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let result = (|| {
            let service = service()?;
            let mut execution = service.snapshot().settings.execution;
            execution.tool_calls = self
                .calls
                .read(cx)
                .value()
                .trim()
                .parse()
                .map_err(|_| "AI_INVALID_BUDGET".to_owned())?;
            execution.model_requests = self
                .requests
                .read(cx)
                .value()
                .trim()
                .parse()
                .map_err(|_| "AI_INVALID_BUDGET".to_owned())?;
            service.save_execution(execution).map_err(|e| e.to_string())
        })();
        self.error = result.err();
        cx.notify();
    }
}
impl Render for BudgetForm {
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
            self.loaded = true;
        }
        let mut content = v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(field(
                "ai_tool_budget",
                Input::new(&self.calls)
                    .small()
                    .disabled(!self.loaded || snapshot.busy)
                    .into_any_element(),
                cx,
            ))
            .child(note(t!("ai_tool_budget_hint").to_string(), cx))
            .child(field(
                "ai_model_budget",
                Input::new(&self.requests)
                    .small()
                    .disabled(!self.loaded || snapshot.busy)
                    .into_any_element(),
                cx,
            ))
            .child(note(t!("ai_model_budget_hint").to_string(), cx))
            .child(
                h_flex().gap_2().child(
                    Button::new("ai-save-budget")
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
        content
    }
}
