use crate::ai::Conversation;
use gpui::{ScrollHandle, px};

pub(super) type ScrollStamp = (String, usize, usize);

pub(super) fn follow(
    scroll: &ScrollHandle,
    previous: &mut Option<ScrollStamp>,
    conversation: &Conversation,
) {
    let stamp = (
        conversation.id.clone(),
        conversation.messages.len(),
        conversation.messages.last().map_or(0, |m| m.content.len()),
    );
    let changed = previous.as_ref() != Some(&stamp);
    let new_conversation = previous.as_ref().is_none_or(|old| old.0 != conversation.id);
    let remaining = scroll.max_offset().y + scroll.offset().y;
    // Tool reviews can grow without transcript changes. Follow every active phase;
    // keep an already-bottom viewport anchored when the final Markdown settles.
    // The tight idle tolerance lets even a small manual scroll leave the bottom.
    if conversation.state.active()
        || new_conversation
        || remaining <= px(0.5)
        || (changed && remaining <= px(32.))
    {
        scroll.scroll_to_bottom();
    }
    *previous = Some(stamp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Message, Settings, TurnState};
    use gpui::{
        AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
        StatefulInteractiveElement as _, Styled as _, TestAppContext, VisualTestContext, Window,
        div, point, size,
    };
    use gpui_component::{Root, text::TextView, v_flex};

    struct ChatHost {
        conversation: Conversation,
        scroll: ScrollHandle,
        stamp: Option<ScrollStamp>,
        body_width: f32,
    }

    impl Render for ChatHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            follow(&self.scroll, &mut self.stamp, &self.conversation);
            v_flex().size_full().child(
                v_flex()
                    .id("streaming-chat")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        v_flex()
                            .flex_shrink_0()
                            .w(px(self.body_width))
                            .child(TextView::markdown(
                                "streamed-markdown",
                                self.conversation.messages[0].content.clone(),
                            ))
                            .child(
                                div()
                                    .debug_selector(|| "stream-tail".to_owned())
                                    .h(px(20.))
                                    .child("..."),
                            ),
                    ),
            )
        }
    }

    fn open_chat(cx: &mut TestAppContext) -> (gpui::Entity<ChatHost>, VisualTestContext) {
        cx.update(gpui_component::init);
        let handle = cx.open_window(size(px(360.), px(220.)), |window, cx| {
            let mut conversation = Conversation::new(None, &Settings::default());
            conversation.state = TurnState::Generating;
            conversation.messages.push(Message::text(
                "assistant",
                "## Service status\n\nThe service is active, with the configuration validated and the worker processes healthy.\n\n".repeat(16),
            ));
            let host = cx.new(|_| ChatHost {
                conversation,
                scroll: ScrollHandle::new(),
                stamp: None,
                body_width: 360.,
            });
            Root::new(host, window, cx)
        });
        let root = handle.root(cx).unwrap();
        let host = root.read_with(cx, |root, _| {
            root.view().clone().downcast::<ChatHost>().unwrap()
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.draw(point(px(0.), px(0.)), size(px(360.), px(220.)), |_, _| {
            root.into_any_element()
        });
        visual.run_until_parked();
        (host, visual)
    }

    fn assert_at_bottom(host: &gpui::Entity<ChatHost>, visual: &mut VisualTestContext) {
        let tail = visual.debug_bounds("stream-tail").unwrap();
        host.read_with(visual, |host, _| {
            let remaining = host.scroll.max_offset().y + host.scroll.offset().y;
            assert!(host.scroll.max_offset().y > px(220.));
            assert!(
                remaining.abs() < px(1.),
                "streaming must reach the newly laid out bottom, remaining={remaining:?}"
            );
            let viewport = host.scroll.bounds();
            assert!(
                tail.bottom() <= viewport.bottom() + px(1.) && tail.origin.y >= viewport.origin.y,
                "the latest output must be visible: tail={tail:?}, viewport={viewport:?}"
            );
        });
    }

    #[gpui::test]
    fn successive_sse_chunks_follow_bottom_even_from_scrolled_up_history(cx: &mut TestAppContext) {
        let (host, mut visual) = open_chat(cx);
        for _ in 0..3 {
            host.update_in(&mut visual, |host, _, cx| {
                host.scroll.set_offset(point(px(0.), px(0.)));
                host.conversation.messages[0]
                    .content
                    .push_str(&"More streamed service details.\n\n".repeat(12));
                cx.notify();
            });
            visual.run_until_parked();
            assert_at_bottom(&host, &mut visual);
        }
    }

    #[gpui::test]
    fn streaming_reflow_follows_bottom_without_a_new_content_stamp(cx: &mut TestAppContext) {
        let (host, mut visual) = open_chat(cx);
        let before = host.read_with(&visual, |host, _| host.scroll.max_offset().y);
        host.update_in(&mut visual, |host, _, cx| {
            // Markdown/layout can grow after the SSE bytes were already recorded.
            host.body_width = 210.;
            cx.notify();
        });
        visual.run_until_parked();
        host.read_with(&visual, |host, _| {
            assert!(host.scroll.max_offset().y > before + px(100.));
        });
        assert_at_bottom(&host, &mut visual);
    }

    #[gpui::test]
    fn tool_execution_and_user_waits_follow_without_new_transcript_bytes(cx: &mut TestAppContext) {
        let (host, mut visual) = open_chat(cx);
        for state in [
            TurnState::RunningTool,
            TurnState::AwaitingApproval,
            TurnState::AwaitingContinuation,
        ] {
            host.update_in(&mut visual, |host, _, cx| {
                host.conversation.state = state;
                host.scroll.set_offset(point(px(0.), px(0.)));
                host.body_width -= 25.;
                cx.notify();
            });
            visual.run_until_parked();
            assert_at_bottom(&host, &mut visual);
        }
    }

    #[gpui::test]
    fn final_markdown_reflow_keeps_the_bottom_after_the_turn_finishes(cx: &mut TestAppContext) {
        let (host, mut visual) = open_chat(cx);
        assert_at_bottom(&host, &mut visual);
        host.update_in(&mut visual, |host, _, cx| {
            host.conversation.state = TurnState::Idle;
            host.body_width = 210.;
            cx.notify();
        });
        visual.run_until_parked();
        assert_at_bottom(&host, &mut visual);

        host.update_in(&mut visual, |host, _, cx| {
            let offset = host.scroll.offset();
            host.scroll.set_offset(point(offset.x, offset.y + px(4.)));
            cx.notify();
        });
        visual.run_until_parked();
        host.read_with(&visual, |host, _| {
            let remaining = host.scroll.max_offset().y + host.scroll.offset().y;
            assert!(
                remaining >= px(3.),
                "a small manual scroll must remain usable"
            );
        });
    }

    #[gpui::test]
    fn completed_stopped_and_failed_turns_allow_reading_earlier_messages(cx: &mut TestAppContext) {
        let (host, mut visual) = open_chat(cx);
        for state in [TurnState::Idle, TurnState::Interrupted, TurnState::Failed] {
            host.update_in(&mut visual, |host, _, cx| {
                host.conversation.state = state;
                host.scroll.set_offset(point(px(0.), px(0.)));
                cx.notify();
            });
            visual.run_until_parked();
            host.read_with(&visual, |host, _| {
                assert_eq!(host.scroll.offset().y, px(0.));
            });
        }
    }
}
