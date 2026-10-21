use super::*;
use crate::ai::{Message, Settings, Snapshot};

fn snapshot(id: &str, count: usize) -> Rc<Snapshot> {
    let mut conversation = Conversation::new(None, &Settings::default());
    conversation.id = id.into();
    conversation.messages = (0..count)
        .map(|index| Message::text("assistant", format!("Message {index}")))
        .collect();
    Rc::new(Snapshot {
        conversations: vec![conversation],
        ..Default::default()
    })
}

#[test]
fn unchanged_snapshot_reuses_projection_and_markdown() {
    let mut transcript = Transcript::default();
    let snapshot = snapshot("one", 2_000);
    transcript.prepare(snapshot.clone(), 0);
    let items = transcript.items.as_ptr();
    // Short SharedStrings are stored inline; only heap-backed text has a
    // stable shared allocation whose pointer can prove cache reuse.
    let markdown = "**A sufficiently long message to verify shared Markdown storage.**";
    let (_, first) = transcript.markdown.get(1, markdown);
    transcript.prepare(snapshot, 0);
    let (_, second) = transcript.markdown.get(1, markdown);
    assert_eq!(items, transcript.items.as_ptr());
    assert_eq!(first.as_ptr(), second.as_ptr());
    assert_eq!(transcript.list.item_count(), 2_001);
}

#[test]
fn streaming_and_new_messages_preserve_an_earlier_reading_anchor() {
    let mut transcript = Transcript::default();
    let initial = snapshot("one", 100);
    transcript.prepare(initial.clone(), 0);
    let anchor = gpui::ListOffset {
        item_ix: 12,
        offset_in_item: px(7.),
    };
    transcript.list.scroll_to(anchor);
    let mut next = (*initial).clone();
    next.conversations[0].messages[99].content.push_str(" more");
    next.conversations[0]
        .messages
        .push(Message::text("user", "next"));
    transcript.prepare(Rc::new(next), 0);
    let actual = transcript.list.logical_scroll_top();
    assert_eq!(actual.item_ix, anchor.item_ix);
    assert_eq!(actual.offset_in_item, anchor.offset_in_item);
    assert!(!transcript.list.is_following_tail());
    assert_eq!(transcript.list.item_count(), 102);
}

#[test]
fn switching_conversations_discards_message_cache_and_follows_new_tail() {
    let mut transcript = Transcript::default();
    transcript.prepare(snapshot("one", 100), 0);
    transcript.markdown.get(0, "first conversation");
    transcript.list.scroll_to(gpui::ListOffset::default());
    transcript.prepare(snapshot("two", 3), 0);
    assert!(transcript.markdown.entries.is_empty());
    assert!(transcript.list.is_following_tail());
    assert_eq!(transcript.list.item_count(), 4);
}

#[test]
fn active_tool_phases_reengage_tail_following() {
    for state in [
        TurnState::Generating,
        TurnState::RunningTool,
        TurnState::AwaitingApproval,
        TurnState::AwaitingContinuation,
    ] {
        let mut snapshot = (*snapshot("one", 100)).clone();
        snapshot.conversations[0].state = state;
        let snapshot = Rc::new(snapshot);
        let mut transcript = Transcript::default();
        transcript.prepare(snapshot.clone(), 0);
        transcript.list.scroll_to(gpui::ListOffset::default());
        transcript.prepare(snapshot, 0);
        assert!(transcript.list.is_following_tail());
    }
}

#[test]
fn same_length_edits_are_sanitized_again_and_cache_is_bounded() {
    let mut cache = MarkdownCache::default();
    let (_, before) = cache.get(0, "plain");
    let (source, after) = cache.get(0, "<img>");
    assert_eq!(source.as_ref(), "<img>");
    assert_ne!(before, after);
    assert!(after.contains("&lt;img>"));
    for index in 1..1_000 {
        cache.get(index, "![remote](https://example.invalid/image)");
    }
    assert!(cache.entries.len() <= MARKDOWN_CACHE_ITEMS);
    assert!(cache.bytes <= MARKDOWN_CACHE_BYTES);
}

#[test]
fn oversized_markdown_is_displayed_in_full_without_retaining_it_in_cache() {
    let mut cache = MarkdownCache::default();
    let content = "x".repeat(MARKDOWN_CACHE_BYTES);
    let (source, rendered) = cache.get(0, &content);
    assert_eq!(source.len(), content.len());
    assert_eq!(rendered.len(), content.len());
    assert!(cache.entries.is_empty());
    assert_eq!(cache.bytes, 0);
}

struct LongChatHost {
    panel: Entity<AiPanel>,
    snapshot: Rc<Snapshot>,
}

impl Render for LongChatHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        self.panel.update(cx, |panel, cx| {
            panel.render_snapshot(self.snapshot.clone(), window, cx)
        })
    }
}

#[gpui::test]
fn long_history_only_renders_visible_messages_and_can_revisit_earlier_content(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(gpui_component::init);
    let snapshot = snapshot("long-history", 2_000);
    let size = gpui::size(px(360.), px(600.));
    let handle = cx.open_window(size, |window, cx| {
        let input = cx.new(|cx| InputState::new(window, cx).auto_grow(3, 7));
        let search = cx.new(|cx| InputState::new(window, cx));
        let rename = cx.new(|cx| InputState::new(window, cx));
        let panel = cx.new(|_| AiPanel {
            owner: WeakEntity::new_invalid(),
            current: Some("long-history".into()),
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
        let host = cx.new(|_| LongChatHost { panel, snapshot });
        gpui_component::Root::new(host, window, cx)
    });
    let root = handle.root(cx).unwrap();
    let host = root.read_with(cx, |root, _| {
        root.view().clone().downcast::<LongChatHost>().unwrap()
    });
    let panel = host.read_with(cx, |host, _| host.panel.clone());
    let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
    visual.draw(gpui::point(px(0.), px(0.)), size, |_, _| {
        root.into_any_element()
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("ai-message-row-1999").is_some());
    assert!(visual.debug_bounds("ai-message-row-0").is_none());
    panel.read_with(&visual, |panel, _| {
        let cached = &panel.transcript.markdown.entries;
        assert!(!cached.is_empty());
        assert!(
            cached.len() < 40,
            "opening the tail must not parse the entire history"
        );
        assert!(cached.iter().all(|(index, _, _)| *index > 1_900));
    });
    host.update_in(&mut visual, |host, _, cx| {
        host.panel.update(cx, |panel, _| {
            panel.transcript.list.scroll_to(gpui::ListOffset::default())
        });
        cx.notify();
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("ai-message-row-0").is_some());
    assert!(visual.debug_bounds("ai-message-row-1999").is_none());
    host.update_in(&mut visual, |host, _, cx| {
        let snapshot = Rc::make_mut(&mut host.snapshot);
        snapshot.conversations[0].messages[1999]
            .content
            .push_str(" appended tail");
        cx.notify();
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("ai-message-row-0").is_some());
    panel.read_with(&visual, |panel, _| {
        assert!(!panel.transcript.list.is_following_tail())
    });
}
