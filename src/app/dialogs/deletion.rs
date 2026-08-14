use super::*;

impl TinyShell {
    pub(crate) fn request_saved_session_deletion(
        &mut self,
        session_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session_name) = self
            .config
            .sessions()
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.name.clone())
        else {
            return;
        };

        let view = cx.entity();
        self.open_modal_dialog(
            crate::app::DialogKind::DeleteConfirmation,
            window,
            cx,
            move |dialog: Dialog, token, _window, _| {
                dialog
                    .title(t!("confirm_delete").to_string())
                    .w(px(420.))
                    .on_close({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.modal_dialog_closed(token, window, cx);
                                cx.notify();
                            });
                        }
                    })
                    .content({
                        let session_name = session_name.clone();
                        move |content, _window, _cx| {
                            content.child(
                                div().text_sm().child(
                                    t!("session_delete_confirm", name = session_name.clone())
                                        .to_string(),
                                ),
                            )
                        }
                    })
                    .footer(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("cancel-delete-saved-session")
                                    .ghost()
                                    .label(t!("cancel").to_string())
                                    .on_click({
                                        let view = view.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.dismiss_modal_dialog(token, window, cx);
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new(format!("confirm-delete-saved-session-{session_id}"))
                                    .danger()
                                    .label(t!("delete").to_string())
                                    .on_click({
                                        let view = view.clone();
                                        let session_id = session_id.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.remove_saved_session(session_id.clone(), cx);
                                                this.dismiss_modal_dialog(token, window, cx);
                                            });
                                        }
                                    }),
                            ),
                    )
            },
        );
    }

    pub(crate) fn request_managed_key_deletion(
        &mut self,
        key_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .config
            .sessions()
            .iter()
            .any(|session| session.managed_key_id.as_deref() == Some(key_id.as_str()))
        {
            self.delete_managed_key(key_id, cx);
            return;
        }

        let restore_selector = self.managed_key_dialog_token.is_some();
        let view = cx.entity();
        self.replace_modal_dialog(
            crate::app::DialogKind::DeleteConfirmation,
            window,
            cx,
            move |dialog: Dialog, token, _window, _| {
                dialog
                    .title(t!("confirm_delete").to_string())
                    .w(px(420.))
                    .close_button(false)
                    .overlay_closable(false)
                    .on_close({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.modal_dialog_closed(token, window, cx);
                                cx.notify();
                            });
                        }
                    })
                    .content(move |content, _window, _cx| {
                        content.child(div().text_sm().child(t!("key_delete_confirm").to_string()))
                    })
                    .footer(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("cancel-delete-managed-key")
                                    .ghost()
                                    .label(t!("cancel").to_string())
                                    .on_click({
                                        let view = view.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.dismiss_modal_dialog(token, window, cx);
                                            });
                                            if restore_selector {
                                                let view = view.clone();
                                                window.defer(cx, move |window, cx| {
                                                    view.update(cx, |this, cx| {
                                                        crate::managed_key_dialogs::show_managed_key_selector_dialog(
                                                            this, window, cx,
                                                        );
                                                    });
                                                });
                                            }
                                        }
                                    }),
                            )
                            .child(
                                Button::new(format!("confirm-delete-managed-key-{key_id}"))
                                    .danger()
                                    .label(t!("delete").to_string())
                                    .on_click({
                                        let view = view.clone();
                                        let key_id = key_id.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.delete_managed_key(key_id.clone(), cx);
                                                this.dismiss_modal_dialog(token, window, cx);
                                            });
                                            if restore_selector {
                                                let view = view.clone();
                                                window.defer(cx, move |window, cx| {
                                                    view.update(cx, |this, cx| {
                                                        crate::managed_key_dialogs::show_managed_key_selector_dialog(
                                                            this, window, cx,
                                                        );
                                                    });
                                                });
                                            }
                                        }
                                    }),
                            ),
                    )
            },
        );
    }

    pub(crate) fn show_delete_confirm_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let selected_entries = self
            .active_sftp()
            .map(|s| s.selected_entries.clone())
            .unwrap_or_default();
        if selected_entries.is_empty() {
            return;
        }

        let has_system_path = selected_entries.iter().any(|path| {
            let p = path.as_str();
            p.starts_with("/bin/")
                || p == "/bin"
                || p.starts_with("/etc/")
                || p == "/etc"
                || p.starts_with("/usr/")
                || p == "/usr"
                || p.starts_with("/var/")
                || p == "/var"
                || p.starts_with("/sys/")
                || p == "/sys"
                || p.starts_with("/dev/")
                || p == "/dev"
                || p.starts_with("/boot/")
                || p == "/boot"
                || p.starts_with("/lib/")
                || p == "/lib"
                || p.starts_with("/opt/")
                || p == "/opt"
                || p.starts_with("/run/")
                || p == "/run"
                || p.starts_with("/sbin/")
                || p == "/sbin"
        });

        self.open_modal_dialog(
            crate::app::DialogKind::DeleteConfirmation,
            window,
            cx,
            move |dialog: Dialog, token, _window, _| {
                dialog
                    .title(t!("confirm_delete").to_string())
                    .w(px(500.))
                    .on_close({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.modal_dialog_closed(token, window, cx);
                                cx.notify();
                            });
                        }
                    })
                    .keyboard(false)
                    .on_ok({
                        let view = view.clone();
                        let paths_to_delete: Vec<String> =
                            selected_entries.clone().into_iter().collect();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                if let Some(handle) = this.active_sftp_handle() {
                                    handle.send_command(crate::sftp::SftpCommand::DeletePaths(
                                        paths_to_delete.clone(),
                                    ));
                                }
                                if let Some(sftp) = this.active_sftp_mut() {
                                    sftp.selected_entries.clear();
                                }
                                cx.notify();
                            });
                            view.update(cx, |this, cx| {
                                this.dismiss_modal_dialog(token, window, cx);
                            });
                            true
                        }
                    })
                    .content({
                        let view = view.clone();
                        move |content, _window, cx| {
                            let scroll_handle =
                                view.read(cx).sftp_workspace.delete_scroll_handle.clone();
                            let selected_paths: Vec<String> = view
                                .read(cx)
                                .active_sftp()
                                .map(|s| s.selected_entries.clone().into_iter().collect())
                                .unwrap_or_default();

                            let warning_block = if has_system_path {
                                Some(
                                    div()
                                        .w_full()
                                        .p_3()
                                        .mb_3()
                                        .rounded_md()
                                        .bg(gpui::rgba(0xff00001a))
                                        .border_1()
                                        .border_color(gpui::rgba(0xff000080))
                                        .child(
                                            div()
                                                .text_color(gpui::rgba(0xff0000ff))
                                                .font_weight(FontWeight::BOLD)
                                                .child(t!("system_path_warning").to_string()),
                                        ),
                                )
                            } else {
                                None
                            };

                            let paths_list = div()
                                .relative()
                                .max_h(px(200.))
                                .w_full()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().background)
                                .rounded_md()
                                .child(
                                    v_flex()
                                        .id("delete-scroll-view")
                                        .size_full()
                                        .track_scroll(&scroll_handle)
                                        .overflow_y_scroll()
                                        .p_2()
                                        .gap_1()
                                        .children(selected_paths.into_iter().map(|path| {
                                            div()
                                                .text_size(rems(0.917))
                                                .text_color(cx.theme().muted_foreground)
                                                .child(path)
                                        })),
                                )
                                .child(
                                    div().absolute().top_0().bottom_0().right_0().child(
                                        gpui_component::scroll::Scrollbar::vertical(&scroll_handle)
                                            .scrollbar_show(
                                                gpui_component::scroll::ScrollbarShow::Always,
                                            ),
                                    ),
                                );

                            content.child(
                                v_flex()
                                    .w_full()
                                    .gap_2()
                                    .children(warning_block)
                                    .child(
                                        div().text_size(rems(1.0)).mb_2().child(
                                            t!(
                                                "confirm_delete_desc",
                                                count = view
                                                    .read(cx)
                                                    .active_sftp()
                                                    .map(|s| s.selected_entries.len())
                                                    .unwrap_or(0)
                                            )
                                            .to_string(),
                                        ),
                                    )
                                    .child(paths_list),
                            )
                        }
                    })
                    .footer({
                        let view = view.clone();
                        let cancel_view = view.clone();
                        let paths_to_delete: Vec<String> =
                            selected_entries.clone().into_iter().collect();
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("cancel")
                                    .ghost()
                                    .label(t!("cancel").to_string())
                                    .on_click(move |_, window, cx| {
                                        cancel_view.update(cx, |this, cx| {
                                            this.dismiss_modal_dialog(token, window, cx);
                                        });
                                    }),
                            )
                            .child(
                                Button::new("confirm")
                                    .danger()
                                    .label(t!("confirm").to_string())
                                    .on_click({
                                        let view = view.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                if let Some(handle) = this.active_sftp_handle() {
                                                    handle.send_command(
                                                        crate::sftp::SftpCommand::DeletePaths(
                                                            paths_to_delete.clone(),
                                                        ),
                                                    );
                                                }
                                                if let Some(sftp) = this.active_sftp_mut() {
                                                    sftp.selected_entries.clear();
                                                }
                                                cx.notify();
                                            });
                                            view.update(cx, |this, cx| {
                                                this.dismiss_modal_dialog(token, window, cx);
                                            });
                                        }
                                    }),
                            )
                    })
            },
        );
    }
}
