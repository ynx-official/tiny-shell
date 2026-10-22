use super::*;
use gpui::InteractiveElement as _;
use gpui_component::{
    IconName,
    checkbox::Checkbox,
    menu::{DropdownMenu as _, PopupMenuItem},
    switch::Switch,
};

impl ProviderForm {
    pub(super) fn render_editor(
        &self,
        snapshot: &Snapshot,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let form = cx.entity();
        let existing = snapshot
            .settings
            .providers
            .iter()
            .any(|p| p.id == self.draft.id);
        let name = self.name.read(cx).value();
        let title = if name.trim().is_empty() {
            t!("ai_new_provider").to_string()
        } else {
            name.to_string()
        };
        let header = h_flex()
            .w_full()
            .min_w_0()
            .flex_wrap()
            .gap_2()
            .justify_between()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(note(t!("ai_protocol").to_string(), cx)),
            )
            .child(
                Switch::new("ai-provider-enabled")
                    .small()
                    .label(t!("ai_provider_enabled").to_string())
                    .checked(self.draft.enabled)
                    .disabled(disabled)
                    .on_click(cx.listener(|this, enabled, _, cx| {
                        this.draft.enabled = *enabled;
                        if !*enabled
                            && this
                                .selection
                                .as_ref()
                                .is_some_and(|s| s.provider_id == this.draft.id)
                        {
                            this.selection = None;
                        }
                        cx.notify();
                    })),
            )
            .child(
                Button::new("ai-provider-actions")
                    .small()
                    .ghost()
                    .icon(IconName::Ellipsis)
                    .tooltip(t!("ai_provider_actions").to_string())
                    .disabled(disabled || !existing)
                    .dropdown_menu(move |menu, window, _| {
                        menu.item(
                            PopupMenuItem::new(t!("ai_delete_provider").to_string()).on_click(
                                window.listener_for(&form, |this, _, window, cx| {
                                    this.request_delete(window, cx)
                                }),
                            ),
                        )
                    }),
            );
        let mut editor = v_flex()
            .flex_1()
            .min_w(px(320.))
            .gap_3()
            .child(header)
            .child(
                div()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_sm()
                    .child(t!("ai_connection_info").to_string()),
            )
            .child(field(
                "ai_provider_name",
                Input::new(&self.name)
                    .small()
                    .disabled(disabled)
                    .into_any_element(),
                cx,
            ))
            .child(field(
                "ai_endpoint",
                Input::new(&self.endpoint)
                    .small()
                    .disabled(disabled)
                    .into_any_element(),
                cx,
            ))
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .gap_2()
                    .justify_between()
                    .child(note(
                        t!(if snapshot.keys_saved.contains(&self.draft.id) {
                            "ai_key_saved"
                        } else {
                            "ai_key_not_saved"
                        })
                        .to_string(),
                        cx,
                    ))
                    .child(
                        Button::new("ai-clear-key")
                            .ghost()
                            .xsmall()
                            .label(t!("ai_clear_key").to_string())
                            .disabled(disabled || !snapshot.keys_saved.contains(&self.draft.id))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let id = this.draft.id.clone();
                                let form = cx.entity();
                                ConfirmationDialog::new(
                                    t!("ai_clear_key").to_string(),
                                    t!("ai_clear_key_notice").to_string(),
                                )
                                .danger(true)
                                .on_ok(move |_, window, cx| {
                                    form.update(cx, |this, cx| {
                                        let result = service().and_then(|s| {
                                            s.clear_key(id.clone()).map_err(|e| e.to_string())
                                        });
                                        this.error = super::super::feedback::report(
                                            result,
                                            "feedback_key_cleared",
                                            window,
                                            cx,
                                        );
                                        cx.notify();
                                    });
                                    true
                                })
                                .open(window, cx);
                            })),
                    ),
            )
            .child(field(
                "ai_api_key",
                Input::new(&self.key)
                    .small()
                    .disabled(disabled)
                    .mask_toggle()
                    .into_any_element(),
                cx,
            ))
            .child(self.render_models(disabled, window, cx))
            .child(self.render_advanced(disabled, cx));
        if snapshot.temporary {
            editor = editor.child(note(t!("ai_temporary").to_string(), cx));
        }
        if snapshot.temporary_keys.contains(&self.draft.id) {
            editor = editor.child(note(t!("ai_key_temporary").to_string(), cx));
        }
        if let Some(error) = snapshot.storage_error.as_ref().or(self.error.as_ref()) {
            editor = editor.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error_text(error)),
            );
        } else if let Some(notice) = &snapshot.notice {
            let success = matches!(notice.as_str(), "AI_SAVED" | "AI_TEST_OK");
            editor = editor.child(
                div()
                    .text_xs()
                    .text_color(if success {
                        cx.theme().muted_foreground
                    } else {
                        cx.theme().danger
                    })
                    .child(error_text(notice)),
            );
        }
        let permissions = self.permissions.clone();
        editor.child(
            v_flex()
                .gap_2()
                .pt_3()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(note(t!("ai_test_notice").to_string(), cx))
                .child(
                    h_flex()
                        .w_full()
                        .flex_wrap()
                        .gap_2()
                        .justify_between()
                        .child(
                            Button::new("ai-open-permissions")
                                .small()
                                .ghost()
                                .label(t!("ai_manage_permissions").to_string())
                                .on_click(move |_, window, cx| permissions(window, cx)),
                        )
                        .child(
                            h_flex()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    Button::new("ai-test-provider")
                                        .small()
                                        .outline()
                                        .label(t!("ai_test_connection").to_string())
                                        .disabled(disabled || self.draft.models.is_empty())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            let mut provider = this.provider(cx);
                                            // Testing a non-default provider must not change the global selection.
                                            provider.default_model = this
                                                .selection
                                                .as_ref()
                                                .filter(|s| {
                                                    s.provider_id == provider.id
                                                        && provider
                                                            .models
                                                            .iter()
                                                            .any(|m| m.id == s.model_id)
                                                })
                                                .map(|s| s.model_id.clone())
                                                .or_else(|| {
                                                    provider.models.first().map(|m| m.id.clone())
                                                })
                                                .unwrap_or_default();
                                            let result = service().and_then(|s| {
                                                s.test_provider(provider, this.key(cx))
                                                    .map_err(|e| e.to_string())
                                            });
                                            this.error = super::super::feedback::report(
                                                result,
                                                "ai_test_ok",
                                                window,
                                                cx,
                                            );
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("ai-save-provider")
                                        .small()
                                        .primary()
                                        .label(t!("ai_save_changes").to_string())
                                        .disabled(disabled)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.request_save(window, cx)
                                        })),
                                ),
                        ),
                ),
        )
    }

    pub(super) fn render_models(
        &self,
        disabled: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let mut models = v_flex()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .child(t!("ai_configured_models").to_string()),
                    )
                    .child(note(self.draft.models.len().to_string(), cx)),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(120.))
                            .child(Input::new(&self.model).small().disabled(disabled)),
                    )
                    .child(
                        Button::new("ai-model-add")
                            .small()
                            .outline()
                            .icon(IconName::Plus)
                            .label(t!("ai_add_model").to_string())
                            .disabled(disabled)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_model(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("ai-model-refresh")
                            .small()
                            .outline()
                            .label(t!("ai_refresh_models").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                let result = service().and_then(|s| {
                                    s.list_models(this.provider(cx), this.key(cx))
                                        .map_err(|e| e.to_string())
                                });
                                this.error = super::super::feedback::report(
                                    result,
                                    "feedback_models_loaded",
                                    window,
                                    cx,
                                );
                                cx.notify();
                            })),
                    ),
            );
        if self.discovery.is_some() {
            models = models.child(
                discovered_model_select(&self.discovered, disabled, window)
                    .debug_selector(|| "ai-discovered-picker".into()),
            );
        }
        models = models.child(
            h_flex()
                .debug_selector(|| "ai-model-header".into())
                .w_full()
                .min_w_0()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(note(t!("ai_model_id").to_string(), cx)),
                )
                .child(
                    div()
                        .w(px(80.))
                        .flex_shrink_0()
                        .child(note(t!("ai_tools_short").to_string(), cx)),
                )
                .child(
                    div()
                        .w(px(56.))
                        .flex_shrink_0()
                        .child(note(t!("ai_model_actions").to_string(), cx)),
                ),
        );
        // The settings page owns scrolling; a second shrinking scroll region clips rows.
        let mut rows = v_flex().w_full().min_w_0().flex_shrink_0();
        for (index, model) in self.draft.models.iter().enumerate() {
            let id = model.id.clone();
            let remove = id.clone();
            let tools_id = id.clone();
            let selected = self.draft.enabled
                && self
                    .selection
                    .as_ref()
                    .is_some_and(|s| s.provider_id == self.draft.id && s.model_id == id);
            rows = rows.child(
                h_flex()
                    .debug_selector(move || format!("ai-model-row-{index}"))
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .items_start()
                    .gap_2()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .items_start()
                            .gap_1()
                            .child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_sm()
                                    .debug_selector(move || format!("ai-model-name-{index}"))
                                    .child(id),
                            )
                            .when(selected, |row| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .flex_shrink_0()
                                        .px_1()
                                        .rounded(px(4.))
                                        .bg(cx.theme().accent)
                                        .text_color(cx.theme().primary)
                                        .child(t!("ai_global_default_badge").to_string()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w(px(80.))
                            .flex_shrink_0()
                            .debug_selector(move || format!("ai-model-tools-cell-{index}"))
                            .child(
                                Checkbox::new(format!("ai-model-tools-{tools_id}"))
                                    .tooltip(t!("ai_tools_short").to_string())
                                    .checked(model.supports_tools)
                                    .disabled(disabled)
                                    .on_click(cx.listener(move |this, on, _, cx| {
                                        if let Some(model) =
                                            this.draft.models.iter_mut().find(|m| m.id == tools_id)
                                        {
                                            model.supports_tools = *on;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        Button::new(format!("ai-remove-model-{remove}"))
                            .ghost()
                            .xsmall()
                            .w(px(56.))
                            .flex_shrink_0()
                            .icon(IconName::Close)
                            .tooltip(t!("ai_remove_model").to_string())
                            .disabled(disabled)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.remove_model(&remove, cx)),
                            ),
                    ),
            );
        }
        if self.draft.models.is_empty() {
            rows = rows.child(note(t!("ai_models_empty").to_string(), cx));
        }
        models.child(rows).child(
            note(t!("ai_model_capability_notice").to_string(), cx)
                .debug_selector(|| "ai-model-help".into()),
        )
    }

    fn render_advanced(&self, disabled: bool, cx: &mut Context<Self>) -> gpui::Div {
        let mut section = v_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("ai-provider-advanced")
                            .small()
                            .ghost()
                            .icon(if self.advanced {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .label(t!("ai_advanced_options").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.advanced = !this.advanced;
                                cx.notify();
                            })),
                    )
                    .child(note(t!("ai_advanced_summary").to_string(), cx)),
            );
        if self.advanced {
            section = section
                .child(
                    Checkbox::new("ai-stream-usage")
                        .label(t!("audit_stream_usage").to_string())
                        .checked(self.draft.stream_usage)
                        .disabled(disabled)
                        .on_click(cx.listener(|this, on, _, cx| {
                            this.draft.stream_usage = *on;
                            cx.notify();
                        })),
                )
                .child(
                    Checkbox::new("ai-no-key")
                        .label(t!("ai_allow_no_key").to_string())
                        .checked(self.draft.allow_no_key)
                        .disabled(disabled)
                        .on_click(cx.listener(|this, on, _, cx| {
                            this.draft.allow_no_key = *on;
                            cx.notify();
                        })),
                )
                .child(
                    Checkbox::new("ai-http")
                        .label(t!("ai_allow_http").to_string())
                        .checked(self.draft.allow_http)
                        .disabled(disabled)
                        .on_click(cx.listener(|this, on, window, cx| {
                            if *on {
                                let form = cx.entity();
                                ConfirmationDialog::new(
                                    t!("ai_allow_http").to_string(),
                                    t!("ai_http_notice").to_string(),
                                )
                                .on_ok(move |_, _, cx| {
                                    form.update(cx, |this, cx| {
                                        this.draft.allow_http = true;
                                        cx.notify();
                                    });
                                    true
                                })
                                .open(window, cx);
                            } else {
                                this.draft.allow_http = false;
                                cx.notify();
                            }
                        })),
                )
                .child(note(t!("ai_key_privacy").to_string(), cx));
        }
        section
    }
}
