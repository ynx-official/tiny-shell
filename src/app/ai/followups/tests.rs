use super::*;
use gpui::{AppContext as _, TestAppContext, VisualTestContext};
use gpui_component::Root;
use std::cell::RefCell;

#[test]
fn adding_a_suggestion_preserves_the_existing_draft_exactly() {
    assert_eq!(append("", "查看文件"), "查看文件");
    assert_eq!(append("已有草稿 🙂", "查看文件"), "已有草稿 🙂\n查看文件");
    assert_eq!(append("已有草稿\n", "查看文件"), "已有草稿\n查看文件");
}

#[gpui::test]
fn suggestion_text_sends_while_the_only_icon_adds_and_narrow_layout_stays_usable(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_component::init);
    let selections = Rc::new(RefCell::new(Vec::new()));
    let selected = selections.clone();
    let size = gpui::size(px(312.), px(300.));
    let handle = cx.open_window(size, |window, cx| {
        let view = cx.new(|_| {
            FollowupView::new(
                vec![
                    "查看文件的完整内容，并保留现有编码、换行和文件权限".into(),
                    "Explain the exact replacement and its effect on the current configuration"
                        .into(),
                    "给出精确替换的命令".into(),
                ],
                move |index, action, _, _| {
                    selected
                        .borrow_mut()
                        .push((index, matches!(action, Action::Send)))
                },
            )
        });
        Root::new(view, window, cx)
    });
    let root = handle.root(cx).unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.draw(gpui::point(px(0.), px(0.)), size, |_, _| {
        root.into_any_element()
    });
    let list = visual.debug_bounds("ai-followups").unwrap();
    assert!(visual.debug_bounds("ai-followup-row-2").is_none());
    for (text_id, add_id, send_id) in [
        (
            "ai-followup-text-0",
            "ai-followup-add-0",
            "ai-followup-send-0",
        ),
        (
            "ai-followup-text-1",
            "ai-followup-add-1",
            "ai-followup-send-1",
        ),
    ] {
        let text = visual.debug_bounds(text_id).unwrap();
        let add = visual.debug_bounds(add_id).unwrap();
        assert!(text.right() <= add.origin.x);
        assert!(add.right() <= list.right());
        assert!(visual.debug_bounds(send_id).is_none());
    }
    let text = visual.debug_bounds("ai-followup-text-0").unwrap();
    visual.simulate_click(text.center(), gpui::Modifiers::none());
    let add = visual.debug_bounds("ai-followup-add-1").unwrap();
    visual.simulate_click(add.center(), gpui::Modifiers::none());
    let more = visual.debug_bounds("ai-followup-more").unwrap();
    visual.simulate_click(more.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert!(visual.debug_bounds("ai-followup-row-2").is_some());
    let text = visual.debug_bounds("ai-followup-text-2").unwrap();
    visual.simulate_click(text.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert_eq!(*selections.borrow(), [(0, true), (1, false), (2, true)]);
    let toggle = visual.debug_bounds("ai-followup-toggle").unwrap();
    visual.simulate_click(toggle.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert!(visual.debug_bounds("ai-followup-row-0").is_none());
    visual.simulate_click(toggle.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert!(visual.debug_bounds("ai-followup-row-2").is_some());
    let more = visual.debug_bounds("ai-followup-more").unwrap();
    visual.simulate_click(more.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert!(visual.debug_bounds("ai-followup-row-0").is_some());
    assert!(visual.debug_bounds("ai-followup-row-1").is_some());
    assert!(visual.debug_bounds("ai-followup-row-2").is_none());
}
