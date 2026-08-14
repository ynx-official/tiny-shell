use gpui::{
    AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, rems,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::Dialog,
    h_flex,
    input::{Input, InputEvent, InputState},
    radio::{Radio, RadioGroup},
    switch::Switch,
    v_flex,
};
use rust_i18n::t;

use crate::TinyShell;

struct SftpPermissionsForm {
    remote_path: String,
    is_dir: bool,
    mode: u32,
    recursive: bool,
    apply_to: crate::sftp::PermissionApplyTarget,
    input: Entity<InputState>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl SftpPermissionsForm {
    fn new(
        remote_path: String,
        is_dir: bool,
        mode: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format!("{:03o}", mode & 0o7777))
                .placeholder(t!("sftp_permissions_placeholder").to_string())
        });
        let subscriptions = vec![cx.subscribe_in(&input, window, Self::on_input_event)];
        Self {
            remote_path,
            is_dir,
            mode: mode & 0o7777,
            recursive: false,
            apply_to: crate::sftp::PermissionApplyTarget::FilesAndDirectories,
            input,
            _subscriptions: subscriptions,
        }
    }

    fn on_input_event(
        &mut self,
        input: &Entity<InputState>,
        _: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = input.read(cx).value().trim().to_string();
        if (3..=4).contains(&value.len())
            && let Ok(mode) = u32::from_str_radix(&value, 8)
            && mode <= 0o7777
        {
            self.mode = mode;
            cx.notify();
        }
    }

    fn permission_checkbox(&self, id: &'static str, bit: u32, cx: &mut Context<Self>) -> Checkbox {
        Checkbox::new(id)
            .checked(self.mode & bit != 0)
            .on_click(cx.listener(move |this, checked, window, cx| {
                if *checked {
                    this.mode |= bit;
                } else {
                    this.mode &= !bit;
                }
                let value = format!("{:03o}", this.mode & 0o7777);
                this.input.update(cx, |input, cx| {
                    input.set_value(value, window, cx);
                });
                cx.notify();
            }))
    }
}

