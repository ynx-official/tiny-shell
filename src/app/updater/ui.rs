use std::time::Duration;

use gpui::{
    AnyWindowHandle, Context, FontWeight, InteractiveElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
    rems,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dialog::Dialog,
    h_flex,
    progress::Progress,
    scroll::{Scrollbar, ScrollbarShow},
    v_flex,
};
use rust_i18n::t;

use crate::{
    TinyShell,
    app::dialog_layout::{
        UPDATE_DIALOG_HEIGHT, UPDATE_RESTART_DIALOG_BASE_HEIGHT, centered_dialog_layout,
        confirmation_dialog_height,
    },
    system::format_bytes,
};

fn update_progress_percent(done: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        done.saturating_mul(100)
            .checked_div(total)
            .unwrap_or(0)
            .min(100)
    }
}

fn update_progress_value(done: u64, total: u64) -> f32 {
    update_progress_percent(done, total) as f32
}

impl TinyShell {
    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        self.check_for_updates_with_notification(None, cx);
    }

    pub(crate) fn check_for_updates_with_notification(
        &mut self,
        notification_window: Option<AnyWindowHandle>,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            self.update_runtime.status,
            Some(crate::app::updater::UpdateStatus::Checking)
                | Some(crate::app::updater::UpdateStatus::Downloading(_, _, _))
        ) {
            return;
        }

        self.update_runtime.status = Some(crate::app::updater::UpdateStatus::Checking);
        cx.notify();
        let view = cx.entity();
        cx.spawn({
            let view = view.clone();
            move |_, cx: &mut gpui::AsyncApp| {
                let cx = cx.clone();
                async move {
                    let (tx, rx) = futures::channel::oneshot::channel();
                    match crate::app::shared_runtime() {
                        Ok(runtime) => {
                            runtime.spawn(async move {
                                let result = crate::app::updater::check_for_update().await;
                                let _ = tx.send(result);
                            });
                        }
                        Err(error) => {
                            let _ = tx.send(Err(anyhow::anyhow!(error)));
                        }
                    }
                    let result = rx
                        .await
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("update check cancelled")));
                    cx.update(|cx| {
                        let update_available = matches!(
                            result,
                            Ok(crate::app::updater::UpdateCheckResult::UpdateAvailable(_))
                        );
                        match result {
                            Ok(crate::app::updater::UpdateCheckResult::UpdateAvailable(info)) => {
                                view.update(cx, |this, cx| {
                                    this.update_runtime.status = Some(
                                        crate::app::updater::UpdateStatus::UpdateAvailable(info),
                                    );
                                    this.record_update_check_completed();
                                    cx.notify();
                                });
                            }
                            Ok(crate::app::updater::UpdateCheckResult::UpToDate(info)) => {
                                view.update(cx, |this, cx| {
                                    this.update_runtime.status =
                                        Some(crate::app::updater::UpdateStatus::UpToDate(info));
                                    this.record_update_check_completed();
                                    cx.notify();
                                });
                            }
                            Err(err) => {
                                view.update(cx, |this, cx| {
                                    this.update_runtime.status =
                                        Some(crate::app::updater::UpdateStatus::Error(format!(
                                            "{err:#}"
                                        )));
                                    cx.notify();
                                });
                            }
                        }

                        if update_available
                            && let Some(window_handle) = notification_window
                            && view.read(cx).config.update_notify()
                        {
                            let view = view.clone();
                            let _ = window_handle.update(cx, move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.show_update_dialog(window, cx);
                                });
                            });
                        }
                    });
                }
            }
        })
        .detach();
    }

    fn record_update_check_completed(&mut self) {
        self.config
            .set_update_last_checked_at(chrono::Utc::now().timestamp());
        self.mark_config_preferences_dirty();
    }

    pub(crate) fn schedule_automatic_update_checks(
        &mut self,
        window_handle: AnyWindowHandle,
        on_startup: bool,
        cx: &mut Context<Self>,
    ) {
        use crate::session::config::UpdateCheckMode;

        let mode = self.config.update_check_mode();
        if matches!(mode, UpdateCheckMode::Disabled)
            || matches!(mode, UpdateCheckMode::Startup) && !on_startup
        {
            self.update_runtime.start_schedule();
            self.async_runtime.supervisor.cancel("automatic-update");
            return;
        }

        let generation = self.update_runtime.start_schedule();
        let cancellation = self.async_runtime.supervisor.start("automatic-update");

        match mode {
            UpdateCheckMode::Disabled => (),
            UpdateCheckMode::Startup => {
                self.check_for_updates_with_notification(Some(window_handle), cx);
            }
            UpdateCheckMode::Interval => {
                let interval_hours = self.config.update_interval_hours() as u64;
                let interval = Duration::from_secs(interval_hours.saturating_mul(3_600));
                let initial_delay = crate::app::updater::automatic_update_delay(
                    self.config.update_interval_hours(),
                    self.config.update_last_checked_at(),
                    chrono::Utc::now().timestamp(),
                );

                cx.spawn(async move |this, cx| {
                    if cancellation.is_cancelled() {
                        return;
                    }
                    cx.background_executor().timer(initial_delay).await;
                    loop {
                        if cancellation.is_cancelled() {
                            break;
                        }
                        let Ok(is_current_schedule) = this.update(cx, |this, cx| {
                            if !this.update_runtime.is_current_schedule(generation)
                                || this.config.update_check_mode() != UpdateCheckMode::Interval
                            {
                                return false;
                            }
                            this.check_for_updates_with_notification(Some(window_handle), cx);
                            true
                        }) else {
                            break;
                        };
                        if !is_current_schedule {
                            break;
                        }
                        cx.background_executor().timer(interval).await;
                    }
                })
                .detach();
            }
        }
    }

    pub(crate) fn download_available_update(&mut self, cx: &mut Context<Self>) {
        let info = match self.update_runtime.status.clone() {
            Some(crate::app::updater::UpdateStatus::UpdateAvailable(info))
            | Some(crate::app::updater::UpdateStatus::DownloadCancelled(info))
            | Some(crate::app::updater::UpdateStatus::DownloadFailed(info, _)) => info,
            _ => return,
        };
        let cancellation = crate::app::updater::DownloadCancellation::default();
        let task_cancellation = self.async_runtime.supervisor.start("update-download");
        let generation = self.update_runtime.start_download(cancellation.clone());
        self.update_runtime.status = Some(crate::app::updater::UpdateStatus::Downloading(
            info.clone(),
            0,
            info.size,
        ));
        cx.notify();

        let view = cx.entity();
        cx.spawn({
            let view = view.clone();
            move |_, cx: &mut gpui::AsyncApp| {
                let cx = cx.clone();
                async move {
                    if task_cancellation.is_cancelled() {
                        return;
                    }
                    let (result_tx, result_rx) = futures::channel::oneshot::channel();
                    let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded();
                    let update_info = info.clone();
                    let download_cancellation = cancellation.clone();
                    match crate::app::shared_runtime() {
                        Ok(runtime) => {
                            runtime.spawn(async move {
                                let result = crate::app::updater::download_update(
                                    &update_info,
                                    &download_cancellation,
                                    |done, total| {
                                        let _ = progress_tx.unbounded_send((done, total));
                                    },
                                )
                                .await;
                                let _ = result_tx.send(result);
                            });
                        }
                        Err(error) => {
                            let _ = result_tx.send(Err(anyhow::anyhow!(error)));
                        }
                    }
                    use futures::StreamExt as _;
                    while let Some((done, total)) = progress_rx.next().await {
                        if task_cancellation.is_cancelled() {
                            break;
                        }
                        cx.update(|cx| {
                            view.update(cx, |this, cx| {
                                if !this.update_runtime.is_current_download(generation) {
                                    return;
                                }
                                this.update_runtime.status =
                                    Some(crate::app::updater::UpdateStatus::Downloading(
                                        info.clone(),
                                        done,
                                        total,
                                    ));
                                cx.notify();
                            });
                        });
                    }
                    let result = result_rx
                        .await
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("update download cancelled")));
                    cx.update(|cx| {
                        view.update(cx, |this, cx| {
                            this.async_runtime
                                .supervisor
                                .finish("update-download", task_cancellation.id());
                            if !this.update_runtime.finish_download(generation) {
                                return;
                            }
                            match result {
                                Ok(path) => {
                                    this.update_runtime.status =
                                        Some(crate::app::updater::UpdateStatus::ReadyToRestart(
                                            info.clone(),
                                            path,
                                        ));
                                }
                                Err(_err) if cancellation.is_cancelled() => {
                                    this.update_runtime.status =
                                        Some(crate::app::updater::UpdateStatus::DownloadCancelled(
                                            info.clone(),
                                        ));
                                }
                                Err(err) => {
                                    this.update_runtime.status =
                                        Some(crate::app::updater::UpdateStatus::DownloadFailed(
                                            info.clone(),
                                            format!("{err:#}"),
                                        ));
                                }
                            }
                            cx.notify();
                        });
                    });
                }
            }
        })
        .detach();
    }

    pub(crate) fn cancel_update_download(&mut self, cx: &mut Context<Self>) {
        let Some(crate::app::updater::UpdateStatus::Downloading(info, _, _)) =
            self.update_runtime.status.clone()
        else {
            return;
        };
        self.update_runtime.cancel_download();
        self.update_runtime.status =
            Some(crate::app::updater::UpdateStatus::DownloadCancelled(info));
        cx.notify();
    }

    pub(crate) fn confirm_update_restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(crate::app::updater::UpdateStatus::ReadyToRestart(info, path)) =
            self.update_runtime.status.clone()
        else {
            return;
        };

        let view = cx.entity();
        self.replace_modal_dialog(
            crate::app::DialogKind::Updater,
            window,
            cx,
            move |dialog: Dialog, token, dialog_window, _| {
                let preferred_height =
                    confirmation_dialog_height(dialog_window, UPDATE_RESTART_DIALOG_BASE_HEIGHT);
                let layout = centered_dialog_layout(dialog_window, preferred_height, 0);
                let display_version = info.version.clone();
                let expected_version = info.version.clone();
                let installation_kind = info.installation_kind;
                let path = path.clone();
                let view = view.clone();
                dialog
                    .title(t!("update_restart_confirm_title").to_string())
                    .w(px(440.))
                    .h(layout.height)
                    .margin_top(layout.margin_top)
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
                        content.child(
                            div().text_sm().child(
                                t!(
                                    "update_restart_confirm_desc",
                                    version = display_version.clone()
                                )
                                .to_string(),
                            ),
                        )
                    })
                    .footer(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("cancel-update-restart")
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
                                Button::new("confirm-update-restart")
                                    .primary()
                                    .label(t!("update_restart_now").to_string())
                                    .on_click({
                                        let path = path.clone();
                                        let view = view.clone();
                                        let expected_version = expected_version.clone();
                                        move |_, _window, _cx| {
                                            if let Err(error) =
                                                crate::app::updater::install_and_restart(
                                                    &path,
                                                    &expected_version,
                                                    installation_kind,
                                                )
                                            {
                                                tracing::error!(
                                                    "failed to install update: {error:#}"
                                                );
                                                view.update(_cx, |this, cx| {
                                                    this.update_runtime.status = Some(
                                                        crate::app::updater::UpdateStatus::Error(
                                                            format!("{error:#}"),
                                                        ),
                                                    );
                                                    cx.notify();
                                                });
                                            }
                                        }
                                    }),
                            ),
                    )
            },
        );
    }

    pub(crate) fn show_update_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_runtime.status.is_none() {
            self.check_for_updates(cx);
        }

        let view = cx.entity();
        let notes_scroll_handle = gpui::ScrollHandle::new();
        self.open_modal_dialog(crate::app::DialogKind::Updater, window, cx, move |dialog: Dialog, token, dialog_window, _| {
            let layout = centered_dialog_layout(dialog_window, UPDATE_DIALOG_HEIGHT, 0);
            dialog
                .title(t!("update_dialog_title").to_string())
                .w(px(600.))
                .h(layout.height)
                .margin_top(layout.margin_top)
                .overlay_closable(true)
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
                    let view = view.clone();
                    let notes_scroll_handle = notes_scroll_handle.clone();
                    move |content, window, cx| {
                        let current_version = env!("CARGO_PKG_VERSION");
                        let status = view.read(cx).update_runtime.status.clone();
                        let show_update_pulse = matches!(
                            &status,
                            Some(crate::app::updater::UpdateStatus::UpdateAvailable(_))
                        );
                        let can_restart = matches!(
                            &status,
                            Some(crate::app::updater::UpdateStatus::ReadyToRestart(_, _))
                        );
                        let is_downloading = matches!(
                            &status,
                            Some(crate::app::updater::UpdateStatus::Downloading(_, _, _))
                        );
                        let can_retry = matches!(
                            &status,
                            Some(crate::app::updater::UpdateStatus::DownloadCancelled(_))
                                | Some(crate::app::updater::UpdateStatus::DownloadFailed(_, _))
                        );
                        let (title, detail, notes, has_update, is_busy, is_error) = match status.clone() {
                            Some(crate::app::updater::UpdateStatus::Checking) => (
                                t!("checking_update").to_string(),
                                format!("{} v{current_version}", t!("update_current_version")),
                                String::new(),
                                false,
                                true,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::UpToDate(info)) => (
                                t!("update_no_update").to_string(),
                                format!(
                                    "{} v{current_version}  ·  {} v{}",
                                    t!("update_current_version"),
                                    t!("update_latest_version"),
                                    info.version
                                ),
                                info.notes,
                                false,
                                false,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::UpdateAvailable(info)) => (
                                t!("update_available", version = info.version.clone()).to_string(),
                                format!(
                                    "{} v{current_version}  ·  {} v{}",
                                    t!("update_current_version"),
                                    t!("update_latest_version"),
                                    info.version
                                ),
                                info.notes,
                                true,
                                false,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::Downloading(info, done, total)) => (
                                t!("update_downloading").to_string(),
                                if total > 0 {
                                    format!(
                                        "{}%  ·  {} / {}",
                                        update_progress_percent(done, total),
                                        format_bytes(done),
                                        format_bytes(total)
                                    )
                                } else {
                                    format_bytes(done)
                                },
                                info.notes,
                                false,
                                true,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::DownloadCancelled(info)) => (
                                t!("update_download_cancelled").to_string(),
                                t!("update_download_cancelled_desc").to_string(),
                                info.notes,
                                true,
                                false,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::DownloadFailed(info, error)) => (
                                t!("update_download_failed").to_string(),
                                error,
                                info.notes,
                                true,
                                false,
                                true,
                            ),
                            Some(crate::app::updater::UpdateStatus::ReadyToRestart(info, _)) => (
                                t!("update_install_complete").to_string(),
                                t!("update_restart_hint").to_string(),
                                info.notes,
                                false,
                                false,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::Error(error)) => (
                                t!("update_check_failed").to_string(),
                                error,
                                String::new(),
                                false,
                                false,
                                true,
                            ),
                            None => (
                                t!("update_no_update").to_string(),
                                format!("{} v{current_version}", t!("update_current_version")),
                                String::new(),
                                false,
                                false,
                                false,
                            ),
                        };

                        let note_rows = notes
                            .lines()
                            .filter_map(|line| {
                                let line = line.trim();
                                if line.is_empty() {
                                    return None;
                                }
                                let is_heading = line.starts_with('#');
                                let text = if is_heading {
                                    line.trim_start_matches('#').trim().to_string()
                                } else if let Some(item) = line
                                    .strip_prefix("- ")
                                    .or_else(|| line.strip_prefix("* "))
                                {
                                    format!("• {item}")
                                } else {
                                    line.to_string()
                                };
                                Some(
                                    div()
                                        .w_full()
                                        .text_size(if is_heading { rems(0.92) } else { rems(0.8) })
                                        .when(is_heading, |this| {
                                            this.font_weight(FontWeight::SEMIBOLD)
                                        })
                                        .child(text),
                                )
                            })
                            .collect::<Vec<_>>();

                        content.child(
                            v_flex()
                                .size_full()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .flex_none()
                                        .items_center()
                                        .gap_3()
                                        .p_3()
                                        .rounded_lg()
                                        .bg(cx.theme().muted.opacity(0.45))
                                        .when(show_update_pulse, |this| {
                                            this.child(crate::app::updater::compact_pulse_icon(
                                                "update-dialog-pulse",
                                                cx.theme().primary,
                                            ))
                                        })
                                        .when(!show_update_pulse, |this| {
                                            this.child(
                                                div()
                                                    .size(px(10.))
                                                    .rounded_full()
                                                    .bg(if is_error {
                                                        cx.theme().danger
                                                    } else if has_update {
                                                        cx.theme().primary
                                                    } else {
                                                        cx.theme().success
                                                    }),
                                            )
                                        })
                                        .child(
                                            v_flex()
                                                .min_w(px(0.))
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_size(rems(1.05))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child(title),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(rems(0.78))
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(detail),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(rems(0.85))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(t!("update_release_notes").to_string()),
                                )
                                .child(
                                    div()
                                        .relative()
                                        .flex_1()
                                        .min_h(px(0.))
                                        .rounded_md()
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .bg(cx.theme().background)
                                        .child(
                                            v_flex()
                                                .id("update-notes-scroll")
                                                .size_full()
                                                .track_scroll(&notes_scroll_handle)
                                                .overflow_y_scroll()
                                                .p_3()
                                                .gap_2()
                                                .when(note_rows.is_empty(), |this| {
                                                    this.items_center()
                                                        .justify_center()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(t!("update_no_release_notes").to_string())
                                                })
                                                .children(note_rows),
                                        )
                                        .child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .right_0()
                                                .bottom_0()
                                                .child(
                                                    Scrollbar::vertical(&notes_scroll_handle)
                                                        .scrollbar_show(ScrollbarShow::Scrolling),
                                                ),
                                        ),
                                )
                                .when(
                                    matches!(
                                        status,
                                        Some(crate::app::updater::UpdateStatus::Downloading(_, _, _))
                                    ),
                                    |this| {
                                        let (done, total) = match status.as_ref() {
                                            Some(crate::app::updater::UpdateStatus::Downloading(_, done, total)) => (*done, *total),
                                            _ => unreachable!(),
                                        };
                                        this.child(
                                            v_flex()
                                                .w_full()
                                                .gap_1()
                                                .child(
                                                    h_flex()
                                                        .w_full()
                                                        .justify_between()
                                                        .text_size(rems(0.75))
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(t!("update_download_progress").to_string())
                                                        .child(format!(
                                                            "{}%",
                                                            update_progress_percent(done, total)
                                                        )),
                                                )
                                                .child(
                                                    Progress::new("update-download-progress")
                                                        .with_size(px(6.))
                                                        .value(update_progress_value(done, total))
                                                        .color(cx.theme().primary)
                                                        .w_full(),
                                                ),
                                        )
                                    },
                                )
                                .child(
                                    h_flex()
                                        .flex_none()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new("update-release-page")
                                                .ghost()
                                                .label(t!("update_release_page").to_string())
                                                .on_click(|_, _, _| {
                                                    let _ = crate::app::platform::open_url(
                                                        "https://github.com/ynx-official/tiny-shell/releases/latest",
                                                    );
                                                }),
                                        )
                                        .child(
                                            Button::new("update-check-again")
                                                .secondary()
                                                .disabled(is_busy)
                                                .label(t!("check_update").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| this.check_for_updates(cx),
                                                )),
                                        )
                                        .when(is_downloading, |this| {
                                            this.child(
                                                Button::new("update-cancel-download")
                                                    .secondary()
                                                    .label(t!("update_cancel_download").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.cancel_update_download(cx)
                                                        },
                                                    )),
                                            )
                                        })
                                        .when(has_update, |this| {
                                            this.child(
                                                Button::new("update-download")
                                                    .primary()
                                                    .label(if can_retry {
                                                        t!("update_download_again").to_string()
                                                    } else {
                                                        t!("update_download").to_string()
                                                    })
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.download_available_update(cx)
                                                        },
                                                    )),
                                            )
                                        })
                                        .when(can_restart, |this| {
                                            this.child(
                                                Button::new("update-restart")
                                                    .primary()
                                                    .label(t!("update_restart_now").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, window, cx| {
                                                            this.confirm_update_restart(window, cx)
                                                        },
                                                    )),
                                            )
                                        }),
                                ),
                        )
                    }
                })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::update_progress_value;

    #[test]
    fn update_progress_value_uses_percentage_scale() {
        assert_eq!(update_progress_value(25, 100), 25.0);
        assert_eq!(update_progress_value(150, 100), 100.0);
        assert_eq!(update_progress_value(1, 0), 0.0);
    }
}
