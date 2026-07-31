use gpui::{
    Entity, FontWeight, ParentElement as _, Styled as _, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::Input,
    setting::{SettingGroup, SettingItem, SettingPage},
    switch::Switch,
    v_flex,
};
use rust_i18n::t;

use crate::{TinyShell, app::settings_window::SyncSettingsInputs};

use super::controls::{labeled_input, labeled_input_with_hint, split_inputs};

pub(crate) fn page(view: &Entity<TinyShell>, inputs: SyncSettingsInputs) -> SettingPage {
    SettingPage::new(t!("settings_sync").to_string())
        .icon(IconName::Globe)
        .group(
            SettingGroup::new()
                .title(t!("settings_sync").to_string())
                .item(SettingItem::render({
                    let view = view.clone();
                    move |_, window, cx| {
                        let (
                            in_progress,
                            status,
                            is_s3,
                            automatic_enabled,
                            last_synced,
                            next_sync,
                            include_secrets,
                        ) = {
                            let state = view.read(cx);
                            (
                                state.sync_in_progress,
                                state.sync_status.clone(),
                                state.config.sync_backend() == "s3",
                                state.config.sync_enabled(),
                                crate::app::config_sync::format_sync_timestamp(
                                    state.config.sync_last_synced_at(),
                                )
                                .unwrap_or_else(|| t!("sync_time_never").to_string()),
                                crate::app::config_sync::format_sync_timestamp(
                                    state.config.sync_next_at(),
                                )
                                .unwrap_or_else(|| t!("sync_time_pending").to_string()),
                                state.config.sync_include_secrets(),
                            )
                        };

                        let privacy_password_valid =
                            inputs.privacy_password.read(cx).value().chars().count() >= 8;
                        let can_upload =
                            !in_progress && (!include_secrets || privacy_password_valid);

                        v_flex()
                            .w_full()
                            .gap_3()
                            .child(backend_selector(&view, is_s3, window))
                            .when(!is_s3, |content| {
                                content
                                    .child(automatic_sync_controls(
                                        &view,
                                        &inputs,
                                        automatic_enabled,
                                        in_progress,
                                    ))
                                    .child(sync_times(last_synced, next_sync, cx))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(t!("sync_webdav_section").to_string()),
                                    )
                                    .child(labeled_input_with_hint(
                                        t!("sync_endpoint").to_string(),
                                        t!("sync_endpoint_hint").to_string(),
                                        inputs.endpoint.clone(),
                                        cx,
                                    ))
                                    .child(labeled_input(
                                        t!("sync_username").to_string(),
                                        inputs.username.clone(),
                                    ))
                                    .child(labeled_input(
                                        t!("sync_webdav_password").to_string(),
                                        inputs.webdav_password.clone(),
                                    ))
                                    .child(
                                        Button::new("sync-verify-connection")
                                            .small()
                                            .secondary()
                                            .disabled(in_progress)
                                            .label(t!("sync_verify_connection").to_string())
                                            .on_click({
                                                let view = view.clone();
                                                let inputs = inputs.clone();
                                                move |_, _, cx| {
                                                    view.update(cx, |this, cx| {
                                                        let form = crate::app::config_sync::SyncFormSnapshot::capture(
                                                            this.config.sync_backend(),
                                                            &inputs,
                                                            cx,
                                                        );
                                                        this.verify_sync_connection(form, cx);
                                                    });
                                                }
                                            }),
                                    )
                            })
                            .when(is_s3, |content| {
                                content
                                    .child(labeled_input(
                                        t!("sync_s3_endpoint").to_string(),
                                        inputs.s3_endpoint.clone(),
                                    ))
                                    .child(split_inputs(
                                        labeled_input(
                                            t!("sync_s3_region").to_string(),
                                            inputs.s3_region.clone(),
                                        ),
                                        labeled_input(
                                            t!("sync_s3_bucket").to_string(),
                                            inputs.s3_bucket.clone(),
                                        ),
                                    ))
                                    .child(labeled_input(
                                        t!("sync_s3_object_key").to_string(),
                                        inputs.s3_object_key.clone(),
                                    ))
                                    .child(labeled_input(
                                        t!("sync_s3_access_key").to_string(),
                                        inputs.s3_access_key.clone(),
                                    ))
                                    .child(labeled_input(
                                        t!("sync_s3_secret_key").to_string(),
                                        inputs.s3_secret_key.clone(),
                                    ))
                                    .child(labeled_input(
                                        t!("sync_s3_session_token").to_string(),
                                        inputs.s3_session_token.clone(),
                                    ))
                            })
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("sync_encryption_section").to_string()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("sync_security_hint").to_string()),
                            )
                            .child(
                                Checkbox::new("sync-include-secrets")
                                    .checked(include_secrets)
                                    .disabled(in_progress)
                                    .label(t!("sync_include_secrets").to_string())
                                    .on_click(window.listener_for(&view, {
                                        let inputs = inputs.clone();
                                        move |this, checked: &bool, window, cx| {
                                            if !*checked {
                                                let _ = this.set_sync_include_secrets(false, cx);
                                                return;
                                            }
                                            let form = crate::app::config_sync::SyncFormSnapshot::capture(
                                                this.config.sync_backend(),
                                                &inputs,
                                                cx,
                                            );
                                            this.show_sync_secrets_password_dialog(
                                                form,
                                                inputs.privacy_password.clone(),
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                            )
                            .when(include_secrets, |content| {
                                content
                                    .child(labeled_input_with_hint(
                                        t!("sync_privacy_password").to_string(),
                                        t!("sync_privacy_password_hint").to_string(),
                                        inputs.privacy_password.clone(),
                                        cx,
                                    ))
                                    .child(
                                        Button::new("sync-reset-privacy")
                                            .ghost()
                                            .small()
                                            .label(t!("sync_reset_privacy_password").to_string())
                                            .disabled(in_progress)
                                            .on_click(window.listener_for(&view, {
                                                let inputs = inputs.clone();
                                                move |this, _, window, cx| {
                                                    let form = crate::app::config_sync::SyncFormSnapshot::capture(
                                                        this.config.sync_backend(),
                                                        &inputs,
                                                        cx,
                                                    );
                                                    this.show_reset_privacy_password_dialog(
                                                        form, window, cx,
                                                    );
                                                }
                                            })),
                                    )
                            })
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("sync-download")
                                            .small()
                                            .secondary()
                                            .disabled(in_progress)
                                            .label(t!("sync_download").to_string())
                                            .on_click(window.listener_for(&view, {
                                                let inputs = inputs.clone();
                                                move |this, _, _, cx| {
                                                    let form = crate::app::config_sync::SyncFormSnapshot::capture(
                                                        this.config.sync_backend(),
                                                        &inputs,
                                                        cx,
                                                    );
                                                    this.download_sync_config(form, cx);
                                                }
                                            })),
                                    )
                                    .child(
                                        Button::new("sync-upload")
                                            .small()
                                            .primary()
                                            .disabled(!can_upload)
                                            .label(t!("sync_upload").to_string())
                                            .on_click(window.listener_for(&view, {
                                                let inputs = inputs.clone();
                                                move |this, _, _, cx| {
                                                    let form = crate::app::config_sync::SyncFormSnapshot::capture(
                                                        this.config.sync_backend(),
                                                        &inputs,
                                                        cx,
                                                    );
                                                    this.upload_sync_config(form, cx);
                                                }
                                            })),
                                    ),
                            )
                            .child(status_banner(status.to_string(), cx))
                    }
                })),
        )
}

