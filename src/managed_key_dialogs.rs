use gpui::{
    AppContext as _, Context, InteractiveElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dialog::Dialog,
    h_flex,
    input::Input,
    v_flex,
};
use rust_i18n::t;

use crate::{TinyShell, app::ssh_key_import::KeyImportValidation};

/// Managed-key modal shown from either the main workspace or a standalone SSH editor.
/// The modal manager binds it to the exact native `window` passed here.
pub(crate) fn show_managed_key_selector_dialog(
    shell: &mut TinyShell,
    window: &mut Window,
    cx: &mut Context<TinyShell>,
) {
    shell.managed_keys = shell.config.managed_keys().to_vec();

    let view = cx.entity();
    let rename_input = shell.connection_inputs.key_import_remark_input.clone();
    shell.replace_modal_dialog(
        crate::app::DialogKind::ManagedKeySelector,
        window,
        cx,
        move |dialog: Dialog, token, window, _cx| {
            let dialog_width = px(760.).min(window.viewport_size().width - px(24.));
            dialog
                .title(t!("select_private_key").to_string())
                .w(dialog_width)
                .close_button(false)
                .overlay_closable(false)
                .on_close({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.modal_dialog_closed(token, window, cx);
                            this.managed_key_dialog_token = None;
                            this.managed_key_editor_target = None;
                            this.managed_key_dialog_selection = None;
                            this.editing_managed_key_id = None;
                            cx.notify();
                        });
                    }
                })
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
        },
    );
}

pub(crate) fn show_managed_key_import_dialog(
    shell: &mut TinyShell,
    window: &mut Window,
    cx: &mut Context<TinyShell>,
) {
    let view = cx.entity();
    let remark_input = shell.connection_inputs.key_import_remark_input.clone();
    let passphrase_input = shell.connection_inputs.key_import_passphrase_input.clone();
    let focus_remark_input = remark_input.clone();
    shell.replace_modal_dialog(
        crate::app::DialogKind::ManagedKeyImport,
        window,
        cx,
        move |dialog: Dialog, token, _window, _cx| {
            dialog
                .title(t!("key_import_dialog_title").to_string())
                .w(px(440.))
                .close_button(false)
                .overlay_closable(false)
                .on_close({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.modal_dialog_closed(token, window, cx);
                            this.managed_key_dialog_token = None;
                            this.key_import.close();
                            this.managed_key_dialog_selection = None;
                            cx.notify();
                        });
                    }
                })
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
                                t!("key_import_failed", error = error.to_string()).to_string(),
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
                                            Input::new(&passphrase_input)
                                                .flex_1()
                                                .mask_toggle(),
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
                                                        this.confirm_managed_key_import(
                                                            window, cx,
                                                        );
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
        },
    );
    crate::app::input_focus::defer_focus_input_at_end(focus_remark_input, window, cx);
}
