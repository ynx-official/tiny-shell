use crate::ai::Conversation;
use gpui::{FollowMode, ListState};

pub(super) fn follow(scroll: &ListState, conversation: &Conversation) {
    // Tail mode anchors after layout, including delayed Markdown and tool review growth.
    // An idle upward scroll disengages it; active turns retain the existing follow policy.
    if conversation.state.active() && !scroll.is_following_tail() {
        scroll.set_follow_mode(FollowMode::Tail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Message, Settings, TurnState};
    use gpui::{
        AppContext as _, Context, InteractiveElement as _, IntoElement, ListAlignment, ListOffset,
        ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
        list, point, px, size,
    };
    use gpui_component::{Root, text::TextView, v_flex};

    struct ChatHost {
        conversation: Conversation,
        scroll: ListState,
        body_width: f32,
    }

    impl Render for ChatHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            follow(&self.scroll, &self.conversation);
            self.scroll.remeasure_items(0..1);
            let width = self.body_width;
            let content = self.conversation.messages[0].content.clone();
            list(self.scroll.clone(), move |_, _, _| {
                v_flex()
                    .w(px(width))
                    .child(TextView::markdown("streamed-markdown", content.clone()))
                    .child(
                        div()
                            .debug_selector(|| "stream-tail".to_owned())
                            .h(px(20.))
                            .child("..."),
                    )
                    .into_any_element()
            })
            .size_full()
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
                scroll: {
                    let list = ListState::new(1, ListAlignment::Top, px(200.));
                    list.set_follow_mode(FollowMode::Tail);
                    list
                },
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
            let remaining = host.scroll.max_offset_for_scrollbar().y
                + host.scroll.scroll_px_offset_for_scrollbar().y;
            assert!(host.scroll.max_offset_for_scrollbar().y > px(220.));
            assert!(
                remaining.abs() < px(1.),
                "streaming must reach the newly laid out bottom, remaining={remaining:?}"
            );
            let viewport = host.scroll.viewport_bounds();
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
                host.scroll.scroll_to(ListOffset::default());
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
        let before = host.read_with(&visual, |host, _| host.scroll.max_offset_for_scrollbar().y);
        host.update_in(&mut visual, |host, _, cx| {
            // Markdown/layout can grow after the SSE bytes were already recorded.
            host.body_width = 210.;
            cx.notify();
        });
        visual.run_until_parked();
        host.read_with(&visual, |host, _| {
            assert!(host.scroll.max_offset_for_scrollbar().y > before + px(100.));
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
                host.scroll.scroll_to(ListOffset::default());
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
            host.scroll.scroll_by(px(-4.));
            cx.notify();
        });
        visual.run_until_parked();
        host.read_with(&visual, |host, _| {
            let remaining = host.scroll.max_offset_for_scrollbar().y
                + host.scroll.scroll_px_offset_for_scrollbar().y;
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
                host.scroll.scroll_to(ListOffset::default());
                cx.notify();
            });
            visual.run_until_parked();
            host.read_with(&visual, |host, _| {
                assert_eq!(host.scroll.scroll_px_offset_for_scrollbar().y, px(0.));
            });
        }
    }
}
