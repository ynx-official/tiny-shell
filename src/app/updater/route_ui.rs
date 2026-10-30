use gpui::{
    AnyWindowHandle, App, Context, ParentElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{ActiveTheme as _, v_flex};
use rust_i18n::t;

use super::{UpdateCheckResult, UpdateStatus, routes};
use crate::{TinyShell, app::runtime_state::UpdateRuntimeState};

pub(crate) fn routes_locked(runtime: &UpdateRuntimeState) -> bool {
    runtime.pending_route_save.is_some()
        || runtime.route_probing
        || matches!(
            runtime.status,
            Some(
                UpdateStatus::Checking
                    | UpdateStatus::Downloading(..)
                    | UpdateStatus::ReadyToRestart(..)
            )
        )
}

pub(crate) fn route_summary(runtime: &UpdateRuntimeState, cx: &App) -> gpui::Div {
    let label = if runtime.pending_route_save.is_some() {
        t!("update_route_saving").to_string()
    } else if runtime.route_probing {
        t!("update_route_probing").to_string()
    } else if let Some(route) = &runtime.active_route {
        let switched = matches!(&runtime.status, Some(UpdateStatus::Downloading(info, ..))
            if info.download_routes.first().is_some_and(|first| first != route));
        t!(
            if switched {
                "update_route_switched"
            } else {
                "update_route_current"
            },
            route = route.label()
        )
        .to_string()
    } else if let Some(route) = runtime
        .route_report
        .as_ref()
        .and_then(|report| report.routes.first())
    {
        t!("update_route_suggested", route = route.label()).to_string()
    } else {
        t!("update_route_not_tested").to_string()
    };
    v_flex()
        .w_full()
        .gap_1()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(label)
        .when_some(runtime.route_error.clone(), |this, error| {
            this.child(div().text_color(cx.theme().danger).child(error))
        })
}

impl TinyShell {
    pub(crate) fn start_update_route_probe(
        &mut self,
        check: bool,
        notification_window: Option<AnyWindowHandle>,
        cx: &mut Context<Self>,
    ) {
        if routes_locked(&self.update_runtime) {
            return;
        }
        let prefs = self.config.update_routes().clone();
        self.update_runtime.route_probing = true;
        self.update_runtime.route_error = None;
        if check {
            self.update_runtime.status = Some(UpdateStatus::Checking);
            self.update_runtime.active_route = None;
        }
        cx.notify();
        let view = cx.entity();
        cx.spawn(move |_, cx: &mut gpui::AsyncApp| {
            let cx = cx.clone();
            async move {
                let (tx, rx) = futures::channel::oneshot::channel();
                let task_prefs = prefs.clone();
                match crate::app::shared_runtime() {
                    Ok(runtime) => {
                        runtime.spawn(async move {
                            let _ = tx.send(routes::probe_routes(&task_prefs).await);
                        });
                    }
                    Err(error) => {
                        let _ = tx.send(Err(anyhow::anyhow!(error)));
                    }
                }
                let result = rx
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("route probe cancelled")));
                cx.update(|cx| {
                    let mut notify_update = false;
                    view.update(cx, |this, cx| {
                        this.update_runtime.route_probing = false;
                        if *this.config.update_routes() != prefs {
                            if check {
                                this.update_runtime.status = None;
                            }
                            cx.notify();
                            return;
                        }
                        this.update_runtime.route_checked_at = Some(chrono::Utc::now().timestamp());
                        match result {
                            Ok(report) => {
                                if check {
                                    this.update_runtime.active_route =
                                        report.routes.first().cloned();
                                    match super::check_probed_release(&report) {
                                        Ok(UpdateCheckResult::UpdateAvailable(info)) => {
                                            this.update_runtime.status =
                                                Some(UpdateStatus::UpdateAvailable(info));
                                            this.record_update_check_completed();
                                            notify_update = this.config.update_notify();
                                        }
                                        Ok(UpdateCheckResult::UpToDate(info)) => {
                                            this.update_runtime.status =
                                                Some(UpdateStatus::UpToDate(info));
                                            this.record_update_check_completed();
                                        }
                                        Err(error) => {
                                            this.update_runtime.status =
                                                Some(UpdateStatus::Error(format!("{error:#}")));
                                        }
                                    }
                                }
                                if report.routes.is_empty() {
                                    this.update_runtime.route_error =
                                        Some(t!("update_route_no_available").to_string());
                                }
                                this.update_runtime.route_report = Some(report);
                            }
                            Err(_) => {
                                let error = t!("update_route_probe_error").to_string();
                                this.update_runtime.route_error = Some(error.clone());
                                this.update_runtime.route_report = None;
                                if check {
                                    this.update_runtime.status = Some(UpdateStatus::Error(error));
                                }
                            }
                        }
                        cx.notify();
                    });
                    if notify_update && let Some(window) = notification_window {
                        let view = view.clone();
                        let _ = window.update(cx, move |_, window, cx| {
                            view.update(cx, |this, cx| this.show_update_dialog(window, cx))
                        });
                    }
                });
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probing_prevents_overlapping_checks() {
        let mut state = UpdateRuntimeState::default();
        assert!(!routes_locked(&state));
        state.route_probing = true;
        assert!(routes_locked(&state));
        state.route_probing = false;
        state.status = Some(UpdateStatus::Checking);
        assert!(routes_locked(&state));
    }

    #[test]
    fn ready_payload_cannot_be_replaced_by_another_check_or_route() {
        let mut state = UpdateRuntimeState::default();
        state.status.replace(UpdateStatus::ReadyToRestart(
            super::super::UpdateInfo {
                version: "2.3.0".into(),
                notes: String::new(),
                download_url: String::new(),
                size: 10,
                digest: String::new(),
                installation_kind: super::super::InstallationKind::Portable,
                download_routes: vec![routes::Route::github()],
            },
            "verified.zip".into(),
        ));
        assert!(routes_locked(&state));
    }
}
