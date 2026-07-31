use gpui::{
    Anchor, Entity, IntoElement, ParentElement as _, Styled as _, div, prelude::FluentBuilder as _,
    px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    progress::Progress,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    switch::Switch,
    v_flex,
};
use rust_i18n::t;

use crate::{TinyShell, app::updater::UpdateStatus, session::config::UpdateCheckMode};

#[derive(Debug, PartialEq)]
struct UpdateStatusPresentation {
    text: String,
    progress: Option<(u64, u64)>,
    has_update: bool,
    can_retry: bool,
    can_restart: bool,
}

fn progress_percent(done: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        done.saturating_mul(100)
            .checked_div(total)
            .unwrap_or(0)
            .min(100)
    }
}

fn progress_value(done: u64, total: u64) -> f32 {
    progress_percent(done, total) as f32
}

fn present_status(status: Option<&UpdateStatus>) -> UpdateStatusPresentation {
    let (text, progress) = match status {
        Some(UpdateStatus::Checking) => (t!("checking_update").to_string(), None),
        Some(UpdateStatus::UpToDate(_)) => (t!("update_latest").to_string(), None),
        Some(UpdateStatus::UpdateAvailable(info)) => (
            t!("update_available", version = info.version.clone()).to_string(),
            None,
        ),
        Some(UpdateStatus::Downloading(_, done, total)) => (
            format!(
                "{} · {}%",
                t!("update_downloading"),
                progress_percent(*done, *total)
            ),
            Some((*done, *total)),
        ),
        Some(UpdateStatus::DownloadCancelled(_)) => {
            (t!("update_download_cancelled").to_string(), None)
        }
        Some(UpdateStatus::DownloadFailed(_, error)) => (
            t!("update_download_error", error = error.clone()).to_string(),
            None,
        ),
        Some(UpdateStatus::ReadyToRestart(_, _)) => {
            (t!("update_install_complete").to_string(), None)
        }
        Some(UpdateStatus::Error(message)) => (
            t!("update_error", error = message.clone()).to_string(),
            None,
        ),
        None => (t!("update_not_checked").to_string(), None),
    };

    UpdateStatusPresentation {
        text,
        progress,
        has_update: matches!(
            status,
            Some(
                UpdateStatus::UpdateAvailable(_)
                    | UpdateStatus::DownloadCancelled(_)
                    | UpdateStatus::DownloadFailed(_, _)
            )
        ),
        can_retry: matches!(
            status,
            Some(UpdateStatus::DownloadCancelled(_) | UpdateStatus::DownloadFailed(_, _))
        ),
        can_restart: matches!(status, Some(UpdateStatus::ReadyToRestart(_, _))),
    }
}