impl Render for SftpPermissionsForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self
            .remote_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("/")
            .to_string();
        let parent = TinyShell::sftp_parent_path(&self.remote_path);
        let current_mode = format!("{:03o}", self.mode & 0o7777);
        let recursive = self.recursive;
        let selected_target = match self.apply_to {
            crate::sftp::PermissionApplyTarget::FilesAndDirectories => 0,
            crate::sftp::PermissionApplyTarget::FilesOnly => 1,
            crate::sftp::PermissionApplyTarget::DirectoriesOnly => 2,
        };

        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .gap_3()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .size(px(42.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .bg(cx.theme().muted)
                            .child(
                                Icon::new(if self.is_dir {
                                    IconName::Folder
                                } else {
                                    IconName::File
                                })
                                .with_size(Size::Medium),
                            ),
                    )
                    .child(
                        v_flex()
                            .min_w(px(0.))
                            .gap_1()
                            .child(div().font_weight(FontWeight::BOLD).child(name))
                            .child(
                                div()
                                    .text_size(rems(0.833))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(parent),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(rems(0.833))
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("sftp_permission_matrix")),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(
                                h_flex()
                                    .pb_1()
                                    .child(div().flex_1())
                                    .child(div().w(px(56.)).child(t!("sftp_permission_read")))
                                    .child(div().w(px(56.)).child(t!("sftp_permission_write")))
                                    .child(div().w(px(56.)).child(t!("sftp_permission_execute"))),
                            )
                            .child(
                                h_flex()
                                    .h(px(36.))
                                    .px_2()
                                    .rounded_md()
                                    .bg(cx.theme().muted.opacity(0.7))
                                    .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(t!("sftp_permission_owner")))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-owner-read", 0o400, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-owner-write", 0o200, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-owner-execute", 0o100, cx))),
                            )
                            .child(
                                h_flex()
                                    .h(px(36.))
                                    .px_2()
                                    .rounded_md()
                                    .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(t!("sftp_permission_group")))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-group-read", 0o040, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-group-write", 0o020, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-group-execute", 0o010, cx))),
                            )
                            .child(
                                h_flex()
                                    .h(px(36.))
                                    .px_2()
                                    .rounded_md()
                                    .bg(cx.theme().muted.opacity(0.7))
                                    .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(t!("sftp_permission_other")))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-other-read", 0o004, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-other-write", 0o002, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-other-execute", 0o001, cx))),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(rems(0.833))
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("sftp_permission_advanced")),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(
                                v_flex()
                                    .flex_1()
                                    .gap_1()
                                    .child(div().font_weight(FontWeight::MEDIUM).child(t!("sftp_permission_octal")))
                                    .child(div().text_size(rems(0.8)).text_color(cx.theme().muted_foreground).child(t!("sftp_permissions_hint"))),
                            )
                            .child(Input::new(&self.input).w(px(100.)))
                            .child(
                                div()
                                    .min_w(px(52.))
                                    .text_center()
                                    .font_weight(FontWeight::BOLD)
                                    .child(current_mode),
                            ),
                    ),
            )
            .when(self.is_dir, |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(cx.theme().border)
                        .child(
                            h_flex()
                                .items_center()
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .gap_1()
                                        .child(div().font_weight(FontWeight::MEDIUM).child(t!("sftp_permission_recursive")))
                                        .child(div().text_size(rems(0.8)).text_color(cx.theme().muted_foreground).child(t!("sftp_permission_recursive_hint"))),
                                )
                                .child(
                                    Switch::new("permission-recursive")
                                        .checked(recursive)
                                        .on_click(cx.listener(|this, checked, _, cx| {
                                            this.recursive = *checked;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .when(recursive, |this| {
                            this.child(
                                RadioGroup::vertical("permission-recursive-target")
                                    .selected_index(Some(selected_target))
                                    .on_click(cx.listener(|this, index, _, cx| {
                                        this.apply_to = match *index {
                                            1 => crate::sftp::PermissionApplyTarget::FilesOnly,
                                            2 => crate::sftp::PermissionApplyTarget::DirectoriesOnly,
                                            _ => crate::sftp::PermissionApplyTarget::FilesAndDirectories,
                                        };
                                        cx.notify();
                                    }))
                                    .child(Radio::new("permission-all").label(t!("sftp_permission_apply_all").to_string()))
                                    .child(Radio::new("permission-files").label(t!("sftp_permission_apply_files").to_string()))
                                    .child(Radio::new("permission-directories").label(t!("sftp_permission_apply_directories").to_string())),
                            )
                        }),
                )
            })
    }
}

