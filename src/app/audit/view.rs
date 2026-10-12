use super::*;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    ClipboardItem, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    menu::{DropdownMenu as _, PopupMenuItem},
    text::TextView,
    v_flex,
};
use tiny_shell_mcp::audit::{Category, Event};

pub(crate) fn usage_label(c: &crate::ai::Conversation) -> String {
    let summary = c.usage_summary();
    let legacy = c
        .turns
        .first()
        .map_or(!c.messages.is_empty(), |t| t.user_message > 0);
    if c.requests.is_empty() && legacy {
        return t!("audit_legacy_usage").to_string();
    }
    let label = format!(
        "{} · {}",
        t!("audit_total_tokens", count = summary.total),
        t!("audit_unknown_requests", count = summary.unknown)
    );
    if legacy {
        format!("{label} · {}", t!("audit_legacy_also"))
    } else {
        label
    }
}
pub(crate) fn compact_usage_label(c: &crate::ai::Conversation) -> String {
    if c.requests.is_empty() && !c.messages.is_empty() {
        return t!("audit_usage_unreported").to_string();
    }
    let sum = c.usage_summary();
    format!(
        "{}{}",
        t!("audit_compact_tokens", count = sum.total),
        if sum.unknown > 0 { " *" } else { "" }
    )
}
fn source(source: Source) -> String {
    t!(match source {
        Source::Ai => "audit_source_ai",
        Source::External => "audit_source_external",
        Source::Local => "audit_source_local",
        Source::System => "audit_source_system",
    })
    .to_string()
}
fn category(category: Category) -> String {
    t!(match category {
        Category::Create => "audit_create",
        Category::Delete => "audit_delete",
        Category::Update => "audit_update",
        Category::Read => "audit_read",
        Category::Control => "audit_control",
        Category::Command => "audit_command",
        Category::Model => "audit_model",
        Category::Unknown => "audit_unknown",
    })
    .to_string()
}
pub(super) fn state(state: &str) -> String {
    t!(match state {
        "approved" => "audit_approved",
        "rejected" => "audit_rejected",
        "expired" => "audit_expired",
        "invalidated" => "audit_invalidated",
        "awaiting_user" | "pending" => "audit_pending",
        "queued" => "audit_queued",
        "running" | "received" | "reported" => "audit_running",
        "succeeded" | "completed" | "recorded" => "audit_completed",
        "failed" => "audit_failed",
        "cached" => "audit_cached",
        "idempotent" => "audit_idempotent",
        "not_executed" => "audit_not_executed",
        "cancelled" | "cancel_requested" => "audit_cancelled",
        "outcome_unknown" | "timed_out" => "audit_uncertain",
        "interrupted" => "audit_interrupted",
        _ => "audit_unknown",
    })
    .to_string()
}
fn time(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}
fn title(event: &Event) -> String {
    if event.tool == "model" && event.details["request"].is_object() {
        let request = &event.details["request"];
        return format!(
            "{}{} · {}",
            if event.details["purpose"] == "provider_connection_test" {
                format!("{} · ", t!("audit_provider_test"))
            } else {
                String::new()
            },
            request["provider_name"].as_str().unwrap_or(""),
            request["model"].as_str().unwrap_or("")
        );
    }
    let name = t!(match event.tool.as_str() {
        "run_command" => "audit_tool_command",
        "read_file" => "audit_tool_read",
        "write_file" => "audit_tool_write",
        "list_directory" => "audit_tool_directory",
        "get_status" => "audit_tool_status",
        "list_connections" => "audit_tool_connections",
        "get_task" => "audit_tool_task",
        "cancel_task" => "audit_tool_cancel",
        "open_session" => "audit_tool_open",
        "close_session" => "audit_tool_close",
        "model" => "audit_tool_model",
        "budget" => "audit_tool_budget",
        _ => "audit_tool_other",
    });
    if let Some(purpose) = &event.context.purpose {
        return format!("{name} · {}", purpose.lines().next().unwrap_or(""));
    }
    let args = event
        .details
        .get("arguments")
        .or_else(|| event.details["call"].get("arguments"));
    let detail = args
        .and_then(|v| v["command"].as_str().or_else(|| v["path"].as_str()))
        .unwrap_or("");
    if detail.is_empty() {
        name.to_string()
    } else {
        format!("{name} · {detail}")
    }
}
fn kind(value: &str) -> String {
    t!(match value {
        "request_received" | "external_received" | "tool_requested" => "audit_event_request",
        "request_result" => "audit_event_response",
        "external_rejected" | "authentication_rejected" | "external_transport_failed" =>
            "audit_event_rejection",
        "execution_started" => "audit_event_execution",
        "execution_result" | "tool_completed" => "audit_event_result",
        "approval_requested" => "audit_event_approval_request",
        "approval_approved" | "approval_rejected" | "approval_expired" | "approval_invalidated" =>
            "audit_event_approval_decision",
        "model_started" | "turn_started" => "audit_event_model_start",
        "model_usage" => "audit_event_usage",
        "model_finished" | "turn_finished" => "audit_event_model_finish",
        "budget_requested" | "budget_approved" | "budget_rejected" => "audit_event_budget",
        "tool_cached" => "audit_cached",
        "legacy_summary" => "audit_event_legacy",
        "application_recovered" | "recovery_interrupted" => "audit_event_recovery",
        _ => "audit_event_other",
    })
    .to_string()
}
pub(super) fn code(text: &str) -> String {
    let fence = "`".repeat(
        text.split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .max(2)
            + 1,
    );
    format!("{fence}json\n{text}\n{fence}")
}
impl AuditPanel {
    pub(super) fn view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.status.ready && !self.settings_loaded {
            self.settings_loaded = true;
            self.days.update(cx, |i, cx| {
                i.set_value(self.status.settings.retention_days.to_string(), window, cx)
            });
            self.capacity.update(cx, |i, cx| {
                i.set_value(self.status.settings.capacity_mb.to_string(), window, cx)
            });
            self.patterns.update(cx, |i, cx| {
                i.set_value(self.status.settings.redact_patterns.join("\n"), window, cx)
            });
        }
        let mut nav = h_flex().gap_2().flex_wrap();
        for (tab, key) in [
            (Tab::Usage, "audit_usage"),
            (Tab::Operations, "audit_operations"),
            (Tab::Approvals, "audit_approvals"),
            (Tab::Settings, "audit_settings"),
        ] {
            nav = nav.child(
                Button::new(key)
                    .small()
                    .outline()
                    .label(t!(key).to_string())
                    .selected(self.tab == tab)
                    .disabled(self.loading)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = tab;
                        this.selection = None;
                        this.filter.before_sequence = None;
                        if tab == Tab::Usage {
                            this.filter.source = Some(Source::Ai);
                        }
                        this.load(cx);
                    })),
            );
        }
        let mut content = v_flex()
            .size_full()
            .min_h_0()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                h_flex()
                    .p_3()
                    .gap_3()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(nav)
                    .child(
                        Button::new("audit-refresh")
                            .small()
                            .ghost()
                            .label(t!("audit_refresh").to_string())
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(id) = this.selection.clone() {
                                    this.load_detail(id, None, false, cx);
                                } else {
                                    this.load(cx);
                                }
                            })),
                    ),
            );
        if !self.status.durable {
            content = content.child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(cx.theme().warning)
                    .child(t!("audit_temporary").to_string()),
            );
        }
        if self.status.error.is_some() || self.error.is_some() {
            content = content.child(
                h_flex()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .gap_2()
                    .child(
                        t!(match self.error.as_deref() {
                            Some("AUDIT_INVALID_SETTINGS") => "audit_invalid_settings",
                            Some("AUDIT_EXPORT_LIMIT") => "audit_export_limit",
                            _ => "audit_storage_error",
                        })
                        .to_string(),
                    )
                    .child(
                        Button::new("audit-retry")
                            .small()
                            .label(t!("audit_retry").to_string())
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                    ),
            );
        }
        if self.status.bytes >= u64::from(self.status.settings.capacity_mb) * 1024 * 1024 * 9 / 10 {
            content = content.child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(cx.theme().warning)
                    .child(t!("audit_capacity_warning").to_string()),
            );
        }
        if let Some(notice) = &self.notice {
            content = content.child(
                div()
                    .px_3()
                    .text_sm()
                    .child(t!(notice.as_str()).to_string()),
            );
        }
        if self.tab == Tab::Settings {
            return content.child(self.settings_view(cx)).into_any_element();
        }
        if self.selection.is_some() {
            return content.child(self.detail_view(cx)).into_any_element();
        }
        if self.tab == Tab::Usage {
            let summary = self
                .filter
                .conversation_id
                .as_ref()
                .and_then(|id| {
                    crate::app::ai::service()
                        .ok()
                        .and_then(|s| s.snapshot().conversations.into_iter().find(|c| &c.id == id))
                })
                .map(|c| c.usage_summary())
                .unwrap_or_else(|| self.summary.clone());
            content = content.child(
                v_flex()
                    .px_3()
                    .py_3()
                    .gap_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_lg()
                            .child(t!("audit_total_tokens", count = summary.total).to_string()),
                    )
                    .child(
                        div().text_sm().child(
                            t!(
                                "audit_usage_breakdown",
                                input = summary.input,
                                output = summary.output,
                                requests = summary.requests
                            )
                            .to_string(),
                        ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                t!(
                                    "audit_usage_subtotals",
                                    cached = summary.cached_input,
                                    reasoning = summary.reasoning_output,
                                    unknown = summary.unknown
                                )
                                .to_string(),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("audit_usage_note").to_string()),
                    ),
            );
        }
        let view = cx.entity();
        let mut filters = h_flex().px_3().py_2().gap_2().flex_wrap();
        for (choice, key) in [
            (None, "audit_source_all"),
            (Some(Source::Ai), "audit_source_ai"),
            (Some(Source::External), "audit_source_external"),
        ] {
            if self.tab == Tab::Usage {
                continue;
            }
            filters = filters.child(
                Button::new(key)
                    .ghost()
                    .small()
                    .label(t!(key).to_string())
                    .selected(self.filter.source == choice)
                    .disabled(self.loading)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.filter.source = choice;
                        this.filter.before_sequence = None;
                        this.load(cx);
                    })),
            );
        }
        if self.tab != Tab::Usage {
            filters = filters.child(
                Button::new("audit-category")
                    .small()
                    .outline()
                    .label(
                        self.filter
                            .category
                            .map(category)
                            .unwrap_or_else(|| t!("audit_category_all").to_string()),
                    )
                    .dropdown_menu(move |mut menu, window, _| {
                        for cat in [
                            None,
                            Some(Category::Create),
                            Some(Category::Delete),
                            Some(Category::Update),
                            Some(Category::Read),
                            Some(Category::Command),
                            Some(Category::Control),
                            Some(Category::Model),
                        ] {
                            menu = menu.item(
                                PopupMenuItem::new(
                                    cat.map(category)
                                        .unwrap_or_else(|| t!("audit_category_all").to_string()),
                                )
                                .on_click(window.listener_for(
                                    &view,
                                    move |this, _, _, cx| {
                                        this.filter.category = cat;
                                        this.filter.before_sequence = None;
                                        this.load(cx);
                                    },
                                )),
                            );
                        }
                        menu
                    }),
            );
        }
        content = content.child(filters).child(self.extra_filters(cx)).child(
            h_flex()
                .px_3()
                .gap_2()
                .child(Input::new(&self.search).small().disabled(self.loading))
                .child(
                    Button::new("audit-search")
                        .small()
                        .label(t!("audit_search_button").to_string())
                        .disabled(self.loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.filter.before_sequence = None;
                            this.load(cx);
                        })),
                )
                .child(
                    Button::new("audit-export")
                        .small()
                        .outline()
                        .label(t!("audit_export_summary").to_string())
                        .on_click(cx.listener(|this, _, _, cx| this.export_summary(cx))),
                ),
        );
        let mut list = v_flex()
            .id("audit-record-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .gap_2();
        if self.page.events.is_empty() {
            list = list.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        t!(if self.loading {
                            "audit_loading"
                        } else {
                            "audit_empty"
                        })
                        .to_string(),
                    ),
            );
        }
        for event in &self.page.events {
            let id = event.context.operation_id.clone();
            let target = event
                .context
                .target_name
                .as_deref()
                .or_else(|| {
                    event.details["target"]["name"]
                        .as_str()
                        .or_else(|| event.details["request"]["conversation_title"].as_str())
                })
                .map(str::to_owned)
                .or_else(|| event.context.target_id.clone())
                .unwrap_or_default();
            let count = event.details["request"]["usage"]["total"].as_u64();
            list = list.child(
                v_flex()
                    .id(event.id.clone())
                    .w_full()
                    .flex_shrink_0()
                    .p_2()
                    .gap_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .hover(|s| s.bg(cx.theme().secondary))
                    .child(
                        h_flex()
                            .gap_3()
                            .justify_between()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(title(event)),
                            )
                            .child(div().text_sm().child(state(&event.state))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} · {} · {} · {}",
                                time(event.timestamp_ms),
                                event
                                    .context
                                    .client_name
                                    .clone()
                                    .unwrap_or_else(|| source(event.context.source)),
                                category(event.category),
                                target
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if event.context.source == Source::External {
                                t!("audit_mcp_no_tokens").to_string()
                            } else if event.tool == "model" && event.details["request"].is_object()
                            {
                                count
                                    .map(|n| t!("audit_total_tokens", count = n).to_string())
                                    .unwrap_or_else(|| {
                                        t!(if matches!(event.state.as_str(), "running" | "pending")
                                        {
                                            "audit_usage_pending"
                                        } else {
                                            "audit_usage_unreported"
                                        })
                                        .to_string()
                                    })
                            } else {
                                kind(&event.kind)
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx))),
            );
        }
        content
            .child(list)
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("audit-latest")
                            .small()
                            .ghost()
                            .label(t!("audit_latest").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filter.before_sequence = None;
                                this.load(cx);
                            })),
                    )
                    .child(
                        Button::new("audit-more")
                            .small()
                            .outline()
                            .label(t!("audit_next").to_string())
                            .disabled(self.loading || self.page.next.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filter.before_sequence = this.page.next;
                                this.load(cx);
                            })),
                    ),
            )
            .into_any_element()
    }
    fn detail_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let pending = crate::app::mcp::service()
            .ok()
            .and_then(|s| s.engine.reviews().ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.state == tiny_shell_mcp::tasks::State::AwaitingUser)
            .map(|r| r.id)
            .collect::<std::collections::HashSet<_>>();
        let mut list = v_flex()
            .id("audit-detail-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .gap_3();
        for event in self.detail.events.iter().rev() {
            let text = self.formatted.get(&event.id).cloned().unwrap_or_default();
            let copy = text.clone();
            let op = event.context.operation_id.clone();
            let toggle = event.id.clone();
            let expanded = self.expanded.contains(&event.id);
            list = list.child(
                v_flex()
                    .w_full()
                    .flex_shrink_0()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .pb_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .gap_2()
                            .child(div().child(format!(
                                "{} · {} · {}",
                                time(event.timestamp_ms),
                                kind(&event.kind),
                                state(&event.state)
                            )))
                            .child(
                                Button::new(format!("audit-copy-{}", event.id))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Copy)
                                    .tooltip(t!("audit_copy").to_string())
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy.as_ref().clone(),
                                        ))
                                    }),
                            ),
                    )
                    .child(div().text_sm().child(title(event)))
                    .when(
                        self._owner.is_some()
                            && event
                                .context
                                .task_id
                                .as_ref()
                                .is_some_and(|id| pending.contains(id)),
                        |row| {
                            let task = event.context.task_id.clone();
                            row.child(
                                Button::new(format!("audit-review-{}", event.id))
                                    .small()
                                    .label(t!("audit_open_review").to_string())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(owner) = &this._owner {
                                            owner.update(cx, |owner, cx| {
                                                owner.mcp_requested_task = task.clone();
                                                owner.show_ai_settings(5, window, cx);
                                            });
                                        }
                                    })),
                            )
                        },
                    )
                    .child(
                        Button::new(format!("audit-toggle-{}", event.id))
                            .ghost()
                            .small()
                            .label(
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
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.select(op.clone(), cx)),
                            ),
                    ),
            );
        }
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .child(
                        Button::new("audit-back")
                            .ghost()
                            .small()
                            .icon(IconName::ArrowLeft)
                            .label(t!("audit_back").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selection = None;
                                this.load(cx);
                            })),
                    )
                    .child(
                        Button::new("audit-export-detail")
                            .small()
                            .outline()
                            .label(t!("audit_export_details").to_string())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.confirm_export(window, cx)),
                            ),
                    ),
            )
            .child(list)
            .child(
                Button::new("audit-more-details")
                    .small()
                    .label(t!("audit_detail_more").to_string())
                    .disabled(self.loading || self.detail.next.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(id) = this.selection.clone() {
                            this.load_detail(id, this.detail.next, true, cx);
                        }
                    })),
            )
            .into_any_element()
    }
    fn settings_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .id("audit-settings")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(div().text_sm().child(t!("audit_privacy_note").to_string()))
            .child(div().child(t!("audit_retention_days").to_string()))
            .child(Input::new(&self.days).small())
            .child(div().child(t!("audit_capacity_mb").to_string()))
            .child(Input::new(&self.capacity).small())
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        t!("audit_storage_size", mb = self.status.bytes / (1024 * 1024))
                            .to_string(),
                    ),
            )
            .child(div().child(t!("audit_redact_patterns").to_string()))
            .child(Input::new(&self.patterns).small())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("audit_redact_note").to_string()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("audit-save-settings")
                            .primary()
                            .small()
                            .label(t!("audit_save_settings").to_string())
                            .on_click(cx.listener(|this, _, _, cx| this.save_settings(cx))),
                    )
                    .child(
                        Button::new("audit-clear")
                            .danger()
                            .small()
                            .label(t!("audit_clear").to_string())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.confirm_clear(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}
