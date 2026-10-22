use super::components::{hint, metadata, status_badge, status_color};
use super::presentation::{code, kind, record_text, source, target, time};
use super::*;
use gpui::{
    InteractiveElement as _, IntoElement as _, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    text::TextView,
    v_flex,
};

impl AuditPanel {
    pub(super) fn detail_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let pending = crate::app::mcp::service()
            .ok()
            .and_then(|s| s.engine.reviews().ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.state == tiny_shell_mcp::tasks::State::AwaitingUser)
            .map(|r| r.id)
            .collect::<std::collections::HashSet<_>>();
        let selected = self.detail.events.first().or_else(|| {
            self.page
                .events
                .iter()
                .find(|e| self.selection.as_ref() == Some(&e.context.operation_id))
        });
        let mut list = v_flex()
            .id("audit-detail-list")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .p_4()
            .gap_4();
        if let Some(event) = selected {
            let text = record_text(event);
            let mut summary = v_flex()
                .gap_3()
                .pb_4()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_start()
                        .flex_wrap()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(text.heading),
                        )
                        .child(status_badge(&event.state, cx)),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(metadata(
                            "audit_detail_source",
                            event
                                .context
                                .client_name
                                .clone()
                                .unwrap_or_else(|| source(event.context.source)),
                            cx,
                        ))
                        .child(metadata("audit_detail_target", target(event), cx))
                        .child(metadata("audit_column_time", time(event.timestamp_ms), cx))
                        .child(metadata(
                            "audit_detail_type",
                            super::presentation::category(event.category),
                            cx,
                        )),
                );
            if let Some(technical) = text.technical {
                let copy = technical.clone();
                summary = summary.child(
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(
                                            t!(if event.tool == "run_command" {
                                                "audit_detail_command"
                                            } else {
                                                "audit_detail_reference"
                                            })
                                            .to_string(),
                                        ),
                                )
                                .child(
                                    Button::new("audit-copy-reference")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Copy)
                                        .tooltip(t!("audit_copy").to_string())
                                        .on_click(move |_, window, cx| {
                                            crate::feedback::Feedback::copy(
                                                window,
                                                cx,
                                                copy.clone(),
                                            );
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .id("audit-reference-scroll")
                                .max_h(px(120.))
                                .overflow_y_scroll()
                                .child(
                                    TextView::markdown("audit-reference", code(&technical))
                                        .selectable(true),
                                ),
                        ),
                );
            }
            list = list.child(summary);
        }
        list = list.child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(t!("audit_events_timeline").to_string()),
        );
        if self.detail.events.is_empty() {
            list = list.child(hint(
                t!(if self.loading {
                    "audit_loading"
                } else {
                    "audit_empty"
                })
                .to_string(),
                cx.theme().muted_foreground,
            ));
        }
        for event in self.detail.events.iter().rev() {
            let text = self.formatted.get(&event.id).cloned().unwrap_or_default();
            let copy = text.clone();
            let op = event.context.operation_id.clone();
            let toggle = event.id.clone();
            let expanded = self.expanded.contains(&event.id);
            let mut row = v_flex()
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .gap_2()
                .pb_4()
                .pl_3()
                .border_l_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_start()
                        .child(
                            div()
                                .mt_1()
                                .size(px(6.))
                                .flex_shrink_0()
                                .rounded(px(3.))
                                .bg(status_color(&event.state, cx)),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .gap_1()
                                .child(div().text_sm().child(kind(&event.kind)))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(time(event.timestamp_ms)),
                                ),
                        )
                        .child(
                            Button::new(format!("audit-copy-{}", event.id))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Copy)
                                .tooltip(t!("audit_copy").to_string())
                                .on_click(move |_, window, cx| {
                                    crate::feedback::Feedback::copy(
                                        window,
                                        cx,
                                        copy.as_ref().clone(),
                                    );
                                }),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(status_badge(&event.state, cx)),
                );
            if self._owner.is_some()
                && event
                    .context
                    .task_id
                    .as_ref()
                    .is_some_and(|id| pending.contains(id))
            {
                let task = event.context.task_id.clone();
                row = row.child(
                    Button::new(format!("audit-review-{}", event.id))
                        .small()
                        .primary()
                        .label(t!("audit_open_review").to_string())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(owner) = &this._owner {
                                owner.update(cx, |owner, cx| {
                                    owner.mcp_requested_task = task.clone();
                                    owner.show_ai_settings(5, window, cx);
                                });
                            }
                        })),
                );
            }
            row = row
                .child(
                    Button::new(format!("audit-toggle-{}", event.id))
                        .ghost()
                        .small()
                        .icon(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .label(
                            t!(if expanded {
                                "audit_hide_data"
                            } else {
                                "audit_show_data"
                            })
                            .to_string(),
                        )
                        .tooltip(
                            t!(if expanded {
                                "audit_collapse_details"
                            } else {
                                "audit_expand_details"
                            })
                            .to_string(),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.expanded.insert(toggle.clone()) {
                                this.expanded.remove(&toggle);
                            }
                            cx.notify();
                        })),
                )
                .when(expanded, |row| {
                    row.child(Self::human_details(event, cx)).child(
                        div()
                            .id(format!("audit-data-scroll-{}", event.id))
                            .max_h(px(320.))
                            .overflow_y_scroll()
                            .child(
                                TextView::markdown(
                                    format!("audit-data-{}", event.id),
                                    code(text.as_str()),
                                )
                                .selectable(true),
                            ),
                    )
                })
                .child(
                    Button::new(format!("audit-timeline-{}", event.id))
                        .ghost()
                        .xsmall()
                        .label(t!("audit_operation_timeline").to_string())
                        .disabled(self.loading)
                        .on_click(cx.listener(move |this, _, _, cx| this.select(op.clone(), cx))),
                );
            list = list.child(row);
        }
        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(
                h_flex()
                    .p_4()
                    .gap_2()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(t!("audit_detail_title").to_string()),
                    )
                    .child(
                        Button::new("audit-back")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip(t!("audit_back").to_string())
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selection = None;
                                this.load(cx);
                            })),
                    ),
            )
            .child(list)
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .flex_wrap()
                    .justify_between()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("audit-export-detail")
                            .small()
                            .outline()
                            .icon(IconName::ArrowDown)
                            .label(t!("audit_export_json").to_string())
                            .tooltip(t!("audit_export_details").to_string())
                            .disabled(self.loading || self.detail.events.is_empty())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.confirm_export(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("audit-more-details")
                            .small()
                            .ghost()
                            .label(t!("audit_detail_more").to_string())
                            .disabled(self.loading || self.detail.next.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(id) = this.selection.clone() {
                                    this.load_detail(id, this.detail.next, true, cx);
                                }
                            })),
                    ),
            )
            .into_any_element()
    }
}