impl TinyShell {
    fn apply_sftp_create_input(
        &mut self,
        input: &Entity<InputState>,
        base_path: &str,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = input.read(cx).value().trim().to_string();
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            self.status = t!("sftp_invalid_name").into();
            cx.notify();
            return false;
        }
        if let Some(handle) = self.active_sftp_handle() {
            let path = crate::sftp::join_remote(base_path, &name);
            let command = if is_dir {
                crate::sftp::SftpCommand::CreateDir(path)
            } else {
                crate::sftp::SftpCommand::CreateFile(path)
            };
            handle.send_command(command);
        }
        cx.notify();
        true
    }

    pub(crate) fn show_sftp_create_dialog(
        &mut self,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let base_path = self
            .active_sftp()
            .map(|sftp| sftp.current_path.clone())
            .unwrap_or_else(|| "/".to_string());
        let input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).placeholder(if is_dir {
                t!("sftp_new_folder_name").to_string()
            } else {
                t!("sftp_new_file_name").to_string()
            })
        });
        let submit_input = input.clone();
        let focus_input = input.clone();
        self.open_modal_dialog(
            crate::app::DialogKind::QuickCommandCategory,
            window,
            cx,
            move |dialog: Dialog, token, window, _| {
                let view = view.clone();
                let on_close_view = view.clone();
                let submit_input = submit_input.clone();
                let content_input = input.clone();
                let confirm_input = input.clone();
                let confirm_base_path = base_path.clone();
                dialog
                    .title(if is_dir {
                        t!("sftp_new_folder").to_string()
                    } else {
                        t!("sftp_new_file").to_string()
                    })
                    .w(px(420.))
                    .on_close(move |_, window, cx| {
                        on_close_view.update(cx, |this, cx| {
                            this.modal_dialog_closed(token, window, cx);
                            cx.notify();
                        });
                    })
                    .on_ok({
                        let view = view.clone();
                        let base_path = base_path.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.apply_sftp_create_input(&submit_input, &base_path, is_dir, cx)
                            })
                        }
                    })
                    .footer(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("cancel-sftp-create")
                                    .label(t!("cancel").to_string())
                                    .on_click(window.listener_for(
                                        &view,
                                        move |this, _, window, cx| {
                                            this.dismiss_modal_dialog(token, window, cx);
                                        },
                                    )),
                            )
                            .child(
                                Button::new("confirm-sftp-create")
                                    .primary()
                                    .label(t!("confirm").to_string())
                                    .on_click(window.listener_for(
                                        &view,
                                        move |this, _, window, cx| {
                                            if this.apply_sftp_create_input(
                                                &confirm_input,
                                                &confirm_base_path,
                                                is_dir,
                                                cx,
                                            ) {
                                                this.dismiss_modal_dialog(token, window, cx);
                                            }
                                        },
                                    )),
                            ),
                    )
                    .content(move |content, _, _| {
                        content.child(Input::new(&content_input).w_full())
                    })
            },
        );
        crate::app::input_focus::defer_focus_input_at_end(focus_input, window, cx);
    }

    fn apply_sftp_rename_input(
        &mut self,
        input: &Entity<InputState>,
        remote_path: &str,
        parent: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = input.read(cx).value().trim().to_string();
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            self.status = t!("sftp_invalid_name").into();
            cx.notify();
            return false;
        }
        if let Some(handle) = self.active_sftp_handle() {
            handle.send_command(crate::sftp::SftpCommand::RenamePath {
                old_path: remote_path.to_string(),
                new_path: crate::sftp::join_remote(parent, &name),
            });
        }
        cx.notify();
        true
    }

    pub(crate) fn show_sftp_rename_dialog(
        &mut self,
        remote_path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let old_name = remote_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let parent = crate::sftp::parent_dir(&remote_path).unwrap_or_else(|| "/".to_string());
        let input =
            cx.new(|cx| gpui_component::input::InputState::new(window, cx).default_value(old_name));
        let submit_input = input.clone();
        let focus_input = input.clone();
        self.open_modal_dialog(
            crate::app::DialogKind::QuickCommand,
            window,
            cx,
            move |dialog: Dialog, token, window, _| {
                let on_close_view = view.clone();
                let submit_input = submit_input.clone();
                let content_input = input.clone();
                let confirm_input = input.clone();
                let confirm_remote_path = remote_path.clone();
                let confirm_parent = parent.clone();
                dialog
                    .title(t!("sftp_rename").to_string())
                    .w(px(420.))
                    .on_close(move |_, window, cx| {
                        on_close_view.update(cx, |this, cx| {
                            this.modal_dialog_closed(token, window, cx);
                            cx.notify();
                        });
                    })
                    .on_ok({
                        let view = view.clone();
                        let remote_path = remote_path.clone();
                        let parent = parent.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.apply_sftp_rename_input(
                                    &submit_input,
                                    &remote_path,
                                    &parent,
                                    cx,
                                )
                            })
                        }
                    })
                    .footer(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("cancel-sftp-rename")
                                    .label(t!("cancel").to_string())
                                    .on_click(window.listener_for(
                                        &view,
                                        move |this, _, window, cx| {
                                            this.dismiss_modal_dialog(token, window, cx);
                                        },
                                    )),
                            )
                            .child(
                                Button::new("confirm-sftp-rename")
                                    .primary()
                                    .label(t!("confirm").to_string())
                                    .on_click(window.listener_for(
                                        &view,
                                        move |this, _, window, cx| {
                                            if this.apply_sftp_rename_input(
                                                &confirm_input,
                                                &confirm_remote_path,
                                                &confirm_parent,
                                                cx,
                                            ) {
                                                this.dismiss_modal_dialog(token, window, cx);
                                            }
                                        },
                                    )),
                            ),
                    )
                    .content(move |content, _, _| {
                        content.child(Input::new(&content_input).w_full())
                    })
            },
        );
        crate::app::input_focus::defer_focus_input_at_end(focus_input, window, cx);
    }

    fn apply_sftp_permissions_form(
        &mut self,
        form: &Entity<SftpPermissionsForm>,
        remote_path: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let (value, recursive, apply_to) = {
            let form = form.read(cx);
            (
                form.input.read(cx).value().trim().to_string(),
                form.recursive,
                form.apply_to,
            )
        };
        let Ok(mode) = u32::from_str_radix(&value, 8) else {
            self.status = t!("sftp_invalid_permissions").into();
            cx.notify();
            return false;
        };
        if value.len() < 3 || value.len() > 4 || mode > 0o7777 {
            self.status = t!("sftp_invalid_permissions").into();
            cx.notify();
            return false;
        }
        if let Some(handle) = self.active_sftp_handle() {
            handle.send_command(crate::sftp::SftpCommand::SetPermissions {
                remote_path: remote_path.to_string(),
                mode,
                recursive,
                apply_to,
            });
        }
        cx.notify();
        true
    }

    pub(crate) fn show_sftp_permissions_dialog(
        &mut self,
        remote_path: String,
        is_dir: bool,
        permissions: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let initial_mode = permissions.unwrap_or(if is_dir { 0o755 } else { 0o644 });
        let form = cx.new(|cx| {
            SftpPermissionsForm::new(remote_path.clone(), is_dir, initial_mode, window, cx)
        });
        let focus_input = form.read(cx).input.clone();
        let submit_form = form.clone();
        self.open_modal_dialog(
            crate::app::DialogKind::ManagedKeySelector,
            window,
            cx,
            move |dialog: Dialog, token, window, _| {
                let on_close_view = view.clone();
                let submit_form = submit_form.clone();
                let content_form = form.clone();
                let confirm_form = form.clone();
                let confirm_path = remote_path.clone();
                let content_max_height = (window.viewport_size().height - px(220.))
                    .max(px(180.))
                    .min(px(520.));
                dialog
                    .title(t!("sftp_file_permissions").to_string())
                    .w(px(560.))
                    .on_close(move |_, window, cx| {
                        on_close_view.update(cx, |this, cx| {
                            this.modal_dialog_closed(token, window, cx);
                            cx.notify();
                        });
                    })
                    .on_ok({
                        let view = view.clone();
                        let remote_path = remote_path.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.apply_sftp_permissions_form(&submit_form, &remote_path, cx)
                            })
                        }
                    })
                    .footer(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("cancel-sftp-permissions")
                                    .label(t!("cancel").to_string())
                                    .on_click(window.listener_for(
                                        &view,
                                        move |this, _, window, cx| {
                                            this.dismiss_modal_dialog(token, window, cx);
                                        },
                                    )),
                            )
                            .child(
                                Button::new("confirm-sftp-permissions")
                                    .primary()
                                    .label(t!("confirm").to_string())
                                    .on_click(window.listener_for(
                                        &view,
                                        move |this, _, window, cx| {
                                            if this.apply_sftp_permissions_form(
                                                &confirm_form,
                                                &confirm_path,
                                                cx,
                                            ) {
                                                this.dismiss_modal_dialog(token, window, cx);
                                            }
                                        },
                                    )),
                            ),
                    )
                    .content(move |content, _, _| {
                        content.child(
                            div()
                                .id("sftp-permissions-scroll")
                                .max_h(content_max_height)
                                .overflow_y_scroll()
                                .child(content_form.clone()),
                        )
                    })
            },
        );
        crate::app::input_focus::defer_focus_input_at_end(focus_input, window, cx);
    }

    fn apply_sftp_delete_paths(&mut self, paths: &[String], quick: bool, cx: &mut Context<Self>) {
        if let Some(handle) = self.active_sftp_handle() {
            let command = if quick {
                crate::sftp::SftpCommand::QuickDeletePaths(paths.to_vec())
            } else {
                crate::sftp::SftpCommand::DeletePaths(paths.to_vec())
            };
            handle.send_command(command);
        }
        if let Some(sftp) = self.active_sftp_mut() {
            sftp.selected_entries.clear();
        }
        cx.notify();
    }

    pub(crate) fn show_sftp_delete_paths_confirm_dialog(
        &mut self,
        paths: Vec<String>,
        quick: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let submit_paths = paths.clone();
        self.open_modal_dialog(
            crate::app::DialogKind::ManagedKeyImport,
            window,
            cx,
            move |dialog: Dialog, token, window, _| {
                let on_close_view = view.clone();
                let confirm_paths = paths.clone();
                let content_max_height = (window.viewport_size().height - px(220.))
                    .max(px(160.))
                    .min(px(420.));
                dialog
                    .title(if quick {
                        t!("sftp_quick_delete_title").to_string()
                    } else {
                        t!("confirm_delete").to_string()
                    })
                    .w(px(520.))
                    .on_close(move |_, window, cx| {
                        on_close_view.update(cx, |this, cx| {
                            this.modal_dialog_closed(token, window, cx);
                            cx.notify();
                        });
                    })
                    .on_ok({
                        let view = view.clone();
                        let submit_paths = submit_paths.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.apply_sftp_delete_paths(&submit_paths, quick, cx);
                            });
                            true
                        }
                    })
                    .footer(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new(if quick {
                                    "cancel-sftp-quick-delete"
                                } else {
                                    "cancel-sftp-delete"
                                })
                                .label(t!("cancel").to_string())
                                .on_click(window.listener_for(
                                    &view,
                                    move |this, _, window, cx| {
                                        this.dismiss_modal_dialog(token, window, cx);
                                    },
                                )),
                            )
                            .child(
                                Button::new(if quick {
                                    "confirm-sftp-quick-delete"
                                } else {
                                    "confirm-sftp-delete"
                                })
                                .danger()
                                .label(t!("confirm").to_string())
                                .on_click(window.listener_for(
                                    &view,
                                    move |this, _, window, cx| {
                                        this.apply_sftp_delete_paths(&confirm_paths, quick, cx);
                                        this.dismiss_modal_dialog(token, window, cx);
                                    },
                                )),
                            ),
                    )
                    .content({
                        let paths = paths.clone();
                        move |content, _, cx| {
                            let mut body =
                                v_flex().gap_3().child(div().child(
                                    t!("confirm_delete_desc", count = paths.len()).to_string(),
                                ));
                            if quick {
                                body = body.child(
                                    div()
                                        .w_full()
                                        .p_3()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(cx.theme().danger)
                                        .bg(cx.theme().danger.opacity(0.12))
                                        .text_color(cx.theme().danger)
                                        .child(t!("sftp_quick_delete_warning").to_string()),
                                );
                            }
                            body =
                                body.child(v_flex().gap_1().children(paths.iter().map(|path| {
                                    div()
                                        .text_size(rems(0.833))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(path.clone())
                                })));
                            content.child(
                                div()
                                    .id("sftp-delete-confirm-scroll")
                                    .max_h(content_max_height)
                                    .overflow_y_scroll()
                                    .child(body),
                            )
                        }
                    })
            },
        );
    }
}