fn backend_selector(view: &Entity<TinyShell>, is_s3: bool, window: &mut gpui::Window) -> gpui::Div {
    h_flex()
        .gap_2()
        .child(
            Button::new("sync-backend-webdav")
                .small()
                .label("WebDAV")
                .when(!is_s3, |button| button.primary())
                .on_click(
                    window.listener_for(view, |this, _, _, cx| this.set_sync_backend("webdav", cx)),
                ),
        )
        .child(
            Button::new("sync-backend-s3")
                .small()
                .label("S3")
                .when(is_s3, |button| button.primary())
                .on_click(
                    window.listener_for(view, |this, _, _, cx| this.set_sync_backend("s3", cx)),
                ),
        )
}

fn automatic_sync_controls(
    view: &Entity<TinyShell>,
    inputs: &SyncSettingsInputs,
    enabled: bool,
    in_progress: bool,
) -> gpui::Div {
    v_flex()
        .gap_3()
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(t!("sync_automatic_enabled").to_string()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .child(t!("sync_automatic_enabled_desc").to_string()),
                        ),
                )
                .child(
                    Switch::new("sync-automatic-enabled")
                        .small()
                        .checked(enabled)
                        .disabled(in_progress)
                        .on_click({
                            let view = view.clone();
                            let inputs = inputs.clone();
                            move |checked, _, cx| {
                                view.update(cx, |this, cx| {
                                    let form = crate::app::config_sync::SyncFormSnapshot::capture(
                                        this.config.sync_backend(),
                                        &inputs,
                                        cx,
                                    );
                                    this.set_automatic_sync_enabled(*checked, form, cx);
                                });
                            }
                        }),
                ),
        )
        .child(
            v_flex()
                .gap_1()
                .child(div().text_sm().child(t!("sync_interval_hours").to_string()))
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Input::new(&inputs.interval_hours)
                                .small()
                                .w(px(96.))
                                .disabled(!enabled),
                        )
                        .child(div().text_sm().child(t!("update_hours_unit").to_string())),
                )
                .child(
                    div()
                        .text_xs()
                        .child(t!("sync_interval_hours_desc").to_string()),
                ),
        )
}

fn sync_times(last_synced: String, next_sync: String, cx: &gpui::App) -> gpui::Div {
    h_flex()
        .w_full()
        .gap_4()
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("sync_last_updated").to_string()),
                )
                .child(div().text_sm().child(last_synced)),
        )
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("sync_next_updated").to_string()),
                )
                .child(div().text_sm().child(next_sync)),
        )
}

fn status_banner(status: String, cx: &gpui::App) -> gpui::Div {
    let failed = status.starts_with(t!("sync_failed").as_ref());
    div()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(if failed {
            cx.theme().danger.opacity(0.4)
        } else {
            cx.theme().border
        })
        .bg(if failed {
            cx.theme().danger.opacity(0.08)
        } else {
            cx.theme().muted.opacity(0.35)
        })
        .text_sm()
        .text_color(if failed {
            cx.theme().danger
        } else {
            cx.theme().muted_foreground
        })
        .child(status)
}
