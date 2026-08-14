use super::*;

impl TinyShell {
    pub(crate) fn show_ssh_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

        self.open_modal_dialog(crate::app::DialogKind::NewSsh, window, cx, move |dialog: Dialog, token, _window, _cx| {
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
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.modal_dialog_closed(token, window, cx);
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
                                                    move |this, _, _, cx| {
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
                                                    move |this, _, _, cx| {
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
                                                    move |this, _, _, cx| {
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
                                                                .on_click(window.listener_for(&view, move |this, _, _, cx| {
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
                                                    move |this, _, window, cx| {
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
                                                    move |this, _, window, cx| {
                                                        this.dismiss_modal_dialog(token, window, cx);
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
}
