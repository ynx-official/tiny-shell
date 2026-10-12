//! Chat feedback belongs to the transcript; input controls share one bordered surface.
use crate::ai::Conversation;
use gpui::{
    Animation, AnimationExt as _, App, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Styled as _, div, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::Button,
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use rust_i18n::t;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TurnFeedback {
    Hidden,
    Waiting,
    Dots,
}
pub(super) fn feedback_state(conversation: &Conversation) -> TurnFeedback {
    if !conversation.state.active() {
        return TurnFeedback::Hidden;
    }
    let start = conversation
        .messages
        .iter()
        .rposition(|message| message.role == "user")
        .unwrap_or(0);
    if conversation.messages[start..]
        .iter()
        .any(|message| message.role == "assistant" && !message.content.is_empty())
    {
        TurnFeedback::Dots
    } else {
        TurnFeedback::Waiting
    }
}

pub(super) fn render_feedback(state: TurnFeedback, cx: &App) -> Option<gpui::Div> {
    match state {
        TurnFeedback::Hidden => None,
        TurnFeedback::Waiting => Some(processing(cx)),
        TurnFeedback::Dots => Some(animated_dots(cx)),
    }
}

pub(super) fn animated_dots(cx: &App) -> gpui::Div {
    h_flex()
        .debug_selector(|| "ai-typing-dots".to_owned())
        .flex_shrink_0()
        .gap_0()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .children((0..3).map(|index| {
            div().child(".").with_animation(
                format!("ai-processing-dot-{index}"),
                Animation::new(Duration::from_millis(1_200)).repeat(),
                move |dot, progress| {
                    let phase = (progress * 3. - index as f32).rem_euclid(3.);
                    dot.opacity(if phase < 1. { 1. } else { 0.22 })
                },
            )
        }))
}

pub(super) fn processing(cx: &App) -> gpui::Div {
    v_flex()
        .debug_selector(|| "ai-processing".to_owned())
        .flex_shrink_0()
        .gap_1()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(
            div()
                .debug_selector(|| "ai-processing-role".to_owned())
                .text_xs()
                .child(t!("ai_assistant").to_string()),
        )
        .child(
            h_flex()
                .debug_selector(|| "ai-processing-text".to_owned())
                .gap_0()
                .child(div().child(t!("ai_processing").to_string()))
                .child(animated_dots(cx)),
        )
}

pub(super) fn surface(
    input: &Entity<InputState>,
    active: bool,
    context: impl IntoElement,
    model: impl IntoElement,
    send: Button,
    cx: &App,
) -> gpui::Div {
    v_flex()
        .debug_selector(|| "ai-composer".to_owned())
        .flex_shrink_0()
        .w_full()
        .min_w_0()
        .gap_1()
        .border_1()
        .border_color(cx.theme().input)
        .rounded(px(12.))
        .bg(cx.theme().background)
        .child(div().px_2().pt_2().child(context))
        .child(
            Input::new(input)
                .small()
                .appearance(false)
                .disabled(active)
                .px_3()
                .py_2(),
        )
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .px_2()
                .pb_2()
                .child(div().flex_1().min_w_0().child(model))
                .child(send.flex_none()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Message, Settings, TurnState};
    use gpui::{AppContext as _, Context, Render, TestAppContext, VisualTestContext, Window};
    use gpui_component::{IconName, Root, button::ButtonVariants as _};
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn processing_text_is_replaced_by_dots_when_the_first_body_byte_arrives() {
        let mut c = Conversation::new(None, &Settings::default());
        c.state = TurnState::Generating;
        c.messages.push(Message::text("user", "检查 nginx"));
        assert!(
            feedback_state(&c) == TurnFeedback::Waiting,
            "sending should immediately paint assistant feedback"
        );
        c.messages.push(Message::text("assistant", ""));
        assert_eq!(feedback_state(&c), TurnFeedback::Waiting);
        c.messages.last_mut().unwrap().content = "x".into();
        assert_eq!(
            feedback_state(&c),
            TurnFeedback::Dots,
            "body text must replace the waiting caption, leaving only dots"
        );
        c.messages.last_mut().unwrap().tool_calls = vec![serde_json::json!({"id":"fixture"})];
        for state in [
            TurnState::RunningTool,
            TurnState::AwaitingApproval,
            TurnState::Generating,
        ] {
            c.state = state;
            assert_eq!(
                feedback_state(&c),
                TurnFeedback::Dots,
                "dots must persist throughout every active phase"
            );
        }
        c.state = TurnState::Idle;
        assert_eq!(feedback_state(&c), TurnFeedback::Hidden);
    }
    #[test]
    fn processing_disappears_on_completion_failure_or_stop() {
        let mut c = Conversation::new(None, &Settings::default());
        c.messages.push(Message::text("assistant", ""));
        for state in [TurnState::Idle, TurnState::Failed, TurnState::Interrupted] {
            c.state = state;
            assert_eq!(feedback_state(&c), TurnFeedback::Hidden);
        }
    }

    #[test]
    fn feedback_tracks_the_current_user_turn_and_never_restarts_waiting_after_tools() {
        let mut c = Conversation::new(None, &Settings::default());
        c.state = TurnState::Generating;
        c.messages = vec![
            Message::text("user", "previous request"),
            Message::text("assistant", "previous reply"),
            Message::text("user", "new request"),
            Message::text("tool", "tool output is not reply body"),
        ];
        assert_eq!(
            feedback_state(&c),
            TurnFeedback::Waiting,
            "old replies and tool output cannot count as this turn's first body byte"
        );
        c.messages.push(Message::text("assistant", "first body"));
        c.messages.push(Message::text("tool", "completed result"));
        c.messages.push(Message::text("assistant", ""));
        assert_eq!(
            feedback_state(&c),
            TurnFeedback::Dots,
            "a later model round must not bring back the processing caption"
        );
    }

    struct WaitingHost;
    impl Render for WaitingHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .size_full()
                .p_3()
                .gap_3()
                .child(
                    div()
                        .debug_selector(|| "current-request".to_owned())
                        .child("User request"),
                )
                .child(processing(cx))
        }
    }
    #[gpui::test]
    fn waiting_uses_two_lines_before_any_body_arrives(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let handle = cx.open_window(gpui::size(px(360.), px(220.)), |window, cx| {
            let host = cx.new(|_| WaitingHost);
            Root::new(host, window, cx)
        });
        let root = handle.root(cx).unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(360.), px(220.)),
            |_, _| root.into_any_element(),
        );
        let reply = visual.debug_bounds("current-request").unwrap();
        let role = visual.debug_bounds("ai-processing-role").unwrap();
        let text = visual.debug_bounds("ai-processing-text").unwrap();
        assert!(
            role.origin.y >= reply.bottom(),
            "waiting feedback must follow the current user request"
        );
        assert!(
            text.origin.y >= role.bottom(),
            "processing text must be below the assistant label"
        );
        assert!(text.size.height > px(0.));
    }

    struct ReplyHost;
    impl Render for ReplyHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .size_full()
                .p_3()
                .gap_1()
                .child(
                    div()
                        .debug_selector(|| "existing-assistant-role".to_owned())
                        .child(t!("ai_assistant").to_string()),
                )
                .child(
                    div()
                        .debug_selector(|| "streamed-body".to_owned())
                        .child("Streaming reply"),
                )
                .children(render_feedback(TurnFeedback::Dots, cx))
        }
    }
    #[gpui::test]
    fn body_feedback_contains_only_dots_without_another_assistant_caption(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let handle = cx.open_window(gpui::size(px(360.), px(220.)), |window, cx| {
            let host = cx.new(|_| ReplyHost);
            Root::new(host, window, cx)
        });
        let root = handle.root(cx).unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(360.), px(220.)),
            |_, _| root.into_any_element(),
        );
        assert!(visual.debug_bounds("existing-assistant-role").is_some());
        assert!(
            visual.debug_bounds("ai-processing-role").is_none(),
            "body output must not add another assistant caption"
        );
        assert!(
            visual.debug_bounds("ai-processing-text").is_none(),
            "body output must not retain the waiting text"
        );
        let body = visual.debug_bounds("streamed-body").unwrap();
        let dots = visual.debug_bounds("ai-typing-dots").unwrap();
        assert!(
            dots.origin.y >= body.bottom(),
            "dots must follow the latest output"
        );
    }

    struct ComposerHost {
        input: Entity<InputState>,
        stopped: Rc<Cell<bool>>,
    }
    impl Render for ComposerHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let stopped = self.stopped.clone();
            v_flex().size_full().p_3().child(surface(&self.input,true,
                div().child("Example SSH"),
                Button::new("fixture-model").ghost().xsmall().w_full().min_w_0()
                    .child(div().min_w_0().w_full().overflow_hidden().text_ellipsis().child("Provider · a-very-long-model-name-that-must-not-cover-the-stop-button")),
                Button::new("fixture-stop").debug_selector(||"composer-stop".to_owned())
                    .small().ghost().icon(IconName::Pause).on_click(move |_,_,_|stopped.set(true)),cx))
        }
    }
    #[gpui::test]
    fn narrow_composer_keeps_stop_inside_the_surface_and_usable(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let stopped = Rc::new(Cell::new(false));
        let value = stopped.clone();
        let handle = cx.open_window(gpui::size(px(360.), px(220.)), |window, cx| {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Describe your question")
                    .auto_grow(3, 7)
            });
            let host = cx.new(|_| ComposerHost {
                input,
                stopped: value,
            });
            Root::new(host, window, cx)
        });
        let root = handle.root(cx).unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(360.), px(220.)),
            |_, _| root.into_any_element(),
        );
        let card = visual.debug_bounds("ai-composer").unwrap();
        let stop = visual.debug_bounds("composer-stop").unwrap();
        assert!(card.size.height > px(100.));
        assert!(
            stop.right() <= card.right() && stop.bottom() <= card.bottom(),
            "stop control must be inside the input card: {card:?} {stop:?}"
        );
        visual.simulate_click(stop.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert!(
            stopped.get(),
            "the disabled text field must not disable the stop control"
        );
    }
}
