use super::{Call, Review, ReviewActions, ReviewScope, ReviewView, same_operation};
use gpui::{AppContext as _, IntoElement as _, TestAppContext, VisualTestContext, px};
use gpui_component::Root;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
use tiny_shell_mcp::{
    contracts::{Failure, WriteFile, WriteMode},
    engine::Target,
    tasks::State,
};

fn fixture() -> Review {
    Review {
        id: "pending-write".into(),
        client: "ai:conversation-a".into(),
        target: Target {
            id: "target-a".into(),
            name: "Example SSH".into(),
            group: None,
            host: "example.invalid".into(),
            port: 22,
            user: "fixture".into(),
            digest: "digest-a".into(),
        },
        call: Call::WriteFile(WriteFile {
            session_id: "session-a".into(),
            path: "/tmp/example.txt".into(),
            content: "before\nafter\n".into(),
            mode: WriteMode::Update,
            expected_version: Some("version-a".into()),
            request_id: "request-a".into(),
        }),
        state: State::AwaitingUser,
        error: None,
        preview: json!({"old_content":"before\n"}),
    }
}

#[test]
fn review_scope_rejects_other_conversations_targets_tasks_and_finished_work() {
    let review = fixture();
    let scope = ReviewScope::new("conversation-a", "target-a", "digest-a");
    assert!(scope.matches("pending-write", &review));
    assert!(!scope.matches("other-task", &review));
    let mut other = review.clone();
    other.client = "ai:conversation-b".into();
    assert!(!scope.matches("pending-write", &other));
    other.client = "external-client".into();
    assert!(!scope.matches("pending-write", &other));
    other = review.clone();
    other.target.id = "target-b".into();
    assert!(!scope.matches("pending-write", &other));
    other = review.clone();
    other.target.digest = "changed".into();
    assert!(!scope.matches("pending-write", &other));
    other = review.clone();
    other.state = State::Cancelled;
    assert!(!scope.matches("pending-write", &other));
}

#[test]
fn changed_review_cannot_execute_a_previously_displayed_snapshot() {
    let review = fixture();
    assert!(same_operation(&review, &review));
    let mut changed = review.clone();
    changed.preview["old_content"] = json!("changed remotely");
    assert!(!same_operation(&review, &changed));
    changed = review.clone();
    if let Call::WriteFile(file) = &mut changed.call {
        file.content = "different write".into();
    }
    assert!(!same_operation(&review, &changed));
    changed = review.clone();
    changed.error = Some(Failure::new("AUTH_REQUIRED"));
    assert!(!same_operation(&review, &changed));
}

