use super::components::{hint, record_icon, status_badge};
use super::presentation::{category, format_count, record_text, source, target, time};
use super::*;
use gpui::{
    InteractiveElement as _, IntoElement as _, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, div, prelude::FluentBuilder as _, rems,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

impl AuditPanel {
    pub(super) fn records_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let usage = self.tab == Tab::Usage;
        // An open inspector already shows the exact time and target. Keep the usage title readable.
        let show_time = !usage || self.selection.is_none();
        let time_width = if usage { rems(10.) } else { rems(7.) };
        let title = match self.tab {
            Tab::Usage => "audit_request_details",
            Tab::Approvals => "audit_approvals",
            _ => "audit_record_list",
        };
        let mut list = v_flex()
            .id("audit-record-list")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll();
        if self.page.events.is_empty() {
            list = list.child(self.empty_records(cx));
        }
        for event in &self.page.events {
            let id = event.context.operation_id.clone();
            let selected = self.selection.as_ref() == Some(&id);
            let text = record_text(event);
            let timestamp = time(event.timestamp_ms);
            let (date, clock) = timestamp
                .split_once(' ')
                .unwrap_or(("", timestamp.as_str()));
            let mut tint = cx.theme().primary;
            tint.a *= 0.08;
            let mut description = v_flex().min_w_0().flex_1().gap_1().child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_ellipsis()
                    .child(text.heading),
            );
            if let Some(technical) = text.technical {
                description = description.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_ellipsis()
                        .child(technical),
                );
            }
            let metadata = [
                event
                    .context
                    .client_name
                    .clone()
                    .unwrap_or_else(|| source(event.context.source)),
                if usage {
                    String::new()
                } else {
                    category(event.category)
                },
                target(event),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
            description = description.when(!usage, |description| {
                description.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .text_ellipsis()
                        .child(metadata),
                )
            });
            let mut row = h_flex()
                .id(event.id.clone())
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .items_start()
                .px_6()
                .py_2()
                .gap_3()
                .border_b_1()
                .border_color(cx.theme().border)
                .cursor_pointer()
                .when(selected, |row| row.bg(tint))
                .hover(|row| row.bg(cx.theme().secondary))
                .child(
                    div().w(px(18.)).flex_shrink_0().pt_1().child(
                        Icon::new(record_icon(&event.tool))
                            .size(px(18.))
                            .text_color(cx.theme().primary),
                    ),
                )
                .child(description)
                .when(show_time, |row| {
                    row.child(
                        v_flex()
                            .w(time_width)
                            .flex_shrink_0()
                            .gap_1()
                            .child(div().text_sm().child(if usage {
                                timestamp.clone()
                            } else {
                                clock.to_owned()
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .text_ellipsis()
                                    .child(if usage {
                                        target(event)
                                    } else {
                                        date.to_owned()
                                    }),
                            ),
                    )
                });
            if usage {
                let count = event.details["request"]["usage"]["total"].as_u64();
                row = row.child(
                    div()
                        .w(rems(6.))
                        .flex_shrink_0()
                        .text_right()
                        .text_sm()
                        .child(count.map(format_count).unwrap_or_else(|| {
                            t!(if matches!(event.state.as_str(), "running" | "pending") {
                                "audit_tokens_pending"
                            } else {
                                "audit_tokens_unknown"
                            })
                            .to_string()
                        })),
                );
            }
            row = row
                .child(
                    h_flex()
                        .w(rems(8.))
                        .flex_shrink_0()
                        .justify_end()
                        .child(status_badge(&event.state, cx)),
                )
                .child(
                    Icon::new(IconName::ChevronRight)
                        .size(px(14.))
                        .text_color(cx.theme().muted_foreground),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)));
            list = list.child(row);
        }
        v_flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(
                h_flex()
                    .px_6()
                    .pt_3()
                    .pb_2()
                    .gap_3()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(t!(title).to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                t!("audit_page_count", count = self.page.events.len()).to_string(),
                            ),
                    ),
            )
            .when(!self.page.events.is_empty(), |body| {
                body.child(
                    h_flex()
                        .px_6()
                        .py_2()
                        .gap_3()
                        .bg(cx.theme().muted)
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(div().w(px(18.)).flex_shrink_0())
                        .child(
                            div().flex_1().min_w_0().child(
                                t!(if usage {
                                    "audit_column_model"
                                } else {
                                    "audit_column_operation"
                                })
                                .to_string(),
                            ),
                        )
                        .when(show_time, |row| {
                            row.child(
                                div().w(time_width).flex_shrink_0().child(
                                    t!(if usage {
                                        "audit_column_context"
                                    } else {
                                        "audit_column_time"
                                    })
                                    .to_string(),
                                ),
                            )
                        })
                        .when(usage, |row| {
                            row.child(
                                div()
                                    .w(rems(6.))
                                    .flex_shrink_0()
                                    .text_right()
                                    .child(t!("audit_column_tokens").to_string()),
                            )
                        })
                        .child(
                            div()
                                .w(rems(8.))
                                .flex_shrink_0()
                                .text_right()
                                .child(t!("audit_column_state").to_string()),
                        )
                        .child(div().w(px(14.)).flex_shrink_0()),
                )
            })
            .child(list)
            .child(
                h_flex()
                    .px_6()
                    .py_3()
                    .gap_3()
                    .flex_wrap()
                    .justify_between()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                t!("audit_displayed_count", count = self.page.events.len())
                                    .to_string(),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("audit-latest")
                                    .small()
                                    .ghost()
                                    .label(t!("audit_latest").to_string())
                                    .disabled(self.loading)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.filter.before_sequence = None;
                                        this.selection = None;
                                        this.load(cx);
                                    })),
                            )
                            .child(
                                Button::new("audit-more")
                                    .small()
                                    .outline()
                                    .icon(IconName::ArrowDown)
                                    .label(t!("audit_next").to_string())
                                    .disabled(self.loading || self.page.next.is_none())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.filter.before_sequence = this.page.next;
                                        this.selection = None;
                                        this.load(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn empty_records(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let failed = self.error.is_some() || self.status.error.is_some();
        let title = if self.loading {
            "audit_loading"
        } else if failed {
            "audit_storage_error"
        } else if self.page.next.is_some() {
            "audit_no_matches_yet"
        } else if self.tab == Tab::Approvals {
            "audit_approvals_empty"
        } else {
            "audit_empty"
        };
        let help = if self.page.next.is_some() {
            "audit_continue_search_note"
        } else if self.tab == Tab::Approvals {
            "audit_approvals_empty_help"
        } else {
            "audit_empty_help"
        };
        v_flex()
            .size_full()
            .min_h(px(160.))
            .p_6()
            .gap_3()
            .items_center()
            .justify_center()
            .child(
                Icon::new(if self.loading {
                    IconName::LoaderCircle
                } else if failed {
                    IconName::TriangleAlert
                } else {
                    IconName::Inbox
                })
                .size(px(44.))
                .text_color(cx.theme().primary),
            )
            .child(
                div()
                    .max_w(px(460.))
                    .text_center()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(t!(title).to_string()),
            )
            .when(!self.loading && !failed, |body| {
                body.child(
                    div()
                        .max_w(px(460.))
                        .text_center()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!(help).to_string()),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .justify_center()
                        .child(
                            Button::new("audit-empty-reset")
                                .small()
                                .primary()
                                .label(t!("audit_reset_filters").to_string())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.reset_filters(window, cx)
                                })),
                        )
                        .child(
                            Button::new("audit-empty-refresh")
                                .small()
                                .outline()
                                .label(t!("audit_refresh").to_string())
                                .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                        ),
                )
                .when(self.tab == Tab::Approvals, |body| {
                    body.child(div().mt_2().max_w(px(460.)).child(hint(
                        t!("audit_approval_hint").to_string(),
                        cx.theme().muted_foreground,
                    )))
                })
            })
            .into_any_element()
    }
}
