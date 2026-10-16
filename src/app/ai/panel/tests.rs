use super::*;
use crate::ai::{Message, Model, Provider, Settings, Snapshot};
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::Root;

struct SnapshotHost {
    panel: Entity<AiPanel>,
    snapshot: Snapshot,
}

impl Render for SnapshotHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        self.panel.update(cx, |panel, cx| {
            panel.render_snapshot(Rc::new(self.snapshot.clone()), window, cx)
        })
    }
}

#[test]
fn full_ai_panel_renders_on_a_windows_sized_stack() {
    // A component-only test misses the simultaneous stack use of the panel,
    // transcript and composer. Match the Windows executable's 1 MiB reserve.
    std::thread::Builder::new()
        .name("ai-panel-small-stack".into())
        .stack_size(1024 * 1024)
        .spawn(draw_full_panel)
        .unwrap()
        .join()
        .unwrap();
}

fn draw_full_panel() {
    let mut cx = TestAppContext::single();
    cx.update(gpui_component::init);
    let provider = Provider {
        name: "Fixture provider".into(),
        default_model: "fixture-model".into(),
        models: vec![Model {
            id: "fixture-model".into(),
            supports_tools: true,
        }],
        ..Default::default()
    };
    let settings = Settings {
        default_provider: Some(provider.id.clone()),
        providers: vec![provider],
        ..Default::default()
    };
    let conversation = Conversation::new(None, &settings);
    let size = gpui::size(px(360.), px(600.));
    let handle = cx.open_window(size, |window, cx| {
        let input = cx.new(|cx| InputState::new(window, cx).auto_grow(3, 7));
        let search = cx.new(|cx| InputState::new(window, cx));
        let rename = cx.new(|cx| InputState::new(window, cx));
        let panel = cx.new(|_| AiPanel {
            owner: WeakEntity::new_invalid(),
            current: Some(conversation.id.clone()),
            history: false,
            input,
            search,
            rename,
            rename_id: None,
            transcript: Transcript::default(),
            snapshot: None,
            followup_view: None,
            error: None,
            width: 360.,
            drag: None,
            _subscriptions: Vec::new(),
        });
        let host = cx.new(|_| SnapshotHost {
            panel,
            snapshot: Snapshot {
                settings,
                conversations: vec![conversation],
                ready: true,
                ..Default::default()
            },
        });
        Root::new(host, window, cx)
    });
    let root = handle.root(&mut cx).unwrap();
    let host = root.read_with(&cx, |root, _| {
        root.view().clone().downcast::<SnapshotHost>().unwrap()
    });
    let mut visual = VisualTestContext::from_window(handle.into(), &mut cx);
    for has_messages in [false, true] {
        if has_messages {
            host.update_in(&mut visual, |host, _, cx| {
                host.snapshot.conversations[0].messages = vec![
                    Message::text("user", "Fixture question"),
                    Message::text("assistant", "Fixture response"),
                ];
                cx.notify();
            });
        }
        visual.draw(gpui::point(px(0.), px(0.)), size, |_, _| {
            root.clone().into_any_element()
        });
        let composer = visual.debug_bounds("ai-composer").unwrap();
        let permissions = visual.debug_bounds("ai-mode-selector").unwrap();
        let usage = visual.debug_bounds("ai-conversation-usage").unwrap();
        let target = visual.debug_bounds("ai-target").unwrap();
        assert!(composer.size.height > px(100.));
        assert!(usage.origin.y >= composer.origin.y && usage.bottom() <= composer.bottom());
        assert!(target.right() <= usage.origin.x && usage.right() <= composer.right());
        assert!(permissions.origin.x >= composer.origin.x);
        assert!(permissions.right() <= composer.right());
        assert!(permissions.bottom() <= composer.bottom());
    }
    host.update_in(&mut visual, |host, _, cx| {
        host.snapshot.conversations[0].target = Some(crate::ai::BoundTarget {
            id: "fixture".into(),
            name: "A long server name that must leave its icon visible".into(),
            host: "fixture.invalid".into(),
            port: 22,
            user: "fixture".into(),
            digest: "fixture".into(),
        });
        cx.notify();
    });
    visual.run_until_parked();
    let icon = visual.debug_bounds("ai-target-icon").unwrap();
    let composer = visual.debug_bounds("ai-composer").unwrap();
    assert!(icon.size.width >= px(14.));
    assert!(icon.origin.x >= composer.origin.x && icon.right() <= composer.right());
    // Constructing a menu is another nested render path on the same UI stack.
    let permissions = visual.debug_bounds("ai-mode-selector").unwrap();
    visual.simulate_click(permissions.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert!(
        visual
            .debug_bounds("ai-mode-option-ai_mode_follow_global")
            .is_some()
    );
}

#[gpui::test]
fn tool_waiting_tail_stays_visible_above_usage_and_composer(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let mut conversation = Conversation::new(None, &Settings::default());
    conversation.state = TurnState::Generating;
    conversation.messages = vec![
        Message::text("user", "Fixture history"),
        Message::text("assistant", "A long earlier reply.\n\n".repeat(40)),
        Message::text("user", "Fixture operation"),
        Message {
            role: "assistant".into(),
            content: "Preparing the requested operation.".into(),
            tool_calls: vec![serde_json::json!({
                "id": "fixture-call",
                "function": {
                    "name": "run_command",
                    "arguments": "{\"command\":\"printf fixture\"}"
                }
            })],
            ..Default::default()
        },
    ];
    conversation.activities.push(crate::ai::Activity {
        name: "run_command".into(),
        task_id: Some("fixture-task".into()),
        state: "running".into(),
        message_index: Some(3),
        tool_call_id: Some("fixture-call".into()),
    });
    let size = gpui::size(px(360.), px(600.));
    let handle = cx.open_window(size, |window, cx| {
        let input = cx.new(|cx| InputState::new(window, cx).auto_grow(3, 7));
        let search = cx.new(|cx| InputState::new(window, cx));
        let rename = cx.new(|cx| InputState::new(window, cx));
        let panel = cx.new(|_| AiPanel {
            owner: WeakEntity::new_invalid(),
            current: Some(conversation.id.clone()),
            history: false,
            input,
            search,
            rename,
            rename_id: None,
            transcript: Transcript::default(),
            snapshot: None,
            followup_view: None,
            error: None,
            width: 360.,
            drag: None,
            _subscriptions: Vec::new(),
        });
        let host = cx.new(|_| SnapshotHost {
            panel,
            snapshot: Snapshot {
                conversations: vec![conversation],
                ready: true,
                ..Default::default()
            },
        });
        Root::new(host, window, cx)
    });
    let root = handle.root(cx).unwrap();
    let host = root.read_with(cx, |root, _| {
        root.view().clone().downcast::<SnapshotHost>().unwrap()
    });
    let panel = host.read_with(cx, |host, _| host.panel.clone());
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.draw(gpui::point(px(0.), px(0.)), size, |_, _| {
        root.into_any_element()
    });
    visual.run_until_parked();

    for (state, activity) in [
        (TurnState::RunningTool, "running"),
        (TurnState::AwaitingApproval, "awaiting_user"),
    ] {
        host.update_in(&mut visual, |host, _, cx| {
            let conversation = &mut host.snapshot.conversations[0];
            conversation.state = state;
            conversation.activities[0].state = activity.into();
            host.panel.update(cx, |panel, _| {
                panel.transcript.list.scroll_to(gpui::ListOffset::default());
            });
            cx.notify();
        });
        visual.run_until_parked();
        let transcript = visual.debug_bounds("ai-messages").unwrap();
        let usage = visual.debug_bounds("ai-conversation-usage").unwrap();
        let composer = visual.debug_bounds("ai-composer").unwrap();
        let tail = visual.debug_bounds("ai-typing-dots").unwrap();
        assert!(transcript.bottom() <= composer.origin.y + px(1.));
        assert!(usage.origin.y >= composer.origin.y && usage.bottom() <= composer.bottom());
        assert!(composer.bottom() <= size.height);
        assert!(tail.bottom() <= transcript.bottom() + px(1.));
        assert!(tail.origin.y >= transcript.origin.y);
        panel.read_with(&visual, |panel, _| {
            let remaining = panel.transcript.list.max_offset_for_scrollbar().y
                + panel.transcript.list.scroll_px_offset_for_scrollbar().y;
            assert!(panel.transcript.list.max_offset_for_scrollbar().y > px(0.));
            assert!(
                remaining.abs() < px(1.),
                "tool output must reach the bottom"
            );
        });
    }
}
