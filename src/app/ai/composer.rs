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

pub(super) fn turn_in_progress(conversation: &Conversation) -> bool {
    conversation.state.active()
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
                .children((0..3).map(|index| {
                    div().child(".").with_animation(
                        format!("ai-processing-dot-{index}"),
                        Animation::new(Duration::from_millis(1_200)).repeat(),
                        move |dot, progress| {
                            let phase = (progress * 3. - index as f32).rem_euclid(3.);
                            dot.opacity(if phase < 1. { 1. } else { 0.22 })
                        },
                    )
                })),
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
    fn processing_stays_visible_through_streaming_and_tool_work_until_the_turn_ends() {
        let mut c = Conversation::new(None, &Settings::default());
        c.state = TurnState::Generating;
        c.messages.push(Message::text("user", "检查 nginx"));
        assert!(
            turn_in_progress(&c),
            "sending should immediately paint assistant feedback"
        );
        c.messages.push(Message::text("assistant", ""));
        assert!(turn_in_progress(&c));
        c.messages.last_mut().unwrap().content = "正在检查".into();
        assert!(
            turn_in_progress(&c),
            "streamed text must not finish this turn's processing feedback"
        );
        c.messages.last_mut().unwrap().tool_calls = vec![serde_json::json!({"id":"fixture"})];
        for state in [
            TurnState::RunningTool,
            TurnState::AwaitingApproval,
            TurnState::Generating,
        ] {
            c.state = state;
            assert!(
                turn_in_progress(&c),
                "processing must persist throughout every active phase"
            );
        }
        c.state = TurnState::Idle;
        assert!(!turn_in_progress(&c));
    }
    #[test]
    fn processing_disappears_on_completion_failure_or_stop() {
        let mut c = Conversation::new(None, &Settings::default());
        c.messages.push(Message::text("assistant", ""));
        for state in [TurnState::Idle, TurnState::Failed, TurnState::Interrupted] {
            c.state = state;
            assert!(!turn_in_progress(&c));
        }
    }

    struct ProcessingHost;
    impl Render for ProcessingHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .size_full()
                .p_3()
                .gap_3()
                .child(
                    div()
                        .debug_selector(|| "latest-reply".to_owned())
                        .child("Streaming reply content"),
                )
                .child(processing(cx))
        }
    }
    #[gpui::test]
    fn processing_uses_two_lines_after_the_latest_reply(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let handle = cx.open_window(gpui::size(px(360.), px(220.)), |window, cx| {
            let host = cx.new(|_| ProcessingHost);
            Root::new(host, window, cx)
        });
        let root = handle.root(cx).unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(360.), px(220.)),
            |_, _| root.into_any_element(),
        );
        let reply = visual.debug_bounds("latest-reply").unwrap();
        let role = visual.debug_bounds("ai-processing-role").unwrap();
        let text = visual.debug_bounds("ai-processing-text").unwrap();
        assert!(
            role.origin.y >= reply.bottom(),
            "processing must follow the latest chat content"
        );
        assert!(
            text.origin.y >= role.bottom(),
            "processing text must be below the assistant label"
        );
        assert!(text.size.height > px(0.));
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
