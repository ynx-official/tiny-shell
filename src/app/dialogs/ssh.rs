use super::*;

impl TinyShell {
    pub(crate) fn show_ssh_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_dialog.is_some() {
            return;
        }
        self.active_dialog = Some(crate::app::DialogKind::NewSsh);

        let view = cx.entity();
        let session_name_input = self.connection_inputs.session_name_input.clone();
        let host_input = self.connection_inputs.host_input.clone();
        let focus_host_input = host_input.clone();
        let port_input = self.connection_inputs.port_input.clone();
        let user_input = self.connection_inputs.user_input.clone();
        let password_input = self.connection_inputs.password_input.clone();
        let key_path_input = self.connection_inputs.key_path_input.clone();
        let key_inline_input = self.connection_inputs.key_inline_input.clone();
        let passphrase_input = self.connection_inputs.passphrase_input.clone();
        let proxy_host_input = self.connection_inputs.proxy_host_input.clone();
        let proxy_port_input = self.connection_inputs.proxy_port_input.clone();
        let proxy_user_input = self.connection_inputs.proxy_user_input.clone();
        let proxy_password_input = self.connection_inputs.proxy_password_input.clone();

        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            dialog
                .title(t!("new_ssh_connection"))
                .w(px(520.))
                .overlay_closable(true)
                .on_ok({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.connect_ssh(window, cx);
                        });
                        false
                    }
                })
                .on_close({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.active_dialog = None;
                            this.key_import.close();
                            cx.notify();
                        });
                    }
                })
                .content({
                    let view = view.clone();
                    let session_name_input = session_name_input.clone();
                    let host_input = host_input.clone();
                    let port_input = port_input.clone();
                    let user_input = user_input.clone();
                    let password_input = password_input.clone();
                    let key_path_input = key_path_input.clone();
                    let key_inline_input = key_inline_input.clone();
                    let passphrase_input = passphrase_input.clone();
                    let proxy_host_input = proxy_host_input.clone();
                    let proxy_port_input = proxy_port_input.clone();
                    let proxy_user_input = proxy_user_input.clone();
                    let proxy_password_input = proxy_password_input.clone();
                    move |content, window, cx| {
                        let auth_method = view.read(cx).ssh_auth_method;
                        let is_password = auth_method == AuthMethod::Password;
                        let is_key = matches!(auth_method, AuthMethod::Key | AuthMethod::KeyPending);
                        let is_config = auth_method == AuthMethod::Config;
                        let is_editing = view.read(cx).editing_session_id.is_some();
                        let proxy_type = view.read(cx).ssh_proxy_type.clone();
                        let show_proxy_fields = proxy_type != "none";
                        let key_auth_incomplete = is_key
                            && !view.read(cx).using_custom_key_path
                            && view.read(cx).managed_key_selected.is_none();
                        let connection_groups = view.read(cx).config.connection_groups().to_vec();
                        let selected_group = view.read(cx).session_group_selection.clone();
                        let group_label = selected_group
                            .clone()
                            .unwrap_or_else(|| t!("connection_group_ungrouped").to_string());
                        content.child(
                            v_flex()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Button::new("ssh-auth-password")
                                                .label(t!("password").to_string())
                                                .when(is_password, |button| button.primary())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.set_ssh_auth_method(
                                                            AuthMethod::Password,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("ssh-auth-key")
                                                .label(t!("key").to_string())
                                                .when(is_key, |button| button.primary())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.set_ssh_auth_method(
                                                            AuthMethod::Key,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("ssh-auth-config")
                                                .label(t!("ssh_config").to_string())
                                                .when(is_config, |button| button.primary())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.set_ssh_auth_method(
                                                            AuthMethod::Config,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        ),
                                )
                                .when(!is_config, |this| {
                                    this.child(Input::new(&session_name_input).tab_index(0))
                                        .child(Input::new(&host_input).tab_index(1))
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .child(
                                                    Input::new(&port_input).w(px(96.)).tab_index(2),
                                                )
                                                .child(
                                                    Input::new(&user_input).flex_1().tab_index(3),
                                        ),
                                        )
                                        .child(
                                            Button::new("ssh-session-group")
                                                .secondary()
                                                .w_full()
                                                .label(format!("{}: {}", t!("connection_group"), group_label))
                                                .dropdown_menu_with_anchor(Anchor::BottomLeft, {
                                                    let view = view.clone();
                                                    move |mut menu, window, cx| {
                                                        let selected = view.read(cx).session_group_selection.clone();
                                                        menu = menu.item(
                                                            PopupMenuItem::new(t!("connection_group_ungrouped").to_string())
                                                                .checked(selected.is_none())
                                                                .on_click(window.listener_for(&view, |this, _, _, cx| {
                                                                    this.session_group_selection = None;
                                                                    cx.notify();
                                                                })),
                                                        );
                                                        for group in connection_groups.clone() {
                                                            let group_name = group.clone();
                                                            let checked = selected.as_deref() == Some(group.as_str());
                                                            menu = menu.item(
                                                                PopupMenuItem::new(group)
                                                                    .checked(checked)
                                                                    .on_click(window.listener_for(&view, move |this, _, _, cx| {
                                                                        this.session_group_selection = Some(group_name.clone());
                                                                        cx.notify();
                                                                    })),
                                                            );
                                                        }
                                                        menu
                                                    }
                                                }),
                                        )
                                })
                                .when(is_password, |this| {
                                    this.child(
                                        Input::new(&password_input).mask_toggle().tab_index(4),
                                    )
                                })
                                .when(is_key, |this| {
                                    let managed_keys = view.read(cx).managed_keys.clone();
                                    let managed_key_selected =
                                        view.read(cx).managed_key_selected.clone();
                                    let using_custom_key_path = view.read(cx).using_custom_key_path;
                                    let theme = cx.theme();
                                    let using_managed_key = !using_custom_key_path;

                                    let key_label = if let Some(mk_id) = &managed_key_selected {
                                        managed_keys
                                            .iter()
                                            .find(|k| &k.id == mk_id)
                                            .map(|mk| format!("{} ({})", mk.name, mk.key_type))
                                            .unwrap_or_else(|| t!("select_managed_key").to_string())
                                    } else {
                                        t!("select_managed_key").to_string()
                                    };

                                    let this =
                                        this.child(
                                            h_flex()
                                                .w_full()
                                                .gap_1()
                                                .p(px(3.))
                                                .rounded_md()
                                                .border_1()
                                                .border_color(theme.border)
                                                .bg(theme.muted)
                                                .child(
                                                    Button::new("use-managed-key")
                                                        .small()
                                                        .flex_1()
                                                        .when(using_managed_key, |button| {
                                                            button.primary()
                                                        })
                                                        .label(t!("select_managed_key").to_string())
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            |this, _, _, cx| {
                                                                this.use_managed_key(cx)
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    Button::new("use-custom-path")
                                                        .small()
                                                        .flex_1()
                                                        .when(using_custom_key_path, |button| {
                                                            button.primary()
                                                        })
                                                        .label(t!("use_custom_path").to_string())
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            |this, _, _, cx| {
                                                                this.use_custom_key_path(cx)
                                                            },
                                                        )),
                                                ),
                                        );

                                    if using_managed_key {
                                        let this = this.child(
                                            Button::new("managed-key-select")
                                                .small()
                                                .w_full()
                                                .label(key_label)
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.open_managed_key_selector(window, cx);
                                                    },
                                                )),
                                        );
                                        let info = managed_key_selected
                                            .as_ref()
                                            .and_then(|id| {
                                                managed_keys.iter().find(|k| &k.id == id)
                                            })
                                            .map(|mk| {
                                                format!(
                                                    "{}: {}  |  {}: {}",
                                                    t!("key_type"),
                                                    mk.key_type,
                                                    t!("key_fingerprint"),
                                                    mk.fingerprint
                                                )
                                            });

                                        if let Some(info) = info {
                                            this.child(
                                                div()
                                                    .px(px(8.))
                                                    .py(px(6.))
                                                    .rounded_md()
                                                    .border_1()
                                                    .border_color(theme.border)
                                                    .text_xs()
                                                    .text_color(theme.muted_foreground)
                                                    .child(info),
                                            )
                                        } else {
                                            this.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(theme.muted_foreground)
                                                    .child(
                                                        t!("select_managed_key_hint").to_string(),
                                                    ),
                                            )
                                        }
                                    } else {
                                        this.child(
                                            h_flex()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .cursor_pointer()
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            window.listener_for(
                                                                &view,
                                                                |this, _, window, cx| {
                                                                    this.pick_ssh_key_path(
                                                                        window, cx,
                                                                    );
                                                                },
                                                            ),
                                                        )
                                                        .child(
                                                            Input::new(&key_path_input)
                                                                .tab_index(4),
                                                        ),
                                                )
                                                .child(
                                                    Button::new("clear-key-path")
                                                        .ghost()
                                                        .icon(IconName::Close)
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            |this, _, window, cx| {
                                                                Self::set_input_value(
                                                    &this.connection_inputs.key_path_input,
                                                                    "",
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                ),
                                        )
                                        .child(
                                            Input::new(&key_inline_input).h(px(128.)).tab_index(5),
                                        )
                                        .child(
                                            Input::new(&passphrase_input)
                                                .mask_toggle()
                                                .tab_index(6),
                                        )
                                    }
                                })
                                .when(is_config, |this| {
                                    let entries = view.read(cx).ssh_config_entries.clone();
                                    let selected = view.read(cx).ssh_config_selected;
                                    let theme = cx.theme();
                                    if entries.is_empty() {
                                        this.child(
                                            div()
                                                .text_sm()
                                                .text_color(theme.muted_foreground)
                                                .child(t!("ssh_config_empty").to_string()),
                                        )
                                    } else {
                                        this.child(
                                            div()
                                                .h(px(192.))
                                                .id("ssh-config-list")
                                                .track_scroll(
                                                    &view.read(cx).connection_scroll_handle,
                                                )
                                                .overflow_y_scroll()
                                                .border_1()
                                                .border_color(theme.border)
                                                .rounded_md()
                                                .children(entries.iter().enumerate().map(
                                                    |(i, entry)| {
                                                        let is_selected = selected == Some(i);
                                                        let label = if entry.user.is_empty() {
                                                            format!(
                                                                "{}:{}",
                                                                entry.hostname, entry.port
                                                            )
                                                        } else {
                                                            format!(
                                                                "{}@{}:{}",
                                                                entry.user,
                                                                entry.hostname,
                                                                entry.port
                                                            )
                                                        };
                                                        let alias_label =
                                                            if entry.host_alias == entry.hostname {
                                                                String::new()
                                                            } else {
                                                                format!(" ({})", entry.host_alias)
                                                            };
                                                        let view_clone = view.clone();
                                                        div()
                                                            .id(("ssh-config-entry", i))
                                                            .px_2()
                                                            .py_1()
                                                            .when(is_selected, |el| {
                                                                el.bg(theme.selection)
                                                            })
                                                            .cursor_pointer()
                                                            .hover(|el| el.bg(theme.selection))
                                                            .text_sm()
                                                            .child(format!("{label}{alias_label}"))
                                                            .on_click(window.listener_for(
                                                                &view_clone,
                                                                move |this, _, window, cx| {
                                                                    this.select_ssh_config_entry(
                                                                        i, window, cx,
                                                                    );
                                                                },
                                                            ))
                                                    },
                                                )),
                                        )
                                    }
                                })
                                .when(!is_config, |this| {
                                    this.child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::BOLD)
                                            .child(t!("proxy").to_string()),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                Button::new("proxy-none")
                                                    .label(t!("proxy_none").to_string())
                                                    .when(proxy_type == "none", |button| {
                                                        button.primary()
                                                    })
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.set_ssh_proxy_type(
                                                                "none".to_string(),
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("proxy-socks5")
                                                    .label("SOCKS5")
                                                    .when(proxy_type == "socks5", |button| {
                                                        button.primary()
                                                    })
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.set_ssh_proxy_type(
                                                                "socks5".to_string(),
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("proxy-http")
                                                    .label("HTTP")
                                                    .when(proxy_type == "http", |button| {
                                                        button.primary()
                                                    })
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.set_ssh_proxy_type(
                                                                "http".to_string(),
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            ),
                                    )
                                    .when(
                                        show_proxy_fields,
                                        |this| {
                                            this.child(
                                                h_flex()
                                                    .gap_2()
                                                    .child(Input::new(&proxy_host_input).flex_1())
                                                    .child(
                                                        Input::new(&proxy_port_input).w(px(96.)),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .child(Input::new(&proxy_user_input).flex_1())
                                                    .child(
                                                        Input::new(&proxy_password_input).flex_1(),
                                                    ),
                                            )
                                        },
                                    )
                                })
                                .child(
                                    h_flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new("connect-ssh-cancel")
                                                .label(t!("cancel").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.active_dialog = None;
                                                        window.close_dialog(cx);
                                                        cx.notify();
                                                    },
                                                )),
                                        )
                                        .when(!is_config, |this| {
                                            this.child(
                                                Button::new("connect-ssh-confirm")
                                                    .primary()
                                                    .disabled(key_auth_incomplete)
                                                    .label(if is_editing {
                                                        t!("save")
                                                    } else {
                                                        t!("connect")
                                                    })
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, window, cx| {
                                                            this.connect_ssh(window, cx)
                                                        },
                                                    )),
                                            )
                                        }),
                                ),
                        )
                    }
                })
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_host_input, window, cx);
    }
    pub(crate) fn show_managed_key_selector_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_dialog.is_some() {
            return;
        }
        // The selector may be opened from a separate credential window; refresh
        // the cached list from the canonical config before rendering its rows.
        self.managed_keys = self.config.managed_keys().to_vec();
        self.active_dialog = Some(crate::app::DialogKind::ManagedKeySelector);

        let view = cx.entity();
        let rename_input = self.connection_inputs.key_import_remark_input.clone();
        window.open_dialog(cx, move |dialog: Dialog, window, _cx| {
            let dialog_width = px(760.).min(window.viewport_size().width - px(24.));
            dialog
                .title(t!("select_private_key").to_string())
                .w(dialog_width)
                .close_button(false)
                .overlay_closable(false)
                .on_ok({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            if this.managed_key_dialog_selection.is_some() {
                                this.confirm_managed_key_selection(window, cx);
                            }
                        });
                        false
                    }
                })
                .content({
                    let view = view.clone();
                    let rename_input = rename_input.clone();
                    move |content, window, cx| {
                        let keys = view.read(cx).managed_keys.clone();
                        let selected = view.read(cx).managed_key_dialog_selection.clone();
                        let is_renaming = view.read(cx).editing_managed_key_id.is_some();
                        let has_selection = selected.is_some();

                        let mut rows = v_flex()
                            .h(px(190.))
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_md()
                            .overflow_hidden()
                            .child(
                                h_flex()
                                    .px_2()
                                    .py_1()
                                    .bg(cx.theme().muted)
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div()
                                            .w(px(150.))
                                            .flex_shrink_0()
                                            .text_sm()
                                            .child(t!("name").to_string()),
                                    )
                                    .child(
                                        div()
                                            .w(px(80.))
                                            .flex_shrink_0()
                                            .text_sm()
                                            .child(t!("key_type").to_string()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .text_sm()
                                            .child(t!("key_fingerprint").to_string()),
                                    ),
                            );

                        if keys.is_empty() {
                            rows = rows.child(
                                div()
                                    .flex_1()
                                    .items_center()
                                    .justify_center()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("no_managed_keys").to_string()),
                            );
                        } else {
                            for (index, key) in keys.into_iter().enumerate() {
                                let key_id = key.id.clone();
                                let is_selected = selected.as_deref() == Some(key.id.as_str());
                                let fingerprint = if key.fingerprint.len() > 24 {
                                    format!("{}…", &key.fingerprint[..24])
                                } else {
                                    key.fingerprint.clone()
                                };
                                rows = rows.child(
                                    h_flex()
                                        .id(("managed-key-choice", index))
                                        .px_2()
                                        .py_2()
                                        .cursor_pointer()
                                        .border_b_1()
                                        .border_color(cx.theme().border)
                                        .when(is_selected, |row| row.bg(cx.theme().selection))
                                        .hover(|row| row.bg(cx.theme().selection))
                                        .child(
                                            div()
                                                .w(px(150.))
                                                .flex_shrink_0()
                                                .min_w(px(0.))
                                                .overflow_hidden()
                                                .text_sm()
                                                .child(key.name),
                                        )
                                        .child(
                                            div()
                                                .w(px(80.))
                                                .flex_shrink_0()
                                                .text_sm()
                                                .child(key.key_type),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .overflow_hidden()
                                                .text_sm()
                                                .child(fingerprint),
                                        )
                                        .on_click(window.listener_for(
                                            &view,
                                            move |this, _, _, cx| {
                                                this.select_managed_key_candidate(
                                                    key_id.clone(),
                                                    cx,
                                                );
                                            },
                                        )),
                                );
                            }
                        }

                        content.child(
                            v_flex()
                                .gap_3()
                                .child(
                                    h_flex().gap_3().child(rows.flex_1()).child(
                                        v_flex()
                                            .w(px(104.))
                                            .gap_2()
                                            .child(
                                                Button::new("selector-import-key")
                                                    .w_full()
                                                    .label(t!("import_key").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, window, cx| {
                                                            this.open_key_import(window, cx);
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("selector-edit-key")
                                                    .w_full()
                                                    .disabled(!has_selection)
                                                    .label(t!("edit").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, window, cx| {
                                                            this.begin_managed_key_rename(
                                                                window, cx,
                                                            );
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("selector-delete-key")
                                                    .w_full()
                                                    .disabled(!has_selection)
                                                    .label(t!("delete").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, window, cx| {
                                                            this.delete_selected_managed_key(
                                                                window, cx,
                                                            );
                                                        },
                                                    )),
                                            ),
                                    ),
                                )
                                .when(is_renaming, |this| {
                                    this.child(
                                        h_flex()
                                            .gap_2()
                                            .child(Input::new(&rename_input).flex_1())
                                            .child(
                                                Button::new("save-key-rename")
                                                    .primary()
                                                    .label(t!("save").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.save_managed_key_rename(cx);
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("cancel-key-rename")
                                                    .label(t!("cancel").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.cancel_managed_key_rename(cx);
                                                        },
                                                    )),
                                            ),
                                    )
                                })
                                .child(
                                    h_flex()
                                        .justify_center()
                                        .gap_2()
                                        .child(
                                            Button::new("confirm-key-selection")
                                                .primary()
                                                .disabled(!has_selection)
                                                .label(t!("confirm").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.confirm_managed_key_selection(
                                                            window, cx,
                                                        );
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("cancel-key-selection")
                                                .label(t!("cancel").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.return_to_ssh_dialog(window, cx);
                                                    },
                                                )),
                                        ),
                                ),
                        )
                    }
                })
        });
    }

    pub(crate) fn show_managed_key_import_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_dialog.is_some() {
            return;
        }
        self.active_dialog = Some(crate::app::DialogKind::ManagedKeyImport);

        let view = cx.entity();
        let remark_input = self.connection_inputs.key_import_remark_input.clone();
        let passphrase_input = self.connection_inputs.key_import_passphrase_input.clone();
        let focus_remark_input = remark_input.clone();
        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            dialog
                .title(t!("key_import_dialog_title").to_string())
                .w(px(440.))
                .close_button(false)
                .overlay_closable(false)
                .on_ok({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.confirm_managed_key_import(window, cx);
                        });
                        false
                    }
                })
                .content({
                    let view = view.clone();
                    let remark_input = remark_input.clone();
                    let passphrase_input = passphrase_input.clone();
                    move |content, window, cx| {
                        let path = view.read(cx).key_import.path.clone();
                        let validation = view.read(cx).key_import.validation.clone();
                        let can_confirm = validation.can_confirm();
                        let (status, status_color) = match &validation {
                            KeyImportValidation::WaitingForFile => (
                                t!("key_import_select_file_hint").to_string(),
                                cx.theme().muted_foreground,
                            ),
                            KeyImportValidation::Validating => (
                                t!("key_import_validating").to_string(),
                                cx.theme().muted_foreground,
                            ),
                            KeyImportValidation::Invalid(error) => (
                                format!("{}: {error}", t!("key_import_failed")),
                                cx.theme().danger,
                            ),
                            KeyImportValidation::Duplicate => (
                                t!("key_duplicate_fingerprint").to_string(),
                                cx.theme().danger,
                            ),
                            KeyImportValidation::Valid {
                                key_type,
                                fingerprint,
                            } => (
                                format!(
                                    "{} · {}: {}",
                                    key_type,
                                    t!("key_fingerprint"),
                                    fingerprint
                                ),
                                cx.theme().success,
                            ),
                        };

                        content.child(
                            v_flex()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .child(
                                            div()
                                                .w(px(80.))
                                                .text_sm()
                                                .child(t!("name").to_string()),
                                        )
                                        .child(Input::new(&remark_input).flex_1()),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .w(px(80.))
                                                .text_sm()
                                                .child(t!("private_key").to_string()),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .px_3()
                                                .py_2()
                                                .rounded_md()
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .text_sm()
                                                .overflow_hidden()
                                                .text_color(if path.is_empty() {
                                                    cx.theme().muted_foreground
                                                } else {
                                                    cx.theme().foreground
                                                })
                                                .child(if path.is_empty() {
                                                    t!("key_import_choose_file").to_string()
                                                } else {
                                                    path
                                                }),
                                        )
                                        .child(
                                            Button::new("browse-key-import")
                                                .label(t!("browse").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.pick_managed_key_import_file(
                                                            window, cx,
                                                        );
                                                    },
                                                )),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .child(
                                            div()
                                                .w(px(80.))
                                                .text_sm()
                                                .child(t!("key_passphrase").to_string()),
                                        )
                                        .child(
                                            Input::new(&passphrase_input).flex_1().mask_toggle(),
                                        ),
                                )
                                .child(
                                    div()
                                        .pl(px(80.))
                                        .text_xs()
                                        .text_color(status_color)
                                        .child(status),
                                )
                                .child(
                                    h_flex()
                                        .justify_center()
                                        .gap_2()
                                        .child(
                                            Button::new("confirm-key-import")
                                                .primary()
                                                .disabled(!can_confirm)
                                                .label(t!("confirm").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.confirm_managed_key_import(window, cx);
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("cancel-key-import")
                                                .label(t!("cancel").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.close_key_import(window, cx);
                                                    },
                                                )),
                                        ),
                                ),
                        )
                    }
                })
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_remark_input, window, cx);
    }
}
