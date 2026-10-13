use gpui::{App, AppContext as _, Context, Entity, Render, Subscription, Window, px, size};
use gpui_component::{
    Root,
    input::{InputEvent, InputState},
};
use rust_i18n::t;
use std::sync::Arc;
use tiny_shell_mcp::audit::{Filter, Page, Source, Status, Store, Summary};
mod presentation;
mod view;
pub(crate) use presentation::compact_usage_label;
pub(crate) use presentation::usage_label;
mod actions;
mod components;
mod details;
mod filters;
mod inspector;
#[cfg(debug_assertions)]
pub(crate) mod preview;
mod records;
mod retention;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Usage,
    Operations,
    Approvals,
    Settings,
}
pub(crate) struct AuditPanel {
    _owner: Option<Entity<crate::app::TinyShell>>,
    targets: Vec<(String, String)>,
    store: Arc<Store>,
    filter: Filter,
    tab: Tab,
    page: Page,
    summary: Summary,
    status: Status,
    search: Entity<InputState>,
    days: Entity<InputState>,
    capacity: Entity<InputState>,
    patterns: Entity<InputState>,
    loading: bool,
    error: Option<String>,
    notice: Option<String>,
    selection: Option<String>,
    expanded: std::collections::HashSet<String>,
    detail: Page,
    formatted: std::collections::HashMap<String, Arc<String>>,
    _subscriptions: Vec<Subscription>,
    settings_loaded: bool,
}
pub(crate) fn open(filter: Filter, owner: Entity<crate::app::TinyShell>, cx: &mut App) {
    let store = crate::app::ai::service()
        .map(|s| s.audit.clone())
        .or_else(|_| crate::app::mcp::service().map(|s| s.engine.audit.clone()));
    let Ok(store) = store else {
        crate::feedback::Feedback::error_for_owner(&owner, cx, t!("audit_open_error").to_string());
        return;
    };
    let owner_id = owner.read(cx).session_owner_id;
    let error_owner = owner.clone();
    let options = crate::app::platform::auxiliary_window_options(
        cx,
        crate::app::platform::AuxiliaryWindowSpec::new(size(px(1040.), px(760.)))
            .with_min_size(size(px(720.), px(520.)))
            .with_max_ratio(0.9, 0.9),
    );
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(t!("audit_title").as_ref());
        let handle = window.window_handle();
        crate::app::register_auxiliary_window(handle, owner_id);
        window.on_window_should_close(cx, move |_, _| {
            crate::app::deregister_auxiliary_window(handle);
            true
        });
        let panel = cx.new(|cx| AuditPanel::new(store, filter, owner, window, cx));
        cx.new(|cx| Root::new(panel, window, cx))
    });
    if opened.is_err() {
        crate::feedback::Feedback::error_for_owner(
            &error_owner,
            cx,
            t!("audit_open_error").to_string(),
        );
    }
}
impl AuditPanel {
    fn new(
        store: Arc<Store>,
        filter: Filter,
        owner: Entity<crate::app::TinyShell>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let targets = owner
            .read(cx)
            .config
            .sessions()
            .iter()
            .filter(|s| s.connection_type == crate::session::config::ConnectionType::Ssh)
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        Self::with_scope(store, filter, Some(owner), targets, window, cx)
    }
    fn initial_state(
        store: Arc<Store>,
        filter: Filter,
        owner: Option<Entity<crate::app::TinyShell>>,
        targets: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let status = store.status();
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("audit_search").to_string()));
        let days = cx.new(|cx| {
            InputState::new(window, cx).default_value(status.settings.retention_days.to_string())
        });
        let capacity = cx.new(|cx| {
            InputState::new(window, cx).default_value(status.settings.capacity_mb.to_string())
        });
        let patterns = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(status.settings.redact_patterns.join("\n"))
                .auto_grow(3, 7)
        });
        let subscription = cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.filter.before_sequence = None;
                this.selection = None;
                this.load(cx);
            }
            cx.notify();
        });
        let mut panel = Self {
            _owner: owner,
            targets,
            store,
            filter,
            tab: Tab::Operations,
            page: Page::default(),
            summary: Summary::default(),
            status,
            search,
            days,
            capacity,
            patterns,
            loading: false,
            error: None,
            notice: None,
            selection: None,
            expanded: std::collections::HashSet::new(),
            detail: Page::default(),
            formatted: Default::default(),
            _subscriptions: vec![subscription],
            settings_loaded: false,
        };
        if panel.filter.conversation_id.is_some() {
            panel.tab = Tab::Usage;
        }
        panel
    }
    fn with_scope(
        store: Arc<Store>,
        filter: Filter,
        owner: Option<Entity<crate::app::TinyShell>>,
        targets: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::initial_state(store, filter, owner, targets, window, cx);
        panel.load(cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.selection.is_none()
                            && this.tab != Tab::Settings
                            && this.filter.before_sequence.is_none()
                        {
                            this.load(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        panel
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        let store = self.store.clone();
        let mut filter = self.filter.clone();
        filter.limit = 50;
        filter.search = self.search.read(cx).value().trim().to_owned();
        filter.approvals_only = self.tab == Tab::Approvals;
        if self.tab == Tab::Usage {
            filter.source = Some(Source::Ai);
            filter.category = Some(tiny_shell_mcp::audit::Category::Model);
            filter.model_requests_only = true;
        }
        let conversation = filter.conversation_id.clone();
        cx.spawn(async move |this, cx| {
            let page = store.page(filter).await;
            let summary = store.summary(conversation).await;
            let status = store.status();
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                this.status = status;
                match page {
                    Ok(page) => this.page = page,
                    Err(error) => this.error = Some(error.to_string()),
                }
                if let Ok(summary) = summary {
                    this.summary = summary;
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn select(&mut self, id: String, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.selection = Some(id.clone());
        self.expanded.clear();
        self.detail = Page::default();
        self.formatted.clear();
        self.load_detail(id, None, false, cx);
    }
    fn load_detail(
        &mut self,
        id: String,
        cursor: Option<i64>,
        append: bool,
        cx: &mut Context<Self>,
    ) {
        if self.loading {
            return;
        }
        self.loading = true;
        let store = self.store.clone();
        cx.spawn(async move |this, cx| {
            let result = store
                .page(Filter {
                    operation_id: Some(id.clone()),
                    limit: 50,
                    before_sequence: cursor,
                    ..Filter::default()
                })
                .await;
            let formatted = if let Ok(page) = &result {
                let events = page.events.clone();
                cx.background_executor()
                    .spawn(async move {
                        events
                            .into_iter()
                            .map(|event| {
                                (
                                    event.id.clone(),
                                    Arc::new(
                                        serde_json::to_string_pretty(&event).unwrap_or_default(),
                                    ),
                                )
                            })
                            .collect::<std::collections::HashMap<_, _>>()
                    })
                    .await
            } else {
                Default::default()
            };
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                if this.selection.as_ref() == Some(&id) {
                    match result {
                        Ok(page) => {
                            if append {
                                this.detail.events.extend(page.events);
                                if this.detail.events.len() > 200 {
                                    this.detail.events.drain(..this.detail.events.len() - 200);
                                }
                                this.detail.next = page.next;
                                this.detail.scanned += page.scanned;
                            } else {
                                this.detail = page;
                                this.formatted.clear();
                            }
                            this.formatted.extend(formatted);
                            let kept = this
                                .detail
                                .events
                                .iter()
                                .map(|e| e.id.clone())
                                .collect::<std::collections::HashSet<_>>();
                            this.formatted.retain(|id, _| kept.contains(id));
                            this.expanded.retain(|id| kept.contains(id));
                        }
                        Err(error) => this.error = Some(error.to_string()),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
}
impl Render for AuditPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        use gpui::{ParentElement as _, Styled as _, div};
        // All tabs share the window's confirmation layer, including early-return detail views.
        div()
            .relative()
            .size_full()
            .child(self.view(window, cx))
            .children(Root::render_dialog_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{IntoElement as _, TestAppContext, VisualTestContext};
    use gpui_component::WindowExt as _;

    #[gpui::test]
    fn confirmation_controls_render_in_audit_details_and_settings(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let temporary_root = std::env::temp_dir();
        let directory =
            temporary_root.join(format!("tiny-shell-audit-ui-test-{}", uuid::Uuid::new_v4()));
        let store = Store::with_key(directory.clone(), [52; 32]);
        let fixture = store.clone();
        let handle = cx.open_window(size(px(760.), px(600.)), |window, cx| {
            let panel = cx.new(|cx| {
                // This regression exercises modal rendering/cancellation. Actor I/O is covered
                // separately and cannot wake GPUI's deterministic scheduler from its OS thread.
                AuditPanel::initial_state(fixture, Filter::default(), None, vec![], window, cx)
            });
            Root::new(panel, window, cx)
        });
        let root = handle.root(cx).unwrap();
        let panel = root.read_with(cx, |root, _| {
            root.view().clone().downcast::<AuditPanel>().unwrap()
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.draw(
            gpui::point(px(0.), px(0.)),
            size(px(760.), px(600.)),
            |_, _| root.into_any_element(),
        );
        for tab in [Tab::Operations, Tab::Settings] {
            panel.update_in(&mut visual, |panel, window, cx| {
                panel.tab = tab;
                if tab == Tab::Operations {
                    panel.selection = Some("synthetic-operation".into());
                    panel.confirm_export(window, cx);
                } else {
                    panel.selection = None;
                    panel.confirm_clear(window, cx);
                }
                cx.notify();
            });
            // Settle the shared confirmation component's wall-clock entrance animation.
            std::thread::sleep(std::time::Duration::from_millis(300));
            visual.update(|window, _| window.refresh());
            visual.run_until_parked();
            assert!(
                visual.debug_bounds("confirmation-submit").is_some(),
                "audit confirmation must be visible"
            );
            let cancel = visual.debug_bounds("confirmation-cancel").unwrap();
            visual.simulate_click(cancel.center(), gpui::Modifiers::none());
            visual.run_until_parked();
            assert!(
                !visual.update(|window, cx| window.has_active_dialog(cx)),
                "cancel must close the confirmation without exporting or clearing"
            );
        }
        visual.update(|window, _| window.remove_window());
        futures::executor::block_on(store.close()).unwrap();
        let resolved = directory.canonicalize().unwrap();
        assert_eq!(
            resolved.parent(),
            Some(temporary_root.canonicalize().unwrap().as_path())
        );
        std::fs::remove_dir_all(resolved).unwrap();
    }
}
