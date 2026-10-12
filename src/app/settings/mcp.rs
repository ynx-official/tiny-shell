//! MCP settings own transient UI state; authorization and execution remain in the engine.
mod access;
mod clients;
mod model;
mod permissions;
mod tasks;

use crate::{
    TinyShell,
    app::{confirmation_dialog::ConfirmationDialog, mcp},
    feedback::Feedback,
};
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InputEvent, InputState},
    setting::{SettingGroup, SettingItem, SettingPage},
    switch::Switch,
    tab::{Tab, TabBar},
    v_flex,
};
use rust_i18n::t;
use std::collections::HashSet;
use tiny_shell_mcp::{
    engine::{Review, Target},
    policy::{Mode, Policy, Scope},
    storage::Registration,
};

pub(crate) fn page(view: &Entity<TinyShell>) -> SettingPage {
    let view = view.clone();
    SettingPage::new(t!("settings_mcp").to_string())
        .icon(IconName::Network)
        .group(
            SettingGroup::new().item(SettingItem::render(move |_, window, cx| {
                window
                    .use_keyed_state("mcp-settings-panel", cx, |window, cx| {
                        McpPanel::new(view.clone(), window, cx)
                    })
                    .into_any_element()
            })),
        )
}

struct Snapshot {
    policy: Option<Policy>,
    targets: Vec<Target>,
    clients: Vec<Registration>,
    reviews: Vec<Review>,
    history: Vec<serde_json::Value>,
    bridge_error: Option<String>,
    data_error: Option<String>,
}

struct McpPanel {
    owner: Entity<TinyShell>,
    search: Entity<InputState>,
    tab: usize,
    authorized_only: bool,
    show_revoked: bool,
    capabilities: bool,
    selected_client: Option<String>,
    expanded_tasks: HashSet<String>,
    show_history: bool,
    connections_scroll: ScrollHandle,
    _search_subscription: Subscription,
}

#[derive(Clone)]
enum PolicyChange {
    Enabled(bool),
    Mode(Mode),
    Scope(Scope),
    Selection(Vec<Target>, bool),
}

