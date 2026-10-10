use super::*;
use crate::app::confirmation_dialog::ConfirmationDialog;

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
            move |dialog: Dialog, token, window, cx| {
                ConfirmationDialog::new(
                    t!("confirm_delete").to_string(),
                    t!("session_delete_confirm", name = session_name.clone()).to_string(),
                )
                .danger(true)
                .confirm_label(t!("delete").to_string())
                .on_close({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.modal_dialog_closed(token, window, cx);
                            cx.notify();
                        });
                    }
                })
                .on_ok({
                    let view = view.clone();
                    let session_id = session_id.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.remove_saved_session(session_id.clone(), cx)
                        });
                        true
                    }
                })
                .build(dialog, window, cx)
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
            move |dialog: Dialog, token, window, cx| {
                ConfirmationDialog::new(
                    t!("confirm_delete").to_string(),
                    t!("key_delete_confirm").to_string(),
                )
                .danger(true)
                .confirm_label(t!("delete").to_string())
                .on_close({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.modal_dialog_closed(token, window, cx);
                            cx.notify();
                        });
                        if restore_selector {
                            let view = view.clone();
                            window.defer(cx, move |window, cx| {
                                view.update(cx, |this, cx| {
                                    crate::managed_key_dialogs::show_managed_key_selector_dialog(
                                        this, window, cx,
                                    )
                                });
                            });
                        }
                    }
                })
                .on_ok({
                    let view = view.clone();
                    let key_id = key_id.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| this.delete_managed_key(key_id.clone(), cx));
                        true
                    }
                })
                .build(dialog, window, cx)
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

        let mut description = t!("confirm_delete_desc", count = selected_entries.len()).to_string();
        if has_system_path {
            description.push_str(&format!("\n\n{}", t!("system_path_warning")));
        }
        let paths: Vec<String> = selected_entries.into_iter().collect();
        description.push_str(&format!("\n\n{}", paths.join("\n")));
        self.open_modal_dialog(
            crate::app::DialogKind::DeleteConfirmation,
            window,
            cx,
            move |dialog: Dialog, token, window, cx| {
                ConfirmationDialog::new(t!("confirm_delete").to_string(), description.clone())
                    .danger(true)
                    .keyboard(false)
                    .confirm_label(t!("delete").to_string())
                    .on_close({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.modal_dialog_closed(token, window, cx);
                                cx.notify();
                            });
                        }
                    })
                    .on_ok({
                        let view = view.clone();
                        let paths = paths.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                if let Some(handle) = this.active_sftp_handle() {
                                    handle.send_command(crate::sftp::SftpCommand::DeletePaths(
                                        paths.clone(),
                                    ));
                                }
                                if let Some(sftp) = this.active_sftp_mut() {
                                    sftp.selected_entries.clear();
                                }
                                cx.notify();
                            });
                            true
                        }
                    })
                    .build(dialog, window, cx)
            },
        );
    }
}
