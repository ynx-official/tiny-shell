use gpui::{
    AppContext as _, Context, ParentElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    dialog::Dialog,
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use rust_i18n::t;

use crate::TinyShell;

impl TinyShell {
    pub(crate) fn show_sync_secrets_password_dialog(
        &mut self,
        form: crate::app::config_sync::SyncFormSnapshot,
        settings_password_input: gpui::Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("sync_privacy_password").to_string())
                .masked(true)
        });
        self.sync_runtime.secrets_password_dialog =
            Some(crate::app::SyncSecretsPasswordDialogState {
                token: crate::app::DialogToken { generation: 0 },
                status: crate::app::SyncSecretsPasswordDialogStatus::AwaitingInput,
                message: None,
                window: window.window_handle(),
                settings_password_input,
            });

        let view = cx.entity();
        let focus_input = password_input.clone();
        let danger_color = cx.theme().danger;
        let open_result = self.open_dialog(crate::app::DialogKind::VerifySyncSecretsPassword, window, cx, move |dialog: Dialog, token, _window, _cx| {
            dialog
                .title(t!("sync_secret_toggle_dialog_title").to_string())
                .w(px(440.))
                .close_button(false)
                .overlay_closable(false)
                .on_close({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.dialog_closed(token);
                            this.sync_runtime.secrets_password_dialog = None;
                            cx.notify();
                        });
                    }
                })
                .content({
                    let view = view.clone();
                    let password_input = password_input.clone();
                    move |content, _window, cx| {
                        let (status, message) = view
                            .read(cx)
                            .sync_runtime.secrets_password_dialog
                            .as_ref()
                            .map(|state| (state.status, state.message.clone()))
                            .unwrap_or((
                                crate::app::SyncSecretsPasswordDialogStatus::AwaitingInput,
                                None,
                            ));
                        content.child(
                            v_flex()
                                .w_full()
                                .gap_3()
                                .child(
                                    div()
                                        .text_sm()
                                        .child(t!("sync_secret_toggle_dialog_message").to_string()),
                                )
                                .child(Input::new(&password_input).w_full())
                                .when_some(message, |this, message| {
                                    this.child(
                                        div()
                                            .text_sm()
                                            .text_color(if status
                                                == crate::app::SyncSecretsPasswordDialogStatus::Verifying
                                            {
                                                cx.theme().muted_foreground
                                            } else {
                                                danger_color
                                            })
                                            .child(message),
                                    )
                                }),
                        )
                    }
                })
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel-sync-secrets-password")
                                .secondary()
                                .label(t!("cancel").to_string())
                                .on_click({
                                    let view = view.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.sync_runtime.secrets_password_dialog = None;
                                            this.dismiss_dialog(token, window, cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("verify-sync-secrets-password")
                                .primary()
                                .label(t!("sync_secret_toggle_verify").to_string())
                                .on_click({
                                    let view = view.clone();
                                    let password_input = password_input.clone();
                                    let form = form.clone();
                                    move |_, _, cx| {
                                        let password =
                                            password_input.read(cx).value().to_string();
                                        view.update(cx, |this, cx| {
                                            this.verify_sync_secrets_password(
                                                form.clone(),
                                                password,
                                                cx,
                                            );
                                        });
                                    }
                                }),
                        ),
                )
        });
        if matches!(open_result, crate::app::DialogOpenResult::Ignored) {
            self.sync_runtime.secrets_password_dialog = None;
        } else {
            crate::app::input_focus::defer_focus_input_at_end(focus_input, window, cx);
        }
    }

    /// 上传预检发现远端敏感字段无法解密时，阻止覆盖并引导用户重置。
    pub(crate) fn show_sync_upload_secrets_blocked_dialog(
        &mut self,
        form: crate::app::config_sync::SyncFormSnapshot,
        reason: crate::sync::UploadBlockReason,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = match reason {
            crate::sync::UploadBlockReason::PasswordRequired => {
                t!("sync_upload_password_required_dialog_message").to_string()
            }
            crate::sync::UploadBlockReason::PasswordMismatch => {
                t!("sync_privacy_password_incorrect_dialog_message").to_string()
            }
            crate::sync::UploadBlockReason::UnavailableSecrets {
                session_secret_count,
                managed_key_secret_count,
            } => t!(
                "sync_upload_blocked_dialog_message",
                sessions = session_secret_count,
                keys = managed_key_secret_count
            )
            .to_string(),
        };
        let view = cx.entity();
        let danger_color = cx.theme().danger;
        self.open_dialog(
            crate::app::DialogKind::SyncUploadSecretsBlocked,
            window,
            cx,
            move |dialog: Dialog, token, _window, _cx| {
                dialog
                    .title(t!("sync_upload_blocked_dialog_title").to_string())
                    .w(px(460.))
                    .close_button(false)
                    .overlay_closable(false)
                    .content({
                        let message = message.clone();
                        move |content, _window, _cx| {
                            content.child(
                                v_flex()
                                    .w_full()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(danger_color)
                                            .child(message.clone()),
                                    )
                                    .child(
                                        div().text_sm().child(
                                            t!("sync_upload_blocked_dialog_hint").to_string(),
                                        ),
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
                                Button::new("cancel-sync-upload-reset")
                                    .secondary()
                                    .label(t!("cancel").to_string())
                                    .on_click({
                                        let view = view.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.dismiss_dialog(token, window, cx);
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("continue-sync-upload-reset")
                                    .danger()
                                    .label(t!("sync_reset_privacy_password").to_string())
                                    .on_click({
                                        let view = view.clone();
                                        let form = form.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.dismiss_dialog(token, window, cx);
                                                cx.notify();
                                            });
                                            let view = view.clone();
                                            let form = form.clone();
                                            window.defer(cx, move |window, cx| {
                                                view.update(cx, |this, cx| {
                                                    this.show_reset_privacy_password_dialog(
                                                        form.clone(),
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            });
                                        }
                                    }),
                            ),
                    )
            },
        );
    }

    /// 弹出"重置隐私信息加密密码"对话框。
    ///
    /// 危险操作：会用本机当前明文配置 + 新密码重新加密并强制覆盖云端，
    /// 远端原有密文不可恢复，其他设备需用新密码才能解密。
    pub(crate) fn show_reset_privacy_password_dialog(
        &mut self,
        form: crate::app::config_sync::SyncFormSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_pw_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("sync_reset_new_password").to_string())
                .masked(true)
        });
        let confirm_pw_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("sync_reset_confirm_password").to_string())
                .masked(true)
        });
        let view = cx.entity();
        let focus_input = new_pw_input.clone();
        let danger_color = cx.theme().danger;
        self.open_dialog(
            crate::app::DialogKind::ResetPrivacyPassword,
            window,
            cx,
            move |dialog: Dialog, token, _window, _cx| {
                dialog
                    .title(t!("sync_reset_dialog_title").to_string())
                    .w(px(440.))
                    .close_button(false)
                    .overlay_closable(true)
                    .on_ok({
                        let view = view.clone();
                        let new_pw_input = new_pw_input.clone();
                        let confirm_pw_input = confirm_pw_input.clone();
                        let form = form.clone();
                        move |_, window, cx| {
                            let new_pw = new_pw_input.read(cx).value().to_string();
                            let confirm_pw = confirm_pw_input.read(cx).value().to_string();
                            if new_pw != confirm_pw {
                                view.update(cx, |this, cx| {
                                    this.sync_runtime.status =
                                        t!("sync_reset_password_mismatch").into();
                                    cx.notify();
                                });
                                return false;
                            }
                            if new_pw.chars().count() < 8 {
                                view.update(cx, |this, cx| {
                                    this.sync_runtime.status =
                                        t!("sync_privacy_password_required").into();
                                    cx.notify();
                                });
                                return false;
                            }
                            view.update(cx, |this, cx| {
                                this.confirm_reset_privacy_password(
                                    token,
                                    new_pw,
                                    form.clone(),
                                    window,
                                    cx,
                                );
                            });
                            false
                        }
                    })
                    .content({
                        let new_pw_input = new_pw_input.clone();
                        let confirm_pw_input = confirm_pw_input.clone();
                        move |content, _window, _cx| {
                            content.child(
                                v_flex()
                                    .w_full()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(danger_color)
                                            .child(t!("sync_reset_dialog_warning").to_string()),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(
                                                div().text_sm().child(
                                                    t!("sync_reset_new_password").to_string(),
                                                ),
                                            )
                                            .child(Input::new(&new_pw_input).w_full()),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(div().text_sm().child(
                                                t!("sync_reset_confirm_password").to_string(),
                                            ))
                                            .child(Input::new(&confirm_pw_input).w_full()),
                                    ),
                            )
                        }
                    })
            },
        );
        // 延迟聚焦到对话框显示完成后
        crate::app::input_focus::defer_focus_input_at_end(focus_input, window, cx);
    }

    /// 确认重置：关闭对话框并触发强制上传。
    fn confirm_reset_privacy_password(
        &mut self,
        token: crate::app::DialogToken,
        new_password: String,
        form: crate::app::config_sync::SyncFormSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_dialog(token, window, cx);
        self.reset_sync_privacy_password(new_password, form, cx);
    }
}