impl McpPanel {
    fn new(owner: Entity<TinyShell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            crate::app::localization::localized_input(window, cx, || {
                t!("mcp_search_hint").to_string()
            })
        });
        let subscription = cx.subscribe(&search, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.connections_scroll.scroll_to_item(0);
                cx.notify();
            }
        });
        // Keyed state forwards notifications; observing its owner creates a render loop.
        Self {
            owner,
            search,
            tab: 0,
            authorized_only: false,
            show_revoked: false,
            capabilities: false,
            selected_client: None,
            expanded_tasks: HashSet::new(),
            show_history: false,
            connections_scroll: ScrollHandle::new(),
            _search_subscription: subscription,
        }
    }

    fn targets(&self, cx: &App) -> Vec<Target> {
        let config = &self.owner.read(cx).config;
        config
            .sessions()
            .iter()
            .filter(|s| s.connection_type == crate::session::config::ConnectionType::Ssh)
            .map(|s| crate::sftp::mcp_service::target(s, config))
            .collect()
    }

    fn snapshot(&self, cx: &App) -> Snapshot {
        let mut snapshot = Snapshot {
            policy: None,
            targets: self.targets(cx),
            clients: Vec::new(),
            reviews: Vec::new(),
            history: Vec::new(),
            bridge_error: None,
            data_error: None,
        };
        match mcp::service() {
            Ok(service) => {
                snapshot.bridge_error = service.listener_error.lock().ok().and_then(|e| e.clone());
                match service.engine.policy() {
                    Ok(policy) => snapshot.policy = Some(policy),
                    Err(e) => snapshot.data_error = Some(e.code),
                }
                match service.engine.store.clients() {
                    Ok(clients) => snapshot.clients = clients,
                    Err(_) => snapshot.data_error = Some("STORAGE_ERROR".into()),
                }
                match service.engine.reviews() {
                    Ok(reviews) => snapshot.reviews = reviews,
                    Err(e) => snapshot.data_error = Some(e.code),
                }
                if self.tab == 2 {
                    match service.engine.store.audit_tail() {
                        Ok(records) => snapshot.history = records,
                        Err(_) => snapshot.data_error = Some("STORAGE_ERROR".into()),
                    }
                }
            }
            Err(e) => snapshot.bridge_error = Some(e),
        }
        snapshot
    }

    fn save_policy(
        &self,
        change: PolicyChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let result = (|| {
            let service = mcp::service()?;
            let mut policy = service.engine.policy().map_err(|e| e.code)?;
            let before = serde_json::to_value(&policy).map_err(|_| "STORAGE_ERROR".to_string())?;
            let targets = self.targets(cx);
            match change {
                PolicyChange::Enabled(enabled) => policy.enabled = enabled,
                PolicyChange::Mode(mode) => policy.mode = mode,
                PolicyChange::Scope(Scope::Specified) => model::specify(&mut policy, &targets),
                PolicyChange::Scope(Scope::All) => {
                    policy.scope = Scope::All;
                    policy.observe(targets.iter().map(|t| (t.id.clone(), t.digest.clone())));
                }
                PolicyChange::Selection(selected, enabled) => {
                    // A row may be stale after editing a connection in another window.
                    if selected
                        .iter()
                        .any(|s| !targets.iter().any(|t| t.id == s.id && t.digest == s.digest))
                    {
                        return Err("TARGET_CHANGED".into());
                    }
                    model::select_targets(&mut policy, &targets, &selected, enabled);
                }
            }
            if serde_json::to_value(&policy).map_err(|_| "STORAGE_ERROR".to_string())? == before {
                return Ok(());
            }
            let _enter = service.runtime.enter();
            service.engine.set_policy(policy).map_err(|e| e.code)
        })();
        let success = report(result, "mcp_saved", window, cx);
        cx.refresh_windows();
        success
    }

    fn request_policy(&self, change: PolicyChange, window: &mut Window, cx: &mut Context<Self>) {
        let confirmation = match &change {
            PolicyChange::Mode(Mode::Full) => Some(("mcp_full", "mcp_full_confirm")),
            PolicyChange::Scope(Scope::All) => Some(("mcp_all_connections", "mcp_all_confirm")),
            _ => None,
        };
        if let Some((title, description)) = confirmation {
            let panel = cx.entity();
            ConfirmationDialog::new(t!(title).to_string(), t!(description).to_string())
                .danger(true)
                .on_ok(move |_, window, cx| {
                    panel.update(cx, |this, cx| this.save_policy(change.clone(), window, cx))
                })
                .open(window, cx);
        } else {
            self.save_policy(change, window, cx);
        }
    }

    fn render_header(&self, snapshot: &Snapshot, cx: &mut Context<Self>) -> gpui::Div {
        let policy = snapshot.policy.as_ref();
        let enabled = policy.is_some_and(|p| p.enabled);
        let ready = snapshot.bridge_error.is_none() && policy.is_some();
        let authorized = policy.map_or(0, |p| {
            snapshot
                .targets
                .iter()
                .filter(|t| p.authorized(&t.id, &t.digest))
                .count()
        });
        let pending = snapshot
            .reviews
            .iter()
            .filter(|r| r.state == tiny_shell_mcp::tasks::State::AwaitingUser)
            .count();
        let status = if !ready {
            t!("mcp_unavailable").to_string()
        } else if enabled {
            t!("mcp_bridge_running").to_string()
        } else {
            t!("mcp_disabled").to_string()
        };
        v_flex()
            .gap_2()
            .w_full()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Switch::new("mcp-enabled")
                                    .checked(enabled)
                                    .disabled(policy.is_none())
                                    .on_click(cx.listener(|this, checked, window, cx| {
                                        this.request_policy(
                                            PolicyChange::Enabled(*checked),
                                            window,
                                            cx,
                                        )
                                    })),
                            )
                            .child(t!("mcp_enable").to_string()),
                    )
                    .child(
                        Button::new("mcp-stop-all")
                            .small()
                            .outline()
                            .danger()
                            .label(t!("mcp_stop_short").to_string())
                            .tooltip(t!("mcp_stop_all").to_string())
                            .disabled(policy.is_none())
                            .on_click(cx.listener(|_, _, window, cx| {
                                let result = mcp::service().and_then(|service| {
                                    let _enter = service.runtime.enter();
                                    service.engine.stop_all().map_err(|e| e.code)
                                });
                                report(result, "mcp_stopped", window, cx);
                                cx.refresh_windows();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .p_3()
                    .rounded(px(6.))
                    .bg(cx.theme().muted)
                    .child(div().size(px(8.)).rounded_full().bg(if !ready {
                        cx.theme().danger
                    } else if enabled {
                        cx.theme().success
                    } else {
                        cx.theme().muted_foreground
                    }))
                    .child(div().text_sm().child(status))
                    .when_some(policy, |row, policy| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} · {}",
                                    t!(mode_key(policy.mode)),
                                    t!("mcp_authorized_count", count = authorized)
                                )),
                        )
                    })
                    .when(pending > 0, |row| {
                        row.child(
                            Button::new("mcp-pending-jump")
                                .small()
                                .ghost()
                                .label(t!("mcp_pending_count", count = pending).to_string())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.tab = 2;
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(hint(t!("mcp_description").to_string(), cx))
    }
}

impl Render for McpPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.snapshot(cx);
        let pending = snapshot
            .reviews
            .iter()
            .filter(|r| r.state == tiny_shell_mcp::tasks::State::AwaitingUser)
            .count();
        let content = match self.tab {
            1 => self.render_clients(&snapshot, window, cx),
            2 => self.render_tasks(&snapshot, window, cx),
            _ => self.render_access(&snapshot, window, cx),
        };
        // The settings page has a native title, page header and group gutters.
        // Keep the bridge controls visible while only the active tab scrolls.
        let scale = (window.rem_size() / px(14.)).max(1.);
        let panel_height = (window.viewport_size().height - px(148.) * scale).max(px(160.));
        v_flex()
            .w_full()
            .min_w_0()
            .h(panel_height)
            .min_h_0()
            .gap_3()
            .pb_3()
            .child(self.render_header(&snapshot, cx))
            .when_some(
                snapshot
                    .bridge_error
                    .as_ref()
                    .or(snapshot.data_error.as_ref()),
                |panel, error| {
                    panel.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(t!("mcp_action_failed", code = error).to_string()),
                    )
                },
            )
            .child(
                TabBar::new("mcp-page-tabs")
                    .underline()
                    .small()
                    .selected_index(self.tab)
                    .child(Tab::new().label(t!("mcp_tab_access").to_string()))
                    .child(Tab::new().label(t!("mcp_tab_clients").to_string()))
                    .child(Tab::new().label(if pending == 0 {
                        t!("mcp_tab_tasks").to_string()
                    } else {
                        format!("{} ({pending})", t!("mcp_tab_tasks"))
                    }))
                    .on_click(cx.listener(|this, index, _, cx| {
                        this.tab = *index;
                        cx.notify();
                    })),
            )
            .child(
                v_flex()
                    .id("mcp-tab-content")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_3()
                    .child(content)
                    .child(
                        div()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .pt_3()
                            .child(hint(t!("mcp_data_hint").to_string(), cx)),
                    ),
            )
    }
}

fn report(result: Result<(), String>, success: &str, window: &mut Window, cx: &mut App) -> bool {
    match result {
        Ok(()) => {
            Feedback::success(window, cx, t!(success).to_string());
            true
        }
        Err(code) => {
            Feedback::error(window, cx, t!("mcp_action_failed", code = code).to_string());
            false
        }
    }
}
fn heading(value: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(value.into())
}
fn hint(value: impl Into<gpui::SharedString>, cx: &App) -> gpui::Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .whitespace_normal()
        .child(value.into())
}
use permissions::mode_key;