pub(crate) fn page(
    view: &Entity<TinyShell>,
    update_interval_hours: Entity<InputState>,
) -> SettingPage {
    SettingPage::new(t!("settings_online_update").to_string())
        .icon(IconName::ArrowDown)
        .group(
            SettingGroup::new()
                .title(t!("update_settings_group").to_string())
                .item(
                    SettingItem::new(
                        t!("update_check_frequency").to_string(),
                        SettingField::render({
                            let view = view.clone();
                            move |_, _window, cx| {
                                let mode = view.read(cx).config.update_check_mode();
                                Button::new("update-frequency-dropdown")
                                    .small()
                                    .label({
                                        let key = super::update_check_mode_key(mode);
                                        t!(key).to_string()
                                    })
                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                        let view = view.clone();
                                        move |mut menu, window, cx| {
                                            let current = view.read(cx).config.update_check_mode();
                                            menu = menu.min_w(180.);
                                            for mode in [
                                                UpdateCheckMode::Startup,
                                                UpdateCheckMode::Interval,
                                                UpdateCheckMode::Disabled,
                                            ] {
                                                let key = super::update_check_mode_key(mode);
                                                menu = menu.item(
                                                    PopupMenuItem::new(t!(key).to_string())
                                                        .checked(current == mode)
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            move |this, _, window, cx| {
                                                                this.set_update_mode(
                                                                    mode,
                                                                    window.window_handle(),
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                );
                                            }
                                            menu
                                        }
                                    })
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(t!("update_check_frequency_desc").to_string()),
                )
                .item(
                    SettingItem::new(
                        t!("update_interval_hours").to_string(),
                        SettingField::render({
                            let view = view.clone();
                            move |_, _window, cx| {
                                let enabled = view.read(cx).config.update_check_mode()
                                    == UpdateCheckMode::Interval;
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        Input::new(&update_interval_hours)
                                            .small()
                                            .w(px(96.))
                                            .disabled(!enabled),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!("update_hours_unit").to_string()),
                                    )
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(t!("update_interval_hours_desc").to_string()),
                )
                .item(
                    SettingItem::new(
                        t!("update_notify").to_string(),
                        SettingField::render({
                            let view = view.clone();
                            move |_, window, cx| {
                                Switch::new("update-notify")
                                    .small()
                                    .checked(view.read(cx).config.update_notify())
                                    .on_click(window.listener_for(&view, |this, checked, _, cx| {
                                        this.set_update_notifications(*checked, cx);
                                    }))
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(t!("update_notify_desc").to_string()),
                ),
        )
        .group(
            SettingGroup::new()
                .title(t!("update_status").to_string())
                .item(SettingItem::render({
                    let view = view.clone();
                    move |_, window, cx| {
                        let status = present_status(view.read(cx).updater_status.as_ref());
                        let is_downloading = status.progress.is_some();

                        v_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                h_flex()
                                    .w_full()
                                    .justify_between()
                                    .gap_3()
                                    .items_center()
                                    .child(div().min_w_0().flex_1().text_sm().child(status.text))
                                    .child(
                                        h_flex()
                                            .flex_shrink_0()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                Button::new("check-update")
                                                    .disabled(is_downloading)
                                                    .label(t!("check_update").to_string())
                                                    .on_click({
                                                        let view = view.clone();
                                                        move |_, _window, cx| {
                                                            view.update(cx, |this, cx| {
                                                                this.check_for_updates(cx)
                                                            });
                                                        }
                                                    }),
                                            )
                                            .when(is_downloading, |this| {
                                                this.child(
                                                    Button::new("cancel-update-download")
                                                        .secondary()
                                                        .label(
                                                            t!("update_cancel_download")
                                                                .to_string(),
                                                        )
                                                        .on_click({
                                                            let view = view.clone();
                                                            move |_, _window, cx| {
                                                                view.update(cx, |this, cx| {
                                                                    this.cancel_update_download(cx)
                                                                });
                                                            }
                                                        }),
                                                )
                                            })
                                            .when(status.has_update, |this| {
                                                this.child(
                                                    Button::new("download-update")
                                                        .primary()
                                                        .label(if status.can_retry {
                                                            t!("update_download_again").to_string()
                                                        } else {
                                                            t!("update_download").to_string()
                                                        })
                                                        .on_click({
                                                            let view = view.clone();
                                                            move |_, _window, cx| {
                                                                view.update(cx, |this, cx| {
                                                                    this.download_available_update(
                                                                        cx,
                                                                    )
                                                                });
                                                            }
                                                        }),
                                                )
                                            })
                                            .when(status.can_restart, |this| {
                                                this.child(
                                                    Button::new("restart-update")
                                                        .primary()
                                                        .label(t!("update_restart_now").to_string())
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            |this, _, window, cx| {
                                                                this.confirm_update_restart(
                                                                    window, cx,
                                                                )
                                                            },
                                                        )),
                                                )
                                            }),
                                    ),
                            )
                            .when_some(status.progress, |this, (done, total)| {
                                this.child(
                                    Progress::new("settings-update-progress")
                                        .with_size(px(6.))
                                        .value(progress_value(done, total))
                                        .color(cx.theme().primary)
                                        .w_full(),
                                )
                            })
                    }
                })),
        )
}

#[cfg(test)]
mod tests {
    use super::{present_status, progress_percent, progress_value};
    use crate::app::updater::UpdateStatus;

    #[test]
    fn update_status_presentation_exposes_available_actions() {
        let unchecked = present_status(None);
        assert!(!unchecked.has_update);
        assert!(!unchecked.can_retry);
        assert!(!unchecked.can_restart);

        let failed = present_status(Some(&UpdateStatus::Error("network".to_string())));
        assert!(failed.progress.is_none());
        assert!(!failed.has_update);
        assert!(!failed.can_retry);
        assert!(!failed.can_restart);
    }

    #[test]
    fn update_progress_uses_percentage_scale() {
        assert_eq!(progress_percent(0, 0), 0);
        assert_eq!(progress_percent(50, 100), 50);
        assert_eq!(progress_percent(200, 100), 100);
        assert_eq!(progress_value(1, 4), 25.0);
    }
}