#[gpui::test]
fn inline_write_review_shows_both_contents_and_submits_only_once(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let decisions = Rc::new(RefCell::new(Vec::new()));
    let captured = decisions.clone();
    let actions = ReviewActions {
        load: Rc::new(|_| Ok(fixture())),
        decide: Rc::new(move |review, approved, _, _| {
            captured.borrow_mut().push((review.id.clone(), approved));
            Ok(())
        }),
        authenticate: Rc::new(|_, _, _| panic!("file approval must not open settings")),
    };
    let handle = cx.open_window(gpui::size(px(360.), px(900.)), |window, cx| {
        let view = cx.new(|_| ReviewView::new("pending-write".into(), actions));
        Root::new(view, window, cx)
    });
    let root = handle.root(cx).unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(360.), px(900.)),
        |_, _| root.into_any_element(),
    );
    for id in [
        "ai-review-old-pending-write",
        "ai-review-new-pending-write",
        "ai-review-approve",
        "ai-review-deny",
    ] {
        let bounds = visual.debug_bounds(id).unwrap();
        assert!(bounds.size.height > px(0.) && bounds.right() <= px(360.));
    }
    let approve = visual.debug_bounds("ai-review-approve").unwrap();
    visual.simulate_click(approve.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    visual.simulate_click(approve.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert_eq!(*decisions.borrow(), vec![("pending-write".into(), true)]);
    assert!(visual.debug_bounds("ai-review-approve").is_none());
}

#[gpui::test]
fn credential_requests_missing_previews_and_load_errors_cannot_be_approved(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_component::init);
    let current = Rc::new(RefCell::new(Ok(fixture())));
    let source = current.clone();
    let actions = ReviewActions {
        load: Rc::new(move |_| source.borrow().clone()),
        decide: Rc::new(|_, _, _, _| panic!("no decision was requested")),
        authenticate: Rc::new(|_, _, _| {}),
    };
    current.borrow_mut().as_mut().unwrap().error = Some(Failure::new("AUTH_REQUIRED"));
    let handle = cx.open_window(gpui::size(px(360.), px(900.)), |window, cx| {
        let view = cx.new(|_| ReviewView::new("pending-write".into(), actions));
        Root::new(view, window, cx)
    });
    let root = handle.root(cx).unwrap();
    let view = root.read_with(cx, |root, _| {
        root.view().clone().downcast::<ReviewView>().unwrap()
    });
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(360.), px(900.)),
        |_, _| root.clone().into_any_element(),
    );
    assert!(visual.debug_bounds("ai-review-authenticate").is_some());
    assert!(visual.debug_bounds("ai-review-approve").is_none());
    *current.borrow_mut() = Ok(fixture());
    current.borrow_mut().as_mut().unwrap().preview = json!({});
    view.update_in(&mut visual, |_, _, cx| cx.notify());
    visual.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(360.), px(900.)),
        |_, _| root.clone().into_any_element(),
    );
    assert!(visual.debug_bounds("ai-review-approve").is_none());
    assert!(visual.debug_bounds("ai-review-deny").is_some());
    *current.borrow_mut() = Err("TASK_NOT_PENDING".into());
    view.update_in(&mut visual, |_, _, cx| cx.notify());
    visual.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(360.), px(900.)),
        |_, _| root.into_any_element(),
    );
    assert!(visual.debug_bounds("ai-review-unavailable").is_some());
    assert!(visual.debug_bounds("ai-review-approve").is_none());
    assert!(visual.debug_bounds("ai-review-deny").is_none());
}

#[gpui::test]
fn command_review_keeps_full_command_and_can_retry_a_failed_rejection(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let decisions = Rc::new(RefCell::new(Vec::new()));
    let captured = decisions.clone();
    let mut review = fixture();
    review.call = Call::RunCommand(tiny_shell_mcp::contracts::RunCommand {
        session_id: "session-a".into(),
        command: "printf 'first\\n'\nprintf 'second\\n'".into(),
        request_id: "request-a".into(),
        cwd: Some("/tmp".into()),
        timeout_ms: 30000,
    });
    let displayed = review.clone();
    let actions = ReviewActions {
        load: Rc::new(move |_| Ok(review.clone())),
        decide: Rc::new(move |actual, approved, _, _| {
            assert!(same_operation(&displayed, actual));
            captured.borrow_mut().push((actual.id.clone(), approved));
            if captured.borrow().len() == 1 {
                Err("APPROVAL_CHANGED".into())
            } else {
                Ok(())
            }
        }),
        authenticate: Rc::new(|_, _, _| panic!("a command decision stays inline")),
    };
    let handle = cx.open_window(gpui::size(px(360.), px(900.)), |window, cx| {
        let view = cx.new(|_| ReviewView::new("pending-write".into(), actions));
        Root::new(view, window, cx)
    });
    let root = handle.root(cx).unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(360.), px(900.)),
        |_, _| root.into_any_element(),
    );
    assert!(
        visual
            .debug_bounds("ai-review-command-pending-write")
            .is_some()
    );
    assert!(visual.debug_bounds("ai-review-cwd").is_some());
    let deny = visual.debug_bounds("ai-review-deny").unwrap();
    visual.simulate_click(deny.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert!(visual.debug_bounds("ai-review-error").is_some());
    let deny = visual.debug_bounds("ai-review-deny").unwrap();
    visual.simulate_click(deny.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        *decisions.borrow(),
        vec![
            ("pending-write".into(), false),
            ("pending-write".into(), false)
        ]
    );
    assert!(visual.debug_bounds("ai-review-deny").is_none());
}
