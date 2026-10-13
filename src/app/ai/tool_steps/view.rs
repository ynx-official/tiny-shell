//! Compact native trace; expanding a step never invokes the tool again.
use super::{
    StepKey, ToolGroup, ToolState, ToolStep,
    review::{ReviewActions, ReviewView},
};
use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    spinner::Spinner,
    text::TextView,
    v_flex,
};
use rust_i18n::t;
use std::collections::HashSet;

pub(crate) struct ToolGroupView {
    group: ToolGroup,
    expanded: HashSet<StepKey>,
    collapsed: bool,
    review: ReviewActions,
}
impl ToolGroupView {
    pub fn new(group: ToolGroup, review: ReviewActions) -> Self {
        Self {
            group,
            expanded: HashSet::new(),
            collapsed: false,
            review,
        }
    }
    pub fn update_group(&mut self, group: ToolGroup, cx: &mut Context<Self>) {
        if self.group == group {
            return;
        }
        if group.steps.iter().any(|step| {
            step.state == ToolState::AwaitingApproval
                && !self.group.steps.iter().any(|old| {
                    old.task_id == step.task_id && old.state == ToolState::AwaitingApproval
                })
        }) {
            self.collapsed = false;
        }
        self.group = group;
        cx.notify();
    }
    fn review_button(&self, step: &ToolStep, cx: &Context<Self>) -> Option<Button> {
        if step.state != ToolState::AwaitingApproval {
            return None;
        }
        step.task_id.as_ref()?;
        let id = format!("ai-step-review-{}-{}", step.key.message, step.key.call);
        Some(
            Button::new(id.clone())
                .debug_selector(move || id.clone())
                .outline()
                .xsmall()
                .label(t!("ai_review").to_string())
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.collapsed = false;
                    cx.notify();
                })),
        )
    }
}
impl Render for ToolGroupView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.group.state();
        let group_key = self.group.key;
        let mut trace = v_flex()
            .debug_selector(move || format!("ai-trace-{group_key}"))
            // Transcript overflow belongs to its scroller. Clipping this group otherwise
            // gives flex layout permission to reduce completed traces to their border.
            .flex_shrink_0()
            .w_full()
            .min_w_0()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(px(6.))
            .overflow_hidden()
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_1()
                    .pr_2()
                    .bg(cx.theme().muted.opacity(0.35))
                    .child(
                        Button::new("ai-steps-toggle")
                            .ghost()
                            .small()
                            .flex_1()
                            .min_w_0()
                            .h_auto()
                            .py_2()
                            .debug_selector(|| "ai-steps-toggle".into())
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .child(
                                        Icon::new(if self.collapsed {
                                            IconName::ChevronRight
                                        } else {
                                            IconName::ChevronDown
                                        })
                                        .size_3(),
                                    )
                                    .child(div().flex_1().min_w_0().text_left().child(
                                        t!("ai_steps", count = self.group.steps.len()).to_string(),
                                    ))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(state_color(status, cx))
                                            .child(t!(status.key()).to_string()),
                                    ),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.collapsed = !this.collapsed;
                                cx.notify();
                            })),
                    )
                    // Keep approval reachable even when a user collapses the trace.
                    .children(
                        self.group
                            .steps
                            .iter()
                            .filter(|_| self.collapsed)
                            .find_map(|step| self.review_button(step, cx)),
                    ),
            );
        if !self.collapsed {
            for (ordinal, step) in self.group.steps.iter().enumerate() {
                let key = step.key;
                let expanded = self.expanded.contains(&key);
                let preview = step.preview();
                let mut row = v_flex()
                    .w_full()
                    .min_w_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new(format!("ai-step-{}-{}", key.message, key.call))
                            .debug_selector(move || format!("ai-step-{ordinal}"))
                            .ghost()
                            .small()
                            .w_full()
                            .h_auto()
                            .px_2()
                            .py_2()
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_2()
                                    .items_start()
                                    .child(div().pt(px(2.)).child(state_icon(step.state, cx)))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .items_start()
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .text_left()
                                                            .overflow_hidden()
                                                            .text_ellipsis()
                                                            .child(step.label()),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_none()
                                                            .text_xs()
                                                            .text_color(state_color(step.state, cx))
                                                            .child(
                                                                t!(step.state.key()).to_string(),
                                                            ),
                                                    ),
                                            )
                                            .when_some(preview, |row, text| {
                                                row.child(
                                                    div()
                                                        .w_full()
                                                        .min_w_0()
                                                        .text_left()
                                                        .text_xs()
                                                        .font_family(
                                                            cx.theme().mono_font_family.clone(),
                                                        )
                                                        .text_color(cx.theme().muted_foreground)
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .child(text),
                                                )
                                            }),
                                    )
                                    .child(
                                        Icon::new(if expanded {
                                            IconName::ChevronUp
                                        } else {
                                            IconName::ChevronDown
                                        })
                                        .size_3()
                                        .text_color(cx.theme().muted_foreground),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.expanded.remove(&key) {
                                    this.expanded.insert(key);
                                }
                                cx.notify();
                            })),
                    );
                if step.state == ToolState::AwaitingApproval {
                    if let Some(task) = &step.task_id {
                        let actions = self.review.clone();
                        let task = task.clone();
                        let review = window.use_keyed_state(
                            format!("ai-inline-review-{task}"),
                            cx,
                            |_, _| ReviewView::new(task, actions),
                        );
                        row = row.child(review);
                    } else {
                        row = row.child(
                            div()
                                .px_3()
                                .pb_3()
                                .text_xs()
                                .whitespace_normal()
                                .text_color(cx.theme().warning)
                                .child(
                                    t!("ai_review_unavailable", code = "TASK_NOT_FOUND")
                                        .to_string(),
                                ),
                        );
                    }
                }
                if expanded {
                    let mut detail = v_flex().w_full().min_w_0().px_3().pb_3().gap_2().child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(step.name.clone()),
                    );
                    if let Some(arguments) = &step.arguments {
                        detail = detail.child(raw_block(
                            format!("ai-args-{}-{}", key.message, key.call),
                            t!("ai_step_arguments").to_string(),
                            pretty(&arguments.to_string()),
                            cx,
                        ));
                    }
                    if let Some(result) = &step.result {
                        detail = detail.child(raw_block(
                            format!("ai-output-{}-{}", key.message, key.call),
                            t!("ai_tool_result").to_string(),
                            pretty(result),
                            cx,
                        ));
                    } else {
                        detail = detail.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("ai_step_no_result").to_string()),
                        );
                    }
                    row = row.child(detail);
                }
                trace = trace.child(row);
            }
        }
        trace
    }
}
fn state_color(state: ToolState, cx: &App) -> gpui::Hsla {
    match state {
        ToolState::Succeeded => cx.theme().success,
        ToolState::Failed => cx.theme().danger,
        ToolState::AwaitingApproval | ToolState::TimedOut | ToolState::Unknown => {
            cx.theme().warning
        }
        ToolState::Running | ToolState::CancelRequested => cx.theme().primary,
        _ => cx.theme().muted_foreground,
    }
}
fn state_icon(state: ToolState, cx: &App) -> gpui::AnyElement {
    if matches!(state, ToolState::Running | ToolState::CancelRequested) {
        return Spinner::new()
            .xsmall()
            .color(state_color(state, cx))
            .into_any_element();
    }
    Icon::new(match state {
        ToolState::Succeeded => IconName::CircleCheck,
        ToolState::Failed => IconName::CircleX,
        ToolState::AwaitingApproval | ToolState::TimedOut | ToolState::Unknown => {
            IconName::TriangleAlert
        }
        ToolState::Cancelled | ToolState::Interrupted | ToolState::Skipped => IconName::Pause,
        _ => IconName::Ellipsis,
    })
    .size_4()
    .text_color(state_color(state, cx))
    .into_any_element()
}
fn pretty(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| text.to_owned())
}
// A fence longer than any output run prevents tool text from becoming Markdown, HTML or remote media.
fn literal(text: &str) -> String {
    let longest = text
        .split(|ch| ch != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}text\n{text}\n{fence}")
}
pub(super) fn raw_block(id: String, title: String, text: String, cx: &App) -> gpui::Div {
    let copy = text.clone();
    v_flex()
        .w_full()
        .min_w_0()
        .gap_1()
        .child(
            h_flex()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(title),
                )
                .child(
                    Button::new(format!("{id}-copy"))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Copy)
                        .tooltip(t!("ai_copy").to_string())
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                        }),
                ),
        )
        .child(
            div()
                .id(id.clone())
                .debug_selector({
                    let id = id.clone();
                    move || id.clone()
                })
                .max_h(px(220.))
                .w_full()
                .min_w_0()
                .overflow_y_scroll()
                .child(TextView::markdown(id, literal(&text)).selectable(true)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Entity, ScrollHandle, TestAppContext, VisualTestContext};
    use gpui_component::Root;
    use std::rc::Rc;

    fn unavailable_reviews() -> ReviewActions {
        ReviewActions {
            load: Rc::new(|_| Err("TASK_NOT_FOUND".into())),
            decide: Rc::new(|_, _, _, _| panic!("expanding a trace must not submit a decision")),
            authenticate: Rc::new(|_, _, _| panic!("review must stay inside the conversation")),
        }
    }

    #[test]
    fn tool_output_cannot_escape_into_media_or_html() {
        let text = "```\n<img src='https://example.invalid/private'>\n![remote](https://example.invalid/private)\n````";
        let tree = markdown::to_mdast(&literal(text), &markdown::ParseOptions::gfm()).unwrap();
        let children = tree.children().unwrap();
        assert_eq!(children.len(), 1);
        let markdown::mdast::Node::Code(code) = &children[0] else {
            panic!("output must remain one literal code block");
        };
        assert_eq!(code.value, text);
    }
    #[gpui::test]
    fn steps_expand_collapse_and_keep_the_approval_action_reachable(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let group = ToolGroup {
            key: 0,
            steps: vec![ToolStep {
                key: StepKey {
                    message: 0,
                    call: 0,
                },
                call_id: Some("fixture".into()),
                name: "run_command".into(),
                arguments: Some(serde_json::json!({"command":"systemctl is-active nginx"})),
                result: None,
                state: ToolState::AwaitingApproval,
                task_id: Some("fixture-task".into()),
                target: None,
            }],
        };
        let handle = cx.open_window(gpui::size(px(360.), px(600.)), |window, cx| {
            let view = cx.new(|_| ToolGroupView::new(group, unavailable_reviews()));
            Root::new(view, window, cx)
        });
        let root = handle.root(cx).unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(360.), px(600.)),
            |_, _| root.into_any_element(),
        );
        let step = visual.debug_bounds("ai-step-0").unwrap();
        assert!(visual.debug_bounds("ai-args-0-0").is_none());
        visual.simulate_click(step.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert!(visual.debug_bounds("ai-args-0-0").is_some());
        let toggle = visual.debug_bounds("ai-steps-toggle").unwrap();
        visual.simulate_click(toggle.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert!(visual.debug_bounds("ai-step-0").is_none());
        let review = visual.debug_bounds("ai-step-review-0-0").unwrap();
        assert!(
            review.right() <= px(360.),
            "review action must stay inside the narrow panel: {review:?}"
        );
        visual.simulate_click(review.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert!(visual.debug_bounds("ai-step-0").is_some());
        assert!(visual.debug_bounds("ai-review-unavailable").is_some());
    }

    struct ConversationHost {
        trace: Entity<ToolGroupView>,
        show_answer: bool,
        scroll: ScrollHandle,
    }
    impl Render for ConversationHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            v_flex().size_full().child(
                v_flex().id("conversation-scroller").flex_1().min_h_0().overflow_y_scroll()
                    .track_scroll(&self.scroll).gap_3().p_3()
                    .child(div().child("Checking nginx"))
                    .child(self.trace.clone())
                    .when(self.show_answer, |messages| messages.child(TextView::markdown("final-answer",
                        (0..24).map(|i| format!("Result {i}: nginx is running. Configuration and worker status are healthy.")).collect::<Vec<_>>().join("\n\n"))))
            )
        }
    }
    #[gpui::test]
    fn completed_steps_keep_their_height_when_the_final_answer_overflows(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let mut group = ToolGroup {
            key: 0,
            steps: vec![ToolStep {
                key: StepKey {
                    message: 0,
                    call: 0,
                },
                call_id: Some("fixture".into()),
                name: "run_command".into(),
                arguments: Some(serde_json::json!({"command":"systemctl is-active nginx"})),
                result: None,
                state: ToolState::Running,
                task_id: Some("fixture-task".into()),
                target: None,
            }],
        };
        let initial = group.clone();
        let handle = cx.open_window(gpui::size(px(420.), px(260.)), |window, cx| {
            let trace = cx.new(|_| ToolGroupView::new(initial, unavailable_reviews()));
            let host = cx.new(|_| ConversationHost {
                trace,
                show_answer: false,
                scroll: ScrollHandle::new(),
            });
            Root::new(host, window, cx)
        });
        let root = handle.root(cx).unwrap();
        let host = root.read_with(cx, |root, _| {
            root.view().clone().downcast::<ConversationHost>().unwrap()
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(420.), px(260.)),
            |_, _| root.into_any_element(),
        );
        let running = visual.debug_bounds("ai-trace-0").unwrap();
        assert!(
            running.size.height > px(60.),
            "running trace must start fully visible: {running:?}"
        );
        group.steps[0].state = ToolState::Succeeded;
        group.steps[0].result = Some(
            serde_json::json!({"state":"succeeded","result":{"stdout":"active\n","exit_code":0}})
                .to_string(),
        );
        host.update_in(&mut visual, |host, _, cx| {
            host.trace
                .update(cx, |trace, cx| trace.update_group(group, cx));
            host.show_answer = true;
            cx.notify();
        });
        visual.run_until_parked();
        let completed = visual.debug_bounds("ai-trace-0").unwrap();
        assert!(
            completed.size.height >= running.size.height,
            "a completed tool trace must not collapse to a border when the answer grows: running={running:?}, completed={completed:?}"
        );
        let step = visual.debug_bounds("ai-step-0").unwrap();
        assert!(step.bottom() <= completed.bottom());
        visual.simulate_click(step.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("ai-output-0-0").is_some(),
            "completed output must still expand"
        );
        host.read_with(&visual, |host, _| {
            assert!(
                host.scroll.max_offset().y > px(0.),
                "the conversation must scroll instead of compressing tool steps"
            )
        });
    }
}
