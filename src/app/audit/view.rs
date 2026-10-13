use super::*;
use super::{
    components::{hint, metric},
    presentation::{category, format_count, inline_inspector, input_fraction, usage_status_labels},
};
use gpui::{
    IntoElement as _, ParentElement as _, Styled as _, div, prelude::FluentBuilder as _, relative,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    menu::{DropdownMenu as _, PopupMenuItem},
    tab::{Tab as NavigationTab, TabBar},
    v_flex,
};
use tiny_shell_mcp::audit::Category;

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
        let mut content = v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.header(cx));
        if !self.status.durable {
            content = content.child(
                div()
                    .px_6()
                    .py_2()
                    .child(hint(t!("audit_temporary").to_string(), cx.theme().warning)),
            );
        }
        if self.status.error.is_some() || self.error.is_some() {
            content = content.child(
                h_flex()
                    .px_6()
                    .py_2()
                    .gap_3()
                    .flex_wrap()
                    .child(
                        div().flex_1().min_w_0().child(hint(
                            t!(match self.error.as_deref() {
                                Some("AUDIT_INVALID_SETTINGS") => "audit_invalid_settings",
                                Some("AUDIT_EXPORT_LIMIT") => "audit_export_limit",
                                _ => "audit_storage_error",
                            })
                            .to_string(),
                            cx.theme().danger,
                        )),
                    )
                    .child(
                        Button::new("audit-retry")
                            .small()
                            .outline()
                            .label(t!("audit_retry").to_string())
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                    ),
            );
        }
        if self.status.bytes >= u64::from(self.status.settings.capacity_mb) * 1024 * 1024 * 9 / 10 {
            content = content.child(div().px_6().py_2().child(hint(
                t!("audit_capacity_warning").to_string(),
                cx.theme().warning,
            )));
        }
        if let Some(notice) = &self.notice {
            content = content.child(
                div()
                    .px_6()
                    .py_2()
                    .child(hint(t!(notice.as_str()).to_string(), cx.theme().success)),
            );
        }
        if self.tab == Tab::Settings {
            return content.child(self.settings_view(cx)).into_any_element();
        }
        let show_inspector = self.selection.is_some()
            && inline_inspector(window.viewport_size().width, window.rem_size());
        if self.selection.is_some() && !show_inspector {
            return content.child(self.detail_view(cx)).into_any_element();
        }
        if self.tab == Tab::Usage {
            content = content.child(self.usage_overview(cx));
        }
        content
            .child(self.filter_toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .items_stretch()
                    .child(self.records_view(cx))
                    .when(show_inspector, |row| {
                        row.child(
                            div()
                                .w(px(360.) * (window.rem_size() / px(14.)).max(1.))
                                .flex_shrink_0()
                                .h_full()
                                .min_h_0()
                                .border_l_1()
                                .border_color(cx.theme().border)
                                .child(self.detail_view(cx)),
                        )
                    }),
            )
            .into_any_element()
    }

    fn header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tabs = [Tab::Usage, Tab::Operations, Tab::Approvals, Tab::Settings];
        v_flex()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .px_6()
                    .pt_4()
                    .pb_3()
                    .justify_between()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(t!("audit_title").to_string()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("audit_subtitle").to_string()),
                            ),
                    )
                    .child(
                        Button::new("audit-refresh")
                            .small()
                            .outline()
                            .icon(IconName::Redo)
                            .label(t!("audit_refresh").to_string())
                            .loading(self.loading)
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(id) = this.selection.clone() {
                                    this.load_detail(id, None, false, cx);
                                } else {
                                    this.load(cx);
                                }
                            })),
                    ),
            )
            .child(
                div().px_6().child(
                    TabBar::new("audit-tabs")
                        .underline()
                        .small()
                        .menu(true)
                        .selected_index(
                            tabs.iter()
                                .position(|tab| *tab == self.tab)
                                .unwrap_or_default(),
                        )
                        .child(
                            NavigationTab::new()
                                .label(t!("audit_usage").to_string())
                                .disabled(self.loading),
                        )
                        .child(
                            NavigationTab::new()
                                .label(t!("audit_operations").to_string())
                                .disabled(self.loading),
                        )
                        .child(
                            NavigationTab::new()
                                .label(t!("audit_approvals").to_string())
                                .disabled(self.loading),
                        )
                        .child(
                            NavigationTab::new()
                                .label(t!("audit_settings").to_string())
                                .disabled(self.loading),
                        )
                        .on_click(cx.listener(move |this, index, _, cx| {
                            if this.loading {
                                return;
                            }
                            this.tab = tabs[*index];
                            this.selection = None;
                            this.filter.before_sequence = None;
                            this.load(cx);
                        })),
                ),
            )
            .into_any_element()
    }

    fn usage_overview(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
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
        let fraction = input_fraction(summary.input, summary.output);
        let mut overview = v_flex()
            .px_6()
            .py_4()
            .gap_3()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_4()
                    .flex_wrap()
                    .items_center()
                    .child(
                        v_flex()
                            .min_w(px(144.))
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(32.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(format_count(summary.total)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("audit_confirmed_tokens").to_string()),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(180.))
                            .gap_2()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .gap_3()
                                    .text_xs()
                                    .child(
                                        t!(
                                            "audit_input_tokens",
                                            count = format_count(summary.input)
                                        )
                                        .to_string(),
                                    )
                                    .child(
                                        t!(
                                            "audit_output_tokens",
                                            count = format_count(summary.output)
                                        )
                                        .to_string(),
                                    ),
                            )
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(6.))
                                    .rounded(px(3.))
                                    .overflow_hidden()
                                    .bg(cx.theme().secondary)
                                    .when_some(fraction, |bar, fraction| {
                                        bar.child(
                                            div()
                                                .h_full()
                                                .w(relative(fraction))
                                                .bg(cx.theme().primary),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("audit_component_ratio").to_string()),
                            ),
                    )
                    .child(metric(
                        "audit_model_requests",
                        summary.requests.to_string(),
                        cx,
                    ))
                    .child(metric(
                        "audit_cache_hits",
                        format_count(summary.cached_input),
                        cx,
                    ))
                    .child(metric(
                        "audit_reasoning_tokens",
                        format_count(summary.reasoning_output),
                        cx,
                    )),
            );
        let incomplete = usage_status_labels(&summary);
        if !incomplete.is_empty() {
            overview = overview.child(hint(incomplete.join(" · "), cx.theme().warning));
        }
        overview
            .child(hint(
                t!("audit_usage_note").to_string(),
                cx.theme().muted_foreground,
            ))
            .into_any_element()
    }

    fn filter_toolbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let panel = cx.entity();
        let mut toolbar = v_flex()
            .px_6()
            .py_3()
            .gap_2()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border);
        if self.tab != Tab::Usage {
            toolbar = toolbar.child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .child(
                        TabBar::new("audit-source-tabs")
                            .segmented()
                            .small()
                            .selected_index(match self.filter.source {
                                None => 0,
                                Some(Source::Ai) => 1,
                                Some(Source::External) => 2,
                                _ => usize::MAX,
                            })
                            .child(
                                NavigationTab::new()
                                    .label(t!("audit_source_all").to_string())
                                    .disabled(self.loading),
                            )
                            .child(
                                NavigationTab::new()
                                    .label(t!("audit_source_ai").to_string())
                                    .disabled(self.loading),
                            )
                            .child(
                                NavigationTab::new()
                                    .label(t!("audit_source_external").to_string())
                                    .disabled(self.loading),
                            )
                            .on_click(cx.listener(|this, index, _, cx| {
                                if this.loading {
                                    return;
                                }
                                this.filter.source =
                                    [None, Some(Source::Ai), Some(Source::External)][*index];
                                this.filter.before_sequence = None;
                                this.selection = None;
                                this.load(cx);
                            })),
                    )
                    .child(
                        Button::new("audit-category")
                            .small()
                            .outline()
                            .disabled(self.loading)
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
                                        PopupMenuItem::new(cat.map(category).unwrap_or_else(
                                            || t!("audit_category_all").to_string(),
                                        ))
                                        .on_click(
                                            window.listener_for(&panel, move |this, _, _, cx| {
                                                this.filter.category = cat;
                                                this.filter.before_sequence = None;
                                                this.selection = None;
                                                this.load(cx);
                                            }),
                                        ),
                                    );
                                }
                                menu
                            }),
                    ),
            );
        }
        toolbar
            .child(self.extra_filters(cx))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        div().flex_1().min_w(px(240.)).child(
                            Input::new(&self.search)
                                .small()
                                .prefix(Icon::new(IconName::Search).size(px(14.)))
                                .disabled(self.loading),
                        ),
                    )
                    .child(
                        Button::new("audit-search")
                            .small()
                            .primary()
                            .label(t!("audit_search_button").to_string())
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filter.before_sequence = None;
                                this.selection = None;
                                this.load(cx);
                            })),
                    )
                    .child(
                        Button::new("audit-reset")
                            .small()
                            .ghost()
                            .label(t!("audit_reset_filters").to_string())
                            .disabled(self.loading)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.reset_filters(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("audit-export")
                            .small()
                            .outline()
                            .icon(IconName::ArrowDown)
                            .label(t!("audit_export_csv").to_string())
                            .tooltip(t!("audit_export_summary").to_string())
                            .disabled(self.loading || self.page.events.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.export_summary(cx))),
                    ),
            )
            .into_any_element()
    }
}
