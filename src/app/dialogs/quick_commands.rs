use super::*;

impl TinyShell {
    pub(crate) fn show_quick_command_category_dialog(
        &mut self,
        category_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = category_id.as_deref().and_then(|category_id| {
            self.config
                .quick_command_categories()
                .and_then(|categories| categories.iter().find(|item| item.id == category_id))
                .cloned()
        });
        let input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).default_value(
                existing
                    .as_ref()
                    .map(|item| item.name.as_str())
                    .unwrap_or_default(),
            )
        });
        let view = cx.entity();
        let submit_input = input.clone();
        let focus_input = input.clone();
        self.open_dialog(
            crate::app::DialogKind::QuickCommandCategory,
            window,
            cx,
            move |dialog: Dialog, token, _window, _| {
                let submit_input = submit_input.clone();
                let content_input = input.clone();
                let existing = existing.clone();
                dialog
                    .title(t!("quick_command_category_dialog_title").to_string())
                    .w(px(420.))
                    .on_close({
                        let view = view.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.dialog_closed(token);
                                cx.notify();
                            });
                        }
                    })
                    .on_ok({
                        let view = view.clone();
                        move |_, window, cx| {
                            let name = submit_input.read(cx).value().trim().to_string();
                            if name.is_empty() {
                                view.update(cx, |this, cx| {
                                    this.status = t!("quick_command_category_name_required").into();
                                    cx.notify();
                                });
                                return false;
                            }
                            view.update(cx, |this, cx| {
                                let category =
                                    existing.clone().unwrap_or_else(|| QuickCommandCategory {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        name: String::new(),
                                        commands: Vec::new(),
                                    });
                                let category = QuickCommandCategory { name, ..category };
                                this.command_category_filter = Some(category.id.clone());
                                this.config.upsert_quick_command_category(category);
                                this.mark_config_preferences_dirty();
                                this.dismiss_dialog(token, window, cx);
                                cx.notify();
                            });
                            true
                        }
                    })
                    .content(move |content, _, cx| {
                        content.child(
                            v_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t!("quick_command_category_name")),
                                )
                                .child(Input::new(&content_input).w_full()),
                        )
                    })
            },
        );
        crate::app::input_focus::defer_focus_input_at_end(focus_input, window, cx);
    }

    fn confirm_quick_command_dialog(
        &mut self,
        source_category_id: String,
        existing: Option<QuickCommand>,
        inputs: &QuickCommandDialogInputs,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = inputs.name.read(cx).value().trim().to_string();
        let remark = inputs.remark.read(cx).value().trim().to_string();
        let command_text = inputs.command.read(cx).value().trim().to_string();
        if name.is_empty() || command_text.is_empty() {
            self.status = t!("quick_command_fields_required").into();
            cx.notify();
            return false;
        }
        let Some(category_id) = self.editing_quick_command_category.clone() else {
            self.status = t!("quick_command_category_name_required").into();
            cx.notify();
            return false;
        };
        let is_new_command = existing.is_none();
        let command = existing.unwrap_or_else(|| QuickCommand {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            remark: String::new(),
            command: String::new(),
        });
        let command_id = command.id.clone();
        let edited_command_was_selected = self
            .selected_quick_command
            .as_ref()
            .is_some_and(|(_, selected_command_id)| selected_command_id == &command_id);
        if source_category_id != category_id {
            self.config
                .remove_quick_command(&source_category_id, &command_id);
        }
        self.config.upsert_quick_command(
            &category_id,
            QuickCommand {
                name,
                remark,
                command: command_text,
                ..command
            },
        );
        self.command_category_filter = Some(category_id.clone());
        if is_new_command || edited_command_was_selected {
            self.selected_quick_command = Some((category_id, command_id));
        }
        self.mark_config_preferences_dirty();
        cx.notify();
        true
    }

    pub(crate) fn show_quick_command_dialog(
        &mut self,
        category_id: String,
        command_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let categories = self
            .config
            .quick_command_categories()
            .unwrap_or_default()
            .to_vec();
        let existing = self
            .config
            .quick_command_categories()
            .and_then(|categories| categories.iter().find(|item| item.id == category_id))
            .and_then(|category| {
                command_id.as_deref().and_then(|command_id| {
                    category.commands.iter().find(|item| item.id == command_id)
                })
            })
            .cloned();
        let name_input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).default_value(
                existing
                    .as_ref()
                    .map(|item| item.name.as_str())
                    .unwrap_or_default(),
            )
        });
        let remark_input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).default_value(
                existing
                    .as_ref()
                    .map(|item| item.remark.as_str())
                    .unwrap_or_default(),
            )
        });
        let command_input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx)
                .multi_line(true)
                .rows(12)
                .default_value(
                    existing
                        .as_ref()
                        .map(|item| item.command.as_str())
                        .unwrap_or_default(),
                )
        });
        let dialog_inputs = QuickCommandDialogInputs {
            name: name_input.clone(),
            remark: remark_input.clone(),
            command: command_input.clone(),
        };
        self.editing_quick_command_category = Some(category_id.clone());
        let view = cx.entity();
        let source_category_id = category_id.clone();
        let submit_inputs = dialog_inputs.clone();
        let focus_name_input = name_input.clone();
        self.open_dialog(crate::app::DialogKind::QuickCommand, window, cx, move |dialog: Dialog, token, _window, _| {
            let submit_inputs = submit_inputs.clone();
            let content_name = name_input.clone();
            let content_remark = remark_input.clone();
            let content_command = command_input.clone();
            let existing = existing.clone();
            let categories = categories.clone();
            let source_category_id = source_category_id.clone();
            dialog
                .title(t!("quick_command_dialog_title").to_string())
                .w(px(760.))
                .h(px(620.))
                .on_close({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.dialog_closed(token);
                            this.editing_quick_command_category = None;
                            cx.notify();
                        });
                    }
                })
                .on_ok({
                    let view = view.clone();
                    let submit_inputs = submit_inputs.clone();
                    let existing = existing.clone();
                    let source_category_id = source_category_id.clone();
                    move |_, _window, cx| {
                        view.update(cx, |this, cx| {
                            this.confirm_quick_command_dialog(
                                source_category_id.clone(),
                                existing.clone(),
                                &submit_inputs,
                                cx,
                            )
                        })
                    }
                })
                .content({
                    let view = view.clone();
                    move |content, window, cx| {
                        let selected_category = view
                            .read(cx)
                            .editing_quick_command_category
                            .clone();
                        let category_label = categories
                            .iter()
                            .find(|category| {
                                Some(category.id.as_str()) == selected_category.as_deref()
                            })
                            .map(|category| category.name.clone())
                            .unwrap_or_else(|| t!("quick_command_category_name").to_string());
                        content.child(
                        v_flex()
                            .size_full()
                            .gap_4()
                            .child(
                                h_flex()
                                    .gap_3()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t!("quick_command_name")),
                                            )
                                            .child(Input::new(&content_name).w_full()),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t!("quick_command_category_name")),
                                            )
                                            .child(
                                                Button::new("quick-command-category-picker")
                                                    .secondary()
                                                    .w_full()
                                                    .label(category_label)
                                                    .dropdown_menu_with_anchor(
                                                        Anchor::BottomLeft,
                                                        {
                                                            let view = view.clone();
                                                            let categories = categories.clone();
                                                            move |mut menu, window, cx| {
                                                                let selected = view
                                                                    .read(cx)
                                                                    .editing_quick_command_category
                                                                    .clone();
                                                                for category in &categories {
                                                                    let category_id =
                                                                        category.id.clone();
                                                                    menu = menu.item(
                                                                        PopupMenuItem::new(
                                                                            category.name.clone(),
                                                                        )
                                                                        .checked(
                                                                            selected.as_deref()
                                                                                == Some(
                                                                                    category.id
                                                                                        .as_str(),
                                                                                ),
                                                                        )
                                                                        .on_click(
                                                                            window.listener_for(
                                                                                &view,
                                                                                move |this,
                                                                                      _,
                                                                                      _,
                                                                                      cx| {
                                                                                    this.editing_quick_command_category = Some(category_id.clone());
                                                                                    cx.notify();
                                                                                },
                                                                            ),
                                                                        ),
                                                                    );
                                                                }
                                                                menu
                                                            }
                                                        },
                                                    ),
                                            ),
                                    )
                            )
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!("quick_command_remark")),
                                    )
                                    .child(Input::new(&content_remark).w_full()),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!("quick_command_content")),
                                    )
                                    .child(Input::new(&content_command).size_full()),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(t!("quick_command_insert_parameter")),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .children((1usize..=5).map(|index| {
                                                let command_input = content_command.clone();
                                                Button::new(("insert-command-parameter", index))
                                                    .small()
                                                    .secondary()
                                                    .label(format!("[p{index}]"))
                                                    .on_click(move |_, window, cx| {
                                                        let current = command_input
                                                            .read(cx)
                                                            .value()
                                                            .to_string();
                                                        command_input.update(cx, |input, cx| {
                                                            input.set_value(
                                                                format!("{current}[p{index}]"),
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    })
                                            }))
                                            .child(
                                                div()
                                                    .text_size(rems(0.75))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t!("quick_command_parameter_hint")),
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_none()
                                    .justify_end()
                                    .gap_2()
                                    .pt_2()
                                    .child(
                                        Button::new("quick-command-dialog-cancel")
                                            .secondary()
                                            .label(t!("cancel").to_string())
                                            .on_click(window.listener_for(
                                                &view,
                                                move |this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    this.dismiss_dialog(token, window, cx);
                                                },
                                            )),
                                    )
                                    .child(
                                        Button::new("quick-command-dialog-save")
                                            .primary()
                                            .label(t!("save").to_string())
                                            .on_click(window.listener_for(&view, {
                                                let source_category_id =
                                                    source_category_id.clone();
                                                let existing = existing.clone();
                                                let inputs = QuickCommandDialogInputs {
                                                    name: content_name.clone(),
                                                    remark: content_remark.clone(),
                                                    command: content_command.clone(),
                                                };
                                                move |this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    if this.confirm_quick_command_dialog(
                                                        source_category_id.clone(),
                                                        existing.clone(),
                                                        &inputs,
                                                        cx,
                                                    ) {
                                                        this.dismiss_dialog(token, window, cx);
                                                    }
                                                }
                                            })),
                                    ),
                            ),
                        )
                    }
                })
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_name_input, window, cx);
    }
}
