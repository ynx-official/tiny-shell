use std::collections::{HashMap, HashSet};

use gpui::{
    AnyWindowHandle, App, Context, Entity, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Window, px,
};
use gpui_component::WindowExt;
use rust_i18n::t;
use uuid::Uuid;

use crate::{
    PaneLayout, SelectorEntry, TabGroup, TinyShell,
    app::{
        IncomingTabDrag, SystemInfoTab,
        constants::{DEFAULT_COLS, DEFAULT_ROWS},
        tab_drag::{
            DropIntent, cursor_inside_viewport, reorder_index_at_x, should_close_empty_source,
            should_offer_detach,
        },
    },
    backend::{local, ssh},
    session::config::{AuthMethod, Session},
    terminal::{BackendCommand, RenderSnapshot, TabKind, TerminalTab},
};

pub(crate) struct GroupTransfer {
    group: TabGroup,
    group_index: usize,
    tabs: Vec<(usize, TerminalTab)>,
    sftp_handles: HashMap<String, crate::sftp::SftpHandle>,
    route_ids: Vec<String>,
    active_tab: Option<String>,
    was_active_group: bool,
}

impl TinyShell {
    // ── 系统信息标签 ──

    pub(crate) fn open_system_info_tab(&mut self, cx: &mut Context<Self>) {
        let Some(source_tab_id) = self.system_tab_id.clone().or_else(|| {
            self.active_tab.as_ref().and_then(|active_id| {
                self.tabs
                    .iter()
                    .find(|tab| tab.id == *active_id && tab.kind == TabKind::Ssh)
                    .map(|tab| tab.id.clone())
            })
        }) else {
            return;
        };

        let info_id = if let Some(existing) = self
            .system_info_tabs
            .iter()
            .find(|tab| tab.source_tab_id == source_tab_id)
        {
            existing.id.clone()
        } else {
            let host_title = self
                .tabs
                .iter()
                .find(|tab| tab.id == source_tab_id)
                .map(|tab| tab.title.clone())
                .unwrap_or_else(|| t!("system_information").to_string());
            let id = Uuid::new_v4().to_string();
            self.system_info_tabs.push(SystemInfoTab {
                id: id.clone(),
                source_tab_id: source_tab_id.clone(),
                title: format!("{} · {}", host_title, t!("system_information")),
            });
            id
        };

        self.active_tab = Some(source_tab_id.clone());
        self.system_tab_id = Some(source_tab_id);
        self.active_system_info_tab = Some(info_id);
        self.home_page_open = false;
        self.request_active_system_snapshot();
        cx.notify();
    }

    pub(crate) fn close_system_info_tab(&mut self, id: String, cx: &mut Context<Self>) {
        let was_active = self.active_system_info_tab.as_deref() == Some(id.as_str());
        self.system_info_tabs.retain(|tab| tab.id != id);
        if was_active {
            self.active_system_info_tab = None;
        }
        cx.notify();
    }

    // ── 本地终端 ──

    pub(crate) fn open_local(&mut self, cx: &mut Context<Self>) {
        self.active_system_info_tab = None;
        self.home_page_open = false;
        let ordinal = self.next_tab_group_ordinal;
        self.next_tab_group_ordinal += 1;
        let id = Uuid::new_v4().to_string();
        let events = self.backend_events_sender(cx);
        match local::spawn_local_terminal(id.clone(), DEFAULT_COLS, DEFAULT_ROWS, events.clone()) {
            Ok(backend) => {
                let title = if cfg!(windows) {
                    t!("local_terminal_powershell").to_string()
                } else {
                    t!("local_terminal").to_string()
                };
                let mut tab = TerminalTab::new_local(id.clone(), title.clone(), backend, events);
                tab.resize(DEFAULT_COLS, DEFAULT_ROWS);
                self.tabs.push(tab);
                self.register_backend_route(id.clone(), cx);
                self.active_tab = Some(id.clone());
                self.pane_root = PaneLayout::Single(id.clone());
                self.focused_pane_path = vec![];
                let group_id = Uuid::new_v4().to_string();
                self.tab_groups.push(TabGroup {
                    id: group_id.clone(),
                    ordinal,
                    title,
                    pane_root: PaneLayout::Single(id),
                    sftp: None,
                });
                self.active_group = Some(group_id);
                self.tabs_scroll_handle.scroll_to_item(self.tabs.len() - 1);
                self.status = t!("local_terminal_opened").into();
            }
            Err(err) => {
                self.status = t!("local_terminal_open_failed", err = format!("{err:#}")).into();
            }
        }
        cx.notify();
    }

    // ── SSH 连接表单 ──

    pub(crate) fn connect_ssh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        tracing::info!("[ui] user initiating new ssh connection from form");
        let session_name = self.session_name_input.read(cx).value().trim().to_string();
        let host = self.host_input.read(cx).value().trim().to_string();
        let port = self
            .port_input
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .unwrap_or(22);
        let user = self.user_input.read(cx).value().trim().to_string();
        let password = self.password_input.read(cx).value().to_string();
        let key_path = self.key_path_input.read(cx).value().trim().to_string();
        let key_inline = self.key_inline_input.read(cx).value().to_string();
        let passphrase = self.passphrase_input.read(cx).value().to_string();

        if host.is_empty() || user.is_empty() {
            self.status = t!("host_and_user_required").into();
            cx.notify();
            return;
        }

        if self.ssh_proxy_type != "none" {
            let proxy_host = self.proxy_host_input.read(cx).value().trim().to_string();
            let proxy_port_str = self.proxy_port_input.read(cx).value().trim().to_string();
            let proxy_port = proxy_port_str.parse::<u16>().ok();
            if proxy_host.is_empty() || proxy_port.is_none() {
                self.status = t!("ssh_editor_proxy_required").into();
                cx.notify();
                return;
            }
        }

        let name = if session_name.is_empty() {
            host.clone()
        } else {
            session_name
        };
        let existing_id = self.editing_session_id.clone();
        let existing_last_used = existing_id
            .as_deref()
            .and_then(|id| self.config.get(id))
            .and_then(|session| session.last_used.clone());
        let existing_group = existing_id
            .as_deref()
            .and_then(|id| self.config.get(id))
            .and_then(|session| session.group.clone());

        let mut session = match self.ssh_auth_method {
            AuthMethod::Password => Session::password(host, port, user, password),
            AuthMethod::Key | AuthMethod::KeyPending => {
                // If a managed key is selected, reference it by id.
                // The actual key content is resolved at connection time.
                if let Some(mk_id) = &self.managed_key_selected {
                    let mut s = Session::key(
                        host.clone(),
                        port,
                        user.clone(),
                        String::new(),
                        String::new(),
                        String::new(),
                    );
                    s.managed_key_id = Some(mk_id.clone());
                    s
                } else {
                    Session::key(host, port, user, key_path, key_inline, passphrase)
                }
            }
            AuthMethod::Config => {
                // Force key_inline to empty — config mode never uses inline key content.
                // The backend will try default keys from ~/.ssh/ if no explicit key path is set.
                let mut session =
                    Session::key(host, port, user, key_path, String::new(), String::new());
                session.auth = AuthMethod::Config;
                session
            }
        };
        session.name = name;
        if let Some(id) = existing_id {
            session.id = id;
        }
        session.last_used = existing_last_used;
        session.group = self.session_group_selection.clone().or(existing_group);
        session.proxy_type = self.ssh_proxy_type.clone();
        session.proxy_host = self.proxy_host_input.read(cx).value().trim().to_string();
        session.proxy_port = self
            .proxy_port_input
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .ok();
        session.proxy_user = self.proxy_user_input.read(cx).value().trim().to_string();
        session.proxy_password = self.proxy_password_input.read(cx).value().to_string();
        self.config.upsert(session.clone());
        if let Err(err) = crate::app::config_persistence::save_full(&self.config) {
            tracing::warn!("failed to save config: {err:#}");
        }

        self.open_ssh_session(session, cx);
        self.editing_session_id = None;
        self.session_group_selection = self.connection_group_filter.clone();
        self.active_dialog = None;
        window.close_dialog(cx);
        cx.notify();
    }

    // ── SSH 对话框 ──

    pub(crate) fn open_new_ssh_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let group = self.connection_group_parent.take();
        let owner = cx.entity();
        window.defer(cx, move |_, cx| {
            crate::app::connection_manager::ssh_editor_window::open(
                owner,
                crate::app::connection_manager::ssh_editor_window::SshEditorRequest::New {
                    group,
                    prefill: None,
                },
                cx,
            );
        });
    }

    pub(crate) fn open_ssh_address_dialog(
        &mut self,
        address: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let session = crate::session::connection_catalog::parse_session_address(address)?;
        let group = self.connection_group_parent.take();
        let owner = cx.entity();
        window.defer(cx, move |_, cx| {
            crate::app::connection_manager::ssh_editor_window::open(
                owner,
                crate::app::connection_manager::ssh_editor_window::SshEditorRequest::New {
                    group,
                    prefill: Some(session),
                },
                cx,
            );
        });
        Ok(())
    }

    // ── 选择器 ──

    pub(crate) fn selector_entries(&self) -> Vec<SelectorEntry> {
        let mut entries = vec![SelectorEntry::Local, SelectorEntry::NewSsh];
        entries.extend(
            self.config
                .sessions()
                .iter()
                .map(|session| SelectorEntry::Saved(session.id.clone())),
        );
        entries
    }

    pub(crate) fn default_selector_index(&self) -> usize {
        if self.config.sessions().is_empty() {
            0
        } else {
            2
        }
    }

    pub(crate) fn move_selector_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        let entries = self.selector_entries();
        if entries.is_empty() {
            return;
        }
        let current = self.selector_selection.min(entries.len().saturating_sub(1)) as i32;
        let next = (current + delta).clamp(0, entries.len() as i32 - 1) as usize;
        if next != self.selector_selection {
            self.selector_selection = next;
            if next >= 2 {
                self.selector_scroll_handle.scroll_to_item(next - 2);
            }
            cx.notify();
        }
    }

    pub(crate) fn activate_selector_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.selector_entries();
        let Some(entry) = entries.get(self.selector_selection).cloned() else {
            return;
        };

        self.active_dialog = None;
        match entry {
            SelectorEntry::Local => {
                self.open_local(cx);
                window.close_dialog(cx);
            }
            SelectorEntry::NewSsh => {
                window.close_dialog(cx);
                self.open_new_ssh_dialog(window, cx);
            }
            SelectorEntry::Saved(session_id) => {
                self.connect_saved_session(session_id, window, cx);
                window.close_dialog(cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn on_selector_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.to_ascii_lowercase();
        match key.as_str() {
            "up" | "arrowup" => {
                self.move_selector_selection(-1, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "down" | "arrowdown" => {
                self.move_selector_selection(1, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "enter" | "return" => {
                self.activate_selector_selection(window, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    // ── 标签管理 ──

    #[allow(dead_code)]
    pub(crate) fn activate_tab(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        // Save current group state
        if let Some(group_id) = self.active_group.clone() {
            if let Some(group) = self.tab_groups.iter_mut().find(|g| g.id == group_id) {
                group.pane_root = self.pane_root.clone();
            }
        }
        self.active_tab = Some(id.clone());
        // Find which group this tab belongs to and restore its pane_root
        let tab_group = self
            .tab_groups
            .iter_mut()
            .find(|g| g.pane_root.contains(&id));
        if let Some(group) = tab_group {
            self.pane_root = group.pane_root.clone();
            self.active_group = Some(group.id.clone());
            // Focus the activated tab in the pane tree
            self.focus_pane_with_id(id.clone());
        } else {
            self.pane_root = PaneLayout::Single(id.clone());
            self.focused_pane_path = vec![];
        }
        if let Some(index) = self.tabs.iter().position(|t| t.id == id) {
            self.tabs_scroll_handle.scroll_to_item(index);
        }
        if self.tabs.iter().any(|t| t.id == id) {
            if let Some(session_id) = self.active_session_id() {
                if let Some(index) = self
                    .config
                    .sessions()
                    .iter()
                    .position(|s| s.id == session_id)
                {
                    self.saved_scroll_handle.scroll_to_item(index);
                }
            }
        }
        self.focus_handle.focus(window, cx);
        self.sync_system_tab_to_active_group();
        cx.notify();
    }

    pub(crate) fn close_tab(&mut self, id: String, cx: &mut Context<Self>) {
        let closing_sftp_group = self
            .tab_groups
            .iter()
            .find(|group| group.pane_root.contains(&id))
            .filter(|group| group.pane_root.total_panes() <= 1)
            .filter(|group| self.sftp_handles.contains_key(&group.id))
            .map(|group| group.id.clone());

        if let Some(session_id) = closing_sftp_group
            && !crate::app::sftp_editor_window::request_session_close(
                &session_id,
                id.clone(),
                cx.entity(),
                cx,
            )
        {
            return;
        }

        self.handle_tab_close(id);
        cx.notify();
    }

    pub(crate) fn disconnect_tab_group(&mut self, group_id: &str, cx: &mut Context<Self>) {
        let Some(group) = self.tab_groups.iter().find(|group| group.id == group_id) else {
            return;
        };
        let tab_ids: Vec<String> = group
            .pane_root
            .tab_ids()
            .iter()
            .map(|tab_id| (*tab_id).to_string())
            .collect();

        if let Some(handle) = self.sftp_handles.remove(group_id) {
            handle.close();
        }

        for tab_id in tab_ids {
            if let Some(tab) = self
                .tabs
                .iter_mut()
                .find(|tab| tab.id == tab_id && tab.kind == TabKind::Ssh)
            {
                if tab.connected {
                    tab.connected = false;
                    tab.status = rust_i18n::t!("tab_manually_disconnected").into();
                    tab.disconnected_reason =
                        Some(rust_i18n::t!("tab_manually_disconnected").to_string());
                    tab.send_backend(BackendCommand::Close);
                }
            }
        }

        self.status = rust_i18n::t!("tab_manually_disconnected").into();
        cx.notify();
    }

    pub(crate) fn handle_tab_close(&mut self, id: String) {
        self.terminal_completions.remove(&id);
        let removed_active_info = self.system_info_tabs.iter().any(|tab| {
            tab.source_tab_id == id
                && self.active_system_info_tab.as_deref() == Some(tab.id.as_str())
        });
        self.system_info_tabs.retain(|tab| tab.source_tab_id != id);
        if removed_active_info {
            self.active_system_info_tab = None;
        }
        let group_ix = self
            .tab_groups
            .iter()
            .position(|g| g.pane_root.contains(&id));
        let Some(ref group) = group_ix.map(|i| self.tab_groups[i].clone()) else {
            // Fallback: find and close individual tab
            tracing::info!(
                "[handle_tab_close] no group found for tab '{}', closing individually",
                id
            );
            if let Some(ix) = self.tabs.iter().position(|tab| tab.id == id) {
                self.tabs[ix].send_backend(BackendCommand::Close);
                self.tabs.remove(ix);
            }
            self.monitoring.remote_system_snapshots.remove(&id);
            return;
        };

        let pane_ids = group.pane_root.tab_ids();
        let pane_ids_str = pane_ids.to_vec();
        let is_group_close = pane_ids.len() <= 1;
        tracing::info!(
            "[handle_tab_close] id='{}' group_panes={:?} is_group_close={}",
            id,
            pane_ids_str,
            is_group_close
        );

        let was_active = self.active_tab.as_deref() == Some(id.as_str());
        let mut next_active_id = None;
        if was_active {
            let tabs_in_group = group.pane_root.tab_ids();
            if let Some(pos) = tabs_in_group.iter().position(|&s| s == id.as_str()) {
                if pos > 0 {
                    next_active_id = Some(tabs_in_group[pos - 1].to_string());
                } else if pos + 1 < tabs_in_group.len() {
                    next_active_id = Some(tabs_in_group[pos + 1].to_string());
                }
            }
            if next_active_id.is_none() {
                // Find next group's active tab
                let all_groups = &self.tab_groups;
                if let Some(pos) = all_groups.iter().position(|g| g.id == group.id) {
                    if pos > 0 {
                        next_active_id = all_groups[pos - 1]
                            .pane_root
                            .tab_ids()
                            .first()
                            .copied()
                            .map(String::from);
                    } else if pos + 1 < all_groups.len() {
                        next_active_id = all_groups[pos + 1]
                            .pane_root
                            .tab_ids()
                            .first()
                            .copied()
                            .map(String::from);
                    }
                }
            }
        }
        if is_group_close {
            // Close all tabs in the group
            let tab_ids: Vec<String> = group
                .pane_root
                .tab_ids()
                .iter()
                .map(|s| s.to_string())
                .collect();
            for tab_id in &tab_ids {
                if let Some(ix) = self.tabs.iter().position(|tab| tab.id == *tab_id) {
                    self.tabs[ix].send_backend(BackendCommand::Close);
                    self.tabs.retain(|t| t.id != *tab_id);
                }
                self.monitoring.remote_system_snapshots.remove(tab_id);
            }
            if let Some(handle) = self.sftp_handles.remove(&group.id) {
                handle.close();
            }
            if let Some(group_index) = group_ix {
                self.tab_groups.remove(group_index);
            }
            self.pane_root.remove_tab(&id);
        } else {
            // Just remove this tab from the group
            if let Some(ix) = self.tabs.iter().position(|tab| tab.id == id) {
                self.tabs[ix].send_backend(BackendCommand::Close);
                self.tabs.retain(|t| t.id != id);
            }
            self.monitoring.remote_system_snapshots.remove(&id);
            if let Some(g) = self
                .tab_groups
                .iter_mut()
                .find(|g| g.pane_root.contains(&id))
            {
                g.pane_root.remove_tab(&id);
            }
            self.pane_root.remove_tab(&id);
            self.sync_pane_root_to_group();
        }

        self.system_info_tabs
            .retain(|info| self.tabs.iter().any(|tab| tab.id == info.source_tab_id));
        if self
            .active_system_info_tab
            .as_ref()
            .is_some_and(|active_id| {
                !self
                    .system_info_tabs
                    .iter()
                    .any(|info| &info.id == active_id)
            })
        {
            self.active_system_info_tab = None;
        }

        if self.tabs.is_empty() || self.tab_groups.is_empty() {
            self.pane_root = PaneLayout::Single(String::new());
            self.focused_pane_path = vec![];
            self.active_tab = None;
            self.active_group = None;
            self.system_info_tabs.clear();
            self.active_system_info_tab = None;
            self.tab_groups.clear();
            self.tabs.clear();
            self.system_tab_id = None;
            self.monitoring.cpu_history.clear();
            self.monitoring.net_rx_history.clear();
            self.monitoring.net_tx_history.clear();
            self.monitoring.selected_network_interface = None;
            self.monitoring.network_interface_histories.clear();
            self.monitoring.system_status = None;
            self.monitoring.remote_system_snapshots.clear();
            for (_, handle) in self.sftp_handles.drain() {
                handle.close();
            }
            self.home_page_open = true;
            return;
        }

        if was_active
            || self
                .active_tab
                .as_ref()
                .is_some_and(|active_id| !self.tabs.iter().any(|tab| &tab.id == active_id))
        {
            // Activate next available pane
            let new_id = next_active_id.or_else(|| {
                self.pane_root
                    .tab_ids()
                    .first()
                    .copied()
                    .map(String::from)
                    .or_else(|| self.tabs.first().map(|t| t.id.clone()))
            });
            if let Some(new_id) = new_id {
                self.active_tab = Some(new_id.clone());
                if let Some(g) = self
                    .tab_groups
                    .iter()
                    .find(|g| g.pane_root.contains(&new_id))
                {
                    self.active_group = Some(g.id.clone());
                    self.pane_root = g.pane_root.clone();
                }
                self.focus_pane_with_id(new_id);
            }
        } else {
            // Pane root structure may have changed (e.g. sibling removed), recalc path
            if let Some(active_id) = self.active_tab.clone() {
                self.focus_pane_with_id(active_id);
            }
        }
        self.sync_system_tab_to_active_group();
    }

    pub(crate) fn focus_terminal(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // If the search bar is visible and the click is inside it, let the
        // search bar handle the event instead of switching pane focus.
        if self.search_active {
            if let Some(bounds) = self.search_bar_bounds {
                if bounds.contains(&event.position) {
                    return;
                }
            }
        }
        self.focus_handle.focus(window, cx);
        // Check if click is in a different pane and focus it
        let click_pos = event.position;
        let current_active = self.active_tab.clone();
        let clicked_tab_id = self.terminal_bounds.iter().find_map(|(id, bounds)| {
            if bounds.contains(&click_pos) {
                Some(id.clone())
            } else {
                None
            }
        });
        if let Some(tab_id) = clicked_tab_id {
            if current_active.as_deref() != Some(tab_id.as_str()) {
                self.focus_pane_with_id(tab_id.clone());
                cx.notify();
            }
        }
        if event.button == MouseButton::Left {
            if event.modifiers.platform {
                if let Some((row, col, _side)) = self.terminal_grid_point_and_side(event.position) {
                    if let Some(snapshot) = self.active_snapshot() {
                        if let Some((url, _)) = crate::terminal::highlight::find_url_at_cell(
                            &snapshot.cells,
                            snapshot.rows,
                            row,
                            col,
                        ) {
                            let _ = open::that(&url);
                            return;
                        }
                    }
                }
            }
            self.begin_terminal_selection(event, cx);
        }
        cx.notify();
    }

    pub(crate) fn active_snapshot(&self) -> Option<RenderSnapshot> {
        self.active_tab
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|t| &t.id == id))
            .map(|t| t.render_snapshot(self.config.keyword_highlight()))
    }

    pub(crate) fn active_kind(&self) -> Option<TabKind> {
        self.active_tab
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|t| &t.id == id))
            .map(|tab| tab.kind)
    }

    pub(crate) fn active_session_id(&self) -> Option<&str> {
        self.active_tab
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.id == id))
            .and_then(|tab| tab.session.as_ref())
            .map(|session| session.id.as_str())
    }

    // ── 面板分割 ──

    pub(crate) fn split_current_pane(&mut self, direction: &str, cx: &mut Context<Self>) {
        tracing::info!(
            "[split] direction={} pane_root={:?} focused_path={:?} active_tab={:?} tabs={}",
            direction,
            self.pane_root,
            self.focused_pane_path,
            self.active_tab,
            self.tabs.len(),
        );
        let current_id = match self.pane_root.focused_tab_id(&self.focused_pane_path) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => return,
        };
        let (current_kind, current_session) = match self.tabs.iter().find(|t| t.id == current_id) {
            Some(tab) => (tab.kind, tab.session.clone()),
            None => return,
        };
        let new_id = Uuid::new_v4().to_string();
        let events = self.backend_events_sender(cx);
        let proxy_config = self.config.clone();
        self.register_backend_route(new_id.clone(), cx);
        let mut tab = match current_kind {
            TabKind::Local => {
                match local::spawn_local_terminal(
                    new_id.clone(),
                    DEFAULT_COLS,
                    DEFAULT_ROWS,
                    events.clone(),
                ) {
                    Ok(backend) => TerminalTab::new_local(
                        new_id.clone(),
                        t!("local_terminal").to_string(),
                        backend,
                        events.clone(),
                    ),
                    Err(err) => {
                        self.status = t!("split_failed", err = format!("{err:#}")).into();
                        cx.notify();
                        return;
                    }
                }
            }
            TabKind::Ssh => {
                let Some(session) = current_session else {
                    self.status = t!("split_no_session").into();
                    cx.notify();
                    return;
                };
                let backend = ssh::spawn_ssh_terminal(
                    self.runtime.handle(),
                    new_id.clone(),
                    session.clone(),
                    proxy_config.clone(),
                    DEFAULT_COLS,
                    DEFAULT_ROWS,
                    events.clone(),
                );
                let sftp_handle = crate::sftp::spawn_sftp(
                    self.runtime.handle(),
                    new_id.clone(),
                    session.clone(),
                    proxy_config,
                    events.clone(),
                );
                self.sftp_handles.insert(new_id.clone(), sftp_handle);
                TerminalTab::new_ssh(new_id.clone(), &session, backend, events)
            }
        };
        tab.resize(DEFAULT_COLS, DEFAULT_ROWS);
        // Do NOT add to tab_groups — pane stays within the existing group
        self.tabs.push(tab);
        // Do NOT scroll tab bar or add tab bar entry

        let current_pane = PaneLayout::Single(current_id);
        let new_pane = PaneLayout::Single(new_id.clone());

        let split_layout = match direction {
            "left" | "right" => {
                let children = match direction {
                    "left" => vec![new_pane, current_pane],
                    _ => vec![current_pane, new_pane],
                };
                PaneLayout::Vertical(children, 0.5)
            }
            "up" | "down" => {
                let children = match direction {
                    "up" => vec![new_pane, current_pane],
                    _ => vec![current_pane, new_pane],
                };
                PaneLayout::Horizontal(children, 0.5)
            }
            _ => return,
        };

        self.pane_root
            .replace_at(&self.focused_pane_path, split_layout);
        self.sync_pane_root_to_group();
        // Update focused_pane_path: the new pane is at the indicated child index
        let parent_path = self.focused_pane_path.clone();
        let mut new_full_path = parent_path;
        if direction == "right" || direction == "down" {
            new_full_path.push(1);
        } else {
            new_full_path.push(0);
        }
        self.focused_pane_path = new_full_path;
        self.active_tab = Some(new_id);
        self.status = "pane split".into();
        tracing::info!(
            "[split] DONE: pane_root={:?} focused_path={:?} active_tab={:?} tabs={}",
            self.pane_root,
            self.focused_pane_path,
            self.active_tab,
            self.tabs.len(),
        );
        cx.notify();
    }

    pub(crate) fn focus_adjacent_pane(&mut self, direction: &str) {
        if self.focused_pane_path.is_empty() {
            return;
        }
        let path = self.focused_pane_path.clone();
        if let Some(new_path) = Self::find_adjacent_pane(&self.pane_root, &path, direction) {
            self.focused_pane_path = new_path;
            if let Some(id) = self.pane_root.focused_tab_id(&self.focused_pane_path) {
                let id_owned = id.to_string();
                let changed = self.active_tab.as_deref() != Some(id_owned.as_str());
                self.active_tab = Some(id_owned);
                // Clear stale search state when switching to a different pane.
                if changed && self.search_active {
                    self.search_query.clear();
                    self.search_matches.clear();
                    self.search_current = 0;
                    self.search_target_tab = None;
                }
            }
        }
    }

    fn first_leaf_path(layout: &PaneLayout) -> Vec<usize> {
        match layout {
            PaneLayout::Single(_) => vec![],
            PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                let mut path = vec![0];
                path.extend(Self::first_leaf_path(&children[0]));
                path
            }
        }
    }

    fn leaf_at_index(layout: &PaneLayout, index: usize) -> Vec<usize> {
        match layout {
            PaneLayout::Single(_) => vec![],
            PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                if children.is_empty() {
                    return vec![];
                }
                let i = index.min(children.len() - 1);
                let mut path = vec![i];
                path.extend(Self::first_leaf_path(&children[i]));
                path
            }
        }
    }

    fn find_adjacent_pane(
        layout: &PaneLayout,
        path: &[usize],
        direction: &str,
    ) -> Option<Vec<usize>> {
        if path.is_empty() {
            return None;
        }
        match layout {
            PaneLayout::Single(_) => None,
            PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                let is_horizontal = matches!(layout, PaneLayout::Horizontal(_, _));
                let idx = path[0];

                // Does this split level match the movement direction?
                let vert = direction == "up" || direction == "down";
                let horiz = direction == "left" || direction == "right";
                // PaneLayout::Horizontal renders as v_flex (vertical stack),
                // PaneLayout::Vertical renders as h_flex (horizontal row).
                // So for a Vertical (h_flex), h/l moves between children;
                // for a Horizontal (v_flex), j/k moves between children.
                let moves_in_this_split = (vert && is_horizontal) || (horiz && !is_horizontal);

                if path.len() == 1 {
                    // Direct child level
                    if moves_in_this_split {
                        let delta: i32 = if direction == "up" || direction == "left" {
                            -1
                        } else {
                            1
                        };
                        let new_idx = idx as i32 + delta;
                        if new_idx >= 0 && (new_idx as usize) < children.len() {
                            let mut path = vec![new_idx as usize];
                            path.extend(Self::first_leaf_path(&children[new_idx as usize]));
                            Some(path)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    // Recurse into child first
                    if let Some(mut child_path) =
                        Self::find_adjacent_pane(&children[idx], &path[1..], direction)
                    {
                        child_path.insert(0, idx);
                        Some(child_path)
                    } else if moves_in_this_split {
                        // Try sibling at this level
                        let delta: i32 = if direction == "up" || direction == "left" {
                            -1
                        } else {
                            1
                        };
                        let new_idx = idx as i32 + delta;
                        if new_idx >= 0 && (new_idx as usize) < children.len() {
                            let inner_idx = *path.get(1).unwrap_or(&0);
                            let mut path = vec![new_idx as usize];
                            path.extend(Self::leaf_at_index(
                                &children[new_idx as usize],
                                inner_idx,
                            ));
                            Some(path)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            }
        }
    }

    // ── 组管理 ──

    pub(crate) fn activate_group(
        &mut self,
        group_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.home_page_open = false;
        self.active_system_info_tab = None;
        // Save current group state
        if let Some(current_group_id) = self.active_group.clone() {
            if let Some(group) = self
                .tab_groups
                .iter_mut()
                .find(|g| g.id == current_group_id)
            {
                group.pane_root = self.pane_root.clone();
            }
        }
        // Load new group state
        if let Some(group) = self.tab_groups.iter().find(|g| g.id == group_id) {
            self.pane_root = group.pane_root.clone();
            self.active_group = Some(group_id);
            let ids = group.pane_root.tab_ids();
            if let Some(&first_id) = ids.first() {
                self.active_tab = Some(first_id.to_string());
                self.focus_pane_with_id(first_id.to_string());
            }
            self.focus_handle.focus(window, cx);
        }
        self.sync_system_tab_to_active_group();
        cx.notify();
    }

    pub(crate) fn sync_pane_root_to_group(&mut self) {
        if let Some(group_id) = self.active_group.clone() {
            if let Some(group) = self.tab_groups.iter_mut().find(|g| g.id == group_id) {
                group.pane_root = self.pane_root.clone();
            }
        }
    }

    pub(crate) fn sync_system_tab_to_active_group(&mut self) {
        let mut group_ssh_tabs = vec![];
        if let Some(group_id) = &self.active_group {
            if let Some(group) = self.tab_groups.iter().find(|g| g.id == *group_id) {
                let ids = group.pane_root.tab_ids();
                for id in ids {
                    if let Some(tab) = self.tabs.iter().find(|t| t.id == *id) {
                        if tab.kind == TabKind::Ssh {
                            group_ssh_tabs.push(tab.id.clone());
                        }
                    }
                }
            }
        }

        // Check if current system_tab_id is valid in this group
        let is_current_valid = self
            .system_tab_id
            .as_ref()
            .is_some_and(|id| group_ssh_tabs.contains(id));

        if !is_current_valid {
            let new_id = group_ssh_tabs.into_iter().next();
            if self.system_tab_id != new_id {
                self.system_tab_id = new_id;
                self.monitoring.cpu_history.clear();
                self.monitoring.net_rx_history.clear();
                self.monitoring.net_tx_history.clear();
                self.monitoring.selected_network_interface = None;
                self.monitoring.network_interface_histories.clear();
                self.monitoring.remote_sample_in_flight = None;
                if self.system_tab_id.is_none() {
                    self.monitoring.system_status =
                        Some("monitored session closed".to_string().into());
                } else {
                    self.monitoring.system_status = None;
                    self.monitoring.system = self
                        .system_tab_id
                        .as_ref()
                        .and_then(|tab_id| self.monitoring.remote_system_snapshots.get(tab_id))
                        .cloned()
                        .unwrap_or_default();
                    self.monitoring.animated_cpu_percent = self.monitoring.system.cpu_percent;
                    self.monitoring.animated_mem_percent = self.monitoring.system.mem_percent;
                    self.monitoring.animated_swap_percent = self.monitoring.system.swap_percent;
                }
                self.request_active_system_snapshot();
            }
        }
    }

    pub(crate) fn start_drag_split(
        &mut self,
        parent_path: Vec<usize>,
        child_index: usize,
        event: &MouseDownEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.dragging_splitter = Some((parent_path, child_index));
        self.drag_split_origin = Some(event.position);
    }

    pub(crate) fn on_split_drag_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        let Some((ref parent_path, child_idx)) = self.dragging_splitter.clone() else {
            return;
        };
        let Some(origin) = self.drag_split_origin else {
            return;
        };
        let total = window.viewport_size();
        let is_horizontal = Self::is_layout_horizontal_at(&self.pane_root, parent_path);
        let delta: f32 = if is_horizontal {
            (event.position.y - origin.y).into()
        } else {
            (event.position.x - origin.x).into()
        };
        let total_size: f32 = if is_horizontal {
            total.height.into()
        } else {
            total.width.into()
        };
        if delta.abs() < 5.0 {
            return; // dead zone
        }
        let ratio_delta = delta / total_size;
        Self::adjust_split_ratio(&mut self.pane_root, parent_path, child_idx, ratio_delta);
        self.drag_split_origin = Some(event.position);
        self.sync_pane_root_to_group();
    }

    pub(crate) fn end_drag_split(&mut self) {
        self.dragging_splitter = None;
        self.drag_split_origin = None;
    }

    fn is_layout_horizontal_at(layout: &PaneLayout, path: &[usize]) -> bool {
        match (layout, path) {
            (PaneLayout::Horizontal(_, _), []) => true,
            (PaneLayout::Vertical(_, _), []) => false,
            (
                PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _),
                [first, rest @ ..],
            ) => children
                .get(*first)
                .is_some_and(|c| Self::is_layout_horizontal_at(c, rest)),
            _ => false,
        }
    }

    fn adjust_split_ratio(layout: &mut PaneLayout, path: &[usize], _child_idx: usize, delta: f32) {
        if let PaneLayout::Horizontal(children, ratio) | PaneLayout::Vertical(children, ratio) =
            layout
        {
            if path.is_empty() {
                *ratio = (*ratio + delta).clamp(0.1, 0.9);
            } else {
                let Some((&first, rest)) = path.split_first() else {
                    return;
                };
                if let Some(child) = children.get_mut(first) {
                    Self::adjust_split_ratio(child, rest, _child_idx, delta);
                }
            }
        }
    }

    pub(crate) fn focus_pane_with_id(&mut self, tab_id: String) {
        // Find the path to the given tab_id in the pane tree
        fn find_path(layout: &PaneLayout, target: &str, path: &mut Vec<usize>) -> bool {
            match layout {
                PaneLayout::Single(id) => id == target,
                PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                    for (i, child) in children.iter().enumerate() {
                        path.push(i);
                        if find_path(child, target, path) {
                            return true;
                        }
                        path.pop();
                    }
                    false
                }
            }
        }
        let mut path = Vec::new();
        if find_path(&self.pane_root, &tab_id, &mut path) {
            let changed = self.active_tab.as_deref() != Some(tab_id.as_str());
            self.focused_pane_path = path;
            self.active_tab = Some(tab_id.clone());
            if !self.sync_initial_sftp_to_terminal_tab(&tab_id) {
                self.sync_sftp_to_terminal_tab(&tab_id, true);
            }
            // Clear stale search state when switching to a different pane.
            // The user can press Enter to re-search in the new pane.
            if changed && self.search_active {
                self.search_query.clear();
                self.search_matches.clear();
                self.search_current = 0;
                self.search_target_tab = None;
            }
        }
    }

    // ─── Multi-window support ────────────────────────────────────────

    /// Open a new blank window.
    pub(crate) fn open_new_window(&mut self, cx: &mut Context<Self>) {
        crate::app::startup::open_new_window(None, Some(self.session_store.clone()), cx);
        self.status = "new window opened".into();
        cx.notify();
    }

    /// Schedule the active tab group to move into a new native window after
    /// the current input callback has released its window and entity borrows.
    pub(crate) fn detach_tab_to_new_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let group_id = self
            .active_group
            .clone()
            .filter(|group_id| self.tab_groups.iter().any(|group| group.id == *group_id))
            .or_else(|| {
                let active_tab = self.active_tab.as_deref()?;
                self.tab_groups
                    .iter()
                    .find(|group| group.pane_root.tab_ids().contains(&active_tab))
                    .map(|group| group.id.clone())
            });
        let Some(group_id) = group_id else {
            self.status = "cannot detach: active tab group is missing".into();
            cx.notify();
            return;
        };

        self.defer_group_detach(group_id, window, cx);
    }

    pub(crate) fn defer_group_detach(
        &mut self,
        group_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = cx.entity();
        window.defer(cx, move |_window, cx| {
            Self::detach_group_to_new_window(source, group_id, cx);
        });
    }

    /// Detach a complete tab group to a new window without recreating its
    /// terminal or SFTP backends. Window creation and route handoff form the
    /// prepare step; any failure restores the original group in place.
    fn detach_group_to_new_window(source: Entity<Self>, group_id: String, cx: &mut App) {
        tracing::info!(group_id, "[tab-drag] preparing detached window");
        let prepared = source.update(cx, |this, _| {
            this.take_group_transfer(&group_id)
                .map(|transfer| (transfer, this.session_owner_id, this.session_store.clone()))
        });

        let (transfer, source_owner_id, session_store) = match prepared {
            Ok(prepared) => prepared,
            Err(message) => {
                source.update(cx, |this, cx| {
                    this.status = message.into();
                    cx.notify();
                });
                return;
            }
        };

        let result = crate::app::startup::open_new_window_with_group(
            transfer,
            source_owner_id,
            session_store,
            cx,
        );

        source.update(cx, |this, cx| {
            match result {
                Ok(()) => {
                    tracing::info!(group_id, "[tab-drag] detached window opened");
                    this.status = "tab group detached to new window".into();
                }
                Err((message, transfer)) => {
                    tracing::warn!(group_id, %message, "[tab-drag] detached window failed");
                    this.restore_group_transfer(transfer, cx);
                    this.status = format!("failed to detach tab group: {message}").into();
                }
            }
            cx.notify();
        });
    }

    // ─── Tab drag, reorder, detach and merge ──────────────────────────

    fn connection_group_drop_before_at(
        &self,
        source: &str,
        position: Point<Pixels>,
    ) -> Option<Option<String>> {
        let parent = source.rsplit_once('/').map(|(parent, _)| parent);
        let mut rows = self
            .config
            .connection_groups()
            .iter()
            .filter(|group| group.as_str() != source)
            .filter(|group| group.rsplit_once('/').map(|(parent, _)| parent) == parent)
            .filter_map(|group| {
                self.connection_group_bounds
                    .get(group)
                    .copied()
                    .map(|bounds| (group.clone(), bounds))
            })
            .collect::<Vec<_>>();
        rows.sort_by(|(_, left), (_, right)| {
            left.origin
                .y
                .partial_cmp(&right.origin.y)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let first = rows.first()?.1;
        let last = rows.last()?.1;
        if position.x < first.left()
            || position.x > first.right()
            || position.y < first.top() - px(10.)
            || position.y > last.bottom() + px(10.)
        {
            return None;
        }
        Some(
            rows.iter()
                .find(|(_, bounds)| position.y < bounds.origin.y + bounds.size.height / 2.)
                .map(|(group, _)| group.clone()),
        )
    }

    fn on_connection_group_drag_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if self.dragging_connection_group.is_none()
            && let Some((group, origin)) = self.pending_connection_group_drag.clone()
        {
            let dx: f32 = (event.position.x - origin.x).into();
            let dy: f32 = (event.position.y - origin.y).into();
            if (dx * dx + dy * dy).sqrt() > 5.0 {
                self.dragging_connection_group = Some(group);
            }
        }
        if self.dragging_connection_group.is_none() {
            return;
        }

        let Some(source) = self.dragging_connection_group.as_deref() else {
            return;
        };
        let next = self
            .connection_group_drop_before_at(source, event.position)
            .flatten();
        if self.connection_group_drop_before != next {
            self.connection_group_drop_before = next;
            cx.notify();
        }
    }

    /// Returns true when a group drag (including a pending click) consumed the
    /// release, so it cannot also be interpreted as a tab drag release.
    fn finish_connection_group_drag(
        &mut self,
        event: &MouseUpEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let pending = self.pending_connection_group_drag.take();
        let Some(source) = self.dragging_connection_group.take() else {
            return pending.is_some();
        };
        let target = self.connection_group_drop_before_at(&source, event.position);
        self.connection_group_drop_before = None;
        if let Some(before) = target {
            self.config
                .reorder_connection_group(&source, before.as_deref());
            if let Err(err) = crate::app::config_persistence::save_full(&self.config) {
                tracing::warn!("failed to save connection group order: {err:#}");
            }
        }
        cx.notify();
        true
    }

    /// Called on every root-level mouse move. Once the drag threshold is
    /// exceeded, the source tab bar reorders, another main window merges, and
    /// every remaining drop position detaches when the source has other tabs.
    pub(crate) fn on_tab_drag_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_connection_group_drag_mouse_move(event, cx);
        if self.tab_drag.promote_if_needed(event.position, 5.0) {
            cx.notify();
        }
        if !self.tab_drag.is_dragging() {
            return;
        }

        let reorder_index = if self
            .tab_bar_bounds
            .as_ref()
            .is_some_and(|bounds| bounds.contains(&event.position))
        {
            let ordered_bounds = self
                .tab_groups
                .iter()
                .filter_map(|group| {
                    self.tab_group_bounds
                        .get(&group.id)
                        .copied()
                        .map(|bounds| (group.id.clone(), bounds))
                })
                .collect::<Vec<_>>();
            let Some(dragged_group) = self.tab_drag.dragging_group() else {
                return;
            };
            reorder_index_at_x(dragged_group, event.position.x, &ordered_bounds)
        } else {
            None
        };
        let should_detach = should_offer_detach(
            self.tab_groups.len(),
            event.position,
            self.tab_bar_bounds,
            false,
        );

        let reorder_changed = self.tab_drag.set_reorder_index(reorder_index);
        let detach_changed = self.tab_drag.set_outside(should_detach);
        if reorder_changed || detach_changed {
            cx.notify();
        }
    }

    fn prepare_tab_drag_release(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if !self.tab_drag.is_dragging() {
            return;
        }

        let source_handle = window.window_handle();
        let screen_pos = Self::screen_position(window, event.position);
        let over_other_window = !cursor_inside_viewport(event.position, window.viewport_size())
            && crate::app::find_window_at_screen_pos(&source_handle, screen_pos).is_some();
        let release_index = if !over_other_window
            && self
                .tab_bar_bounds
                .as_ref()
                .is_some_and(|bounds| bounds.contains(&event.position))
        {
            let ordered_bounds = self
                .tab_groups
                .iter()
                .filter_map(|group| {
                    self.tab_group_bounds
                        .get(&group.id)
                        .copied()
                        .map(|bounds| (group.id.clone(), bounds))
                })
                .collect::<Vec<_>>();
            self.tab_drag.dragging_group().and_then(|group_id| {
                reorder_index_at_x(group_id, event.position.x, &ordered_bounds)
            })
        } else {
            None
        };
        self.tab_drag.set_reorder_index(release_index);

        let should_detach = should_offer_detach(
            self.tab_groups.len(),
            event.position,
            self.tab_bar_bounds,
            over_other_window,
        );
        self.tab_drag.set_outside(should_detach);
    }

    /// Convert a window-local cursor position to a screen-space position by
    /// adding the source window's screen-space origin.
    fn screen_position(window: &Window, local: Point<Pixels>) -> Point<Pixels> {
        let origin = match window.window_bounds() {
            gpui::WindowBounds::Fullscreen(b)
            | gpui::WindowBounds::Maximized(b)
            | gpui::WindowBounds::Windowed(b) => b.origin,
        };
        Point::new(origin.x + local.x, origin.y + local.y)
    }

    /// Cancel a tab drag and clear the target window's incoming indicator.
    pub(crate) fn cancel_tab_drag(&mut self, cx: &mut Context<Self>) {
        self.tab_drag.cancel();
        self.incoming_tab_drag = None;
        cx.notify();
    }

    /// Finish a tab drag by moving the group to another existing window,
    /// detaching it to a new window, or cancelling a neutral release.
    pub(crate) fn on_tab_drag_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.finish_connection_group_drag(event, cx) {
            return;
        }

        // Native tab drops are committed by the root on_drop handler later in
        // the same mouse-up dispatch. Committing here as well can reorder or
        // detach the source before a cross-window merge runs.
        if cx.has_active_drag() {
            return;
        }

        self.prepare_tab_drag_release(event, window, cx);
        let intent = self.tab_drag.finish();
        window.defer(cx, move |_window, cx| {
            crate::app::clear_all_incoming_tab_drags(cx);
        });
        match intent {
            DropIntent::Reorder { group_id, index } => {
                self.reorder_tab_group(&group_id, index, window, cx);
            }
            DropIntent::Detach { group_id } => {
                self.defer_group_detach(group_id, window, cx);
            }
            DropIntent::None | DropIntent::Cancelled => cx.notify(),
        }
    }

    /// Finish a native tab drag released outside this window. No root drop
    /// target receives that release, so desktop detaches need this fallback.
    pub(crate) fn on_tab_drag_mouse_up_out(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.finish_connection_group_drag(event, cx) {
            return;
        }
        if !self.tab_drag.is_dragging() {
            return;
        }

        // A desktop release has no drop target to consume GPUI's process-wide
        // active drag. Clear it before opening another native window so the new
        // window cannot paint the source window's drag preview while mouse-up
        // dispatch is still unwinding.
        cx.stop_active_drag(window);
        self.prepare_tab_drag_release(event, window, cx);
        let intent = self.tab_drag.finish();
        tracing::info!(
            detach = matches!(intent, DropIntent::Detach { .. }),
            "[tab-drag] native drag released outside source window"
        );
        window.defer(cx, move |_window, cx| {
            crate::app::clear_all_incoming_tab_drags(cx);
        });
        match intent {
            DropIntent::Detach { group_id } => {
                self.defer_group_detach(group_id, window, cx);
            }
            DropIntent::Reorder { .. } | DropIntent::None | DropIntent::Cancelled => cx.notify(),
        }
    }

    pub(crate) fn finish_native_tab_drop(
        drag: IncomingTabDrag,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        cx: &mut App,
    ) {
        if drag.source_window == target_window {
            return;
        }

        tracing::info!(
            drag_id = drag.drag_id,
            group_id = drag.group_id,
            "[tab-drag] committing cross-window merge"
        );
        let source_window = drag.source_window;
        let source = drag.source;
        let group_id = drag.group_id;
        let should_close_source = source.update(cx, |source, cx| {
            source.tab_drag.cancel();
            let merged = source.commit_group_merge(group_id, target_window, target, cx);
            should_close_empty_source(
                merged,
                source.tab_groups.is_empty(),
                &source_window,
                &target_window,
            )
        });
        if should_close_source {
            if let Err(error) = source_window.update(cx, |_, window, _| {
                window.remove_window();
            }) {
                tracing::warn!(
                    "[tab-drag] failed to close empty source window after native drop: {error:?}"
                );
            }
        }
    }

    pub(crate) fn finish_native_local_tab_drop(
        &mut self,
        group_id: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tab_drag.cancel();
        if !self.tab_groups.iter().any(|group| group.id == group_id) {
            cx.notify();
            return;
        }

        if self
            .tab_bar_bounds
            .as_ref()
            .is_some_and(|bounds| bounds.contains(&position))
        {
            let ordered_bounds = self
                .tab_groups
                .iter()
                .filter_map(|group| {
                    self.tab_group_bounds
                        .get(&group.id)
                        .copied()
                        .map(|bounds| (group.id.clone(), bounds))
                })
                .collect::<Vec<_>>();
            if let Some(index) = reorder_index_at_x(&group_id, position.x, &ordered_bounds) {
                self.reorder_tab_group(&group_id, index, window, cx);
                return;
            }
        } else if self.tab_groups.len() > 1 {
            self.defer_group_detach(group_id, window, cx);
            return;
        }

        cx.notify();
    }

    #[allow(clippy::result_large_err)]
    fn commit_group_merge(
        &mut self,
        group_id: String,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        cx: &mut Context<Self>,
    ) -> bool {
        target.update(cx, |target, cx| {
            target.incoming_tab_drag = None;
            cx.notify();
        });
        let source_owner_id = self.session_owner_id;
        let merged = match self.take_group_transfer(&group_id) {
            Ok(transfer) => {
                let result = target.update(cx, |target, cx| {
                    target.receive_group_transfer(transfer, source_owner_id, cx)
                });
                match result {
                    Ok(()) => {
                        let focus_handle = target.read(cx).focus_handle.clone();
                        crate::app::activate_window_with_retry(target_window, focus_handle, cx);
                        self.status = "tab group moved into another window".into();
                        true
                    }
                    Err((message, transfer)) => {
                        self.restore_group_transfer(transfer, cx);
                        self.status = format!("failed to move tab group: {message}").into();
                        false
                    }
                }
            }
            Err(message) => {
                self.status = message.into();
                false
            }
        };
        cx.notify();
        merged
    }

    fn reorder_tab_group(
        &mut self,
        group_id: &str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current_index) = self
            .tab_groups
            .iter()
            .position(|group| group.id == group_id)
        else {
            self.status = "cannot reorder: source group no longer exists".into();
            cx.notify();
            return;
        };

        let group = self.tab_groups.remove(current_index);
        let target_index = index.min(self.tab_groups.len());
        let group_id = group.id.clone();
        self.next_tab_group_ordinal = self.next_tab_group_ordinal.max(group.ordinal + 1);
        self.tab_groups.insert(target_index, group);
        self.activate_group(group_id, window, cx);
        self.tabs_scroll_handle.scroll_to_item(target_index);
        self.status = "tab group reordered".into();
        window.activate_window();
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn take_group_transfer(&mut self, group_id: &str) -> Result<GroupTransfer, String> {
        let group_index = self
            .tab_groups
            .iter()
            .position(|group| group.id == group_id)
            .ok_or_else(|| "cannot move: source group no longer exists".to_string())?;
        let group = self.tab_groups[group_index].clone();
        let tab_ids = group
            .pane_root
            .tab_ids()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if tab_ids.is_empty() || tab_ids.iter().any(String::is_empty) {
            return Err("cannot move: source group has no terminal panes".to_string());
        }
        let tab_id_set = tab_ids.iter().map(String::as_str).collect::<HashSet<_>>();
        if tab_id_set.len() != tab_ids.len() {
            return Err("cannot move: source group contains duplicate terminal ids".to_string());
        }
        if tab_ids
            .iter()
            .any(|tab_id| !self.tabs.iter().any(|tab| tab.id == *tab_id))
        {
            return Err("cannot move: a source terminal no longer exists".to_string());
        }

        let was_active_group = self.active_group.as_deref() == Some(group_id);
        let active_tab = self
            .active_tab
            .clone()
            .filter(|tab_id| tab_id_set.contains(tab_id.as_str()));
        let mut tabs = Vec::with_capacity(tab_ids.len());
        let mut remaining_tabs = Vec::with_capacity(self.tabs.len() - tab_ids.len());
        for (index, tab) in std::mem::take(&mut self.tabs).into_iter().enumerate() {
            if tab_id_set.contains(tab.id.as_str()) {
                tabs.push((index, tab));
            } else {
                remaining_tabs.push(tab);
            }
        }
        self.tabs = remaining_tabs;

        let mut sftp_handles = HashMap::new();
        let handle_ids = self
            .sftp_handles
            .keys()
            .filter(|id| id.as_str() == group_id || tab_id_set.contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        for handle_id in handle_ids {
            if let Some(handle) = self.sftp_handles.remove(&handle_id) {
                sftp_handles.insert(handle_id, handle);
            }
        }

        self.tab_groups.remove(group_index);
        let mut route_ids = tab_ids;
        route_ids.extend(sftp_handles.keys().cloned());
        route_ids.sort();
        route_ids.dedup();

        if was_active_group {
            self.activate_after_group_extraction(group_index);
        } else {
            self.sync_system_tab_to_active_group();
        }
        Ok(GroupTransfer {
            group,
            group_index,
            tabs,
            sftp_handles,
            route_ids,
            active_tab,
            was_active_group,
        })
    }

    fn activate_after_group_extraction(&mut self, removed_index: usize) {
        if self.tab_groups.is_empty() {
            self.pane_root = PaneLayout::Single(String::new());
            self.focused_pane_path.clear();
            self.active_tab = None;
            self.active_group = None;
            self.home_page_open = true;
            self.sync_system_tab_to_active_group();
            return;
        }

        let next_index = removed_index.min(self.tab_groups.len() - 1);
        let next_group = &self.tab_groups[next_index];
        let next_group_id = next_group.id.clone();
        let next_layout = next_group.pane_root.clone();
        let next_tab = next_layout.tab_ids().first().copied().map(str::to_string);
        self.active_group = Some(next_group_id);
        self.pane_root = next_layout;
        self.focused_pane_path.clear();
        self.active_tab = next_tab.clone();
        if let Some(tab_id) = next_tab {
            self.focus_pane_with_id(tab_id);
        }
        self.sync_system_tab_to_active_group();
    }

    fn restore_group_transfer(&mut self, mut transfer: GroupTransfer, cx: &mut Context<Self>) {
        let owner_id = self.session_owner_id;
        self.session_store.update(cx, |store, _| {
            if !store.move_event_routes(&transfer.route_ids, owner_id, owner_id) {
                for route_id in &transfer.route_ids {
                    store.register_event_route(route_id.clone(), owner_id);
                }
            }
        });

        let group_index = transfer.group_index.min(self.tab_groups.len());
        let group_id = transfer.group.id.clone();
        let group_layout = transfer.group.pane_root.clone();
        self.tab_groups.insert(group_index, transfer.group);
        transfer.tabs.sort_by_key(|(index, _)| *index);
        for (index, tab) in transfer.tabs {
            self.tabs.insert(index.min(self.tabs.len()), tab);
        }
        self.sftp_handles.extend(transfer.sftp_handles);

        if transfer.was_active_group {
            self.active_group = Some(group_id);
            self.pane_root = group_layout;
            self.focused_pane_path.clear();
            self.active_tab = transfer.active_tab.or_else(|| {
                self.pane_root
                    .tab_ids()
                    .first()
                    .copied()
                    .map(str::to_string)
            });
            if let Some(tab_id) = self.active_tab.clone() {
                self.focus_pane_with_id(tab_id);
            }
        }
        self.sync_system_tab_to_active_group();
        cx.notify();
    }

    /// Receive an intact group from another window without recreating any
    /// terminal or SFTP backend. The group remains a separate top-level tab
    /// because `TabGroup` owns a single SFTP UI state.
    #[allow(clippy::result_large_err)]
    pub(crate) fn receive_group_transfer(
        &mut self,
        transfer: GroupTransfer,
        source_owner_id: crate::session::store::WindowOwnerId,
        cx: &mut Context<Self>,
    ) -> Result<(), (String, GroupTransfer)> {
        let tab_ids = transfer
            .tabs
            .iter()
            .map(|(_, tab)| tab.id.as_str())
            .collect::<HashSet<_>>();
        if tab_ids.len() != transfer.tabs.len()
            || transfer
                .group
                .pane_root
                .tab_ids()
                .iter()
                .any(|tab_id| !tab_ids.contains(*tab_id))
        {
            return Err((
                "transfer payload does not match the group layout".to_string(),
                transfer,
            ));
        }
        if self
            .tab_groups
            .iter()
            .any(|group| group.id == transfer.group.id)
        {
            return Err(("target already contains this group".to_string(), transfer));
        }
        if self
            .tabs
            .iter()
            .any(|tab| tab_ids.contains(tab.id.as_str()))
        {
            return Err((
                "target already contains one of the transferred terminals".to_string(),
                transfer,
            ));
        }
        if transfer
            .sftp_handles
            .keys()
            .any(|handle_id| self.sftp_handles.contains_key(handle_id))
        {
            return Err((
                "target already contains one of the transferred SFTP handles".to_string(),
                transfer,
            ));
        }

        let target_owner_id = self.session_owner_id;
        let routes_moved = self.session_store.update(cx, |store, _| {
            store.move_event_routes(&transfer.route_ids, source_owner_id, target_owner_id)
        });
        if !routes_moved {
            return Err((
                "backend event routes changed before the move could commit".to_string(),
                transfer,
            ));
        }

        let GroupTransfer {
            group,
            tabs,
            sftp_handles,
            active_tab,
            ..
        } = transfer;
        let group_id = group.id.clone();
        self.next_tab_group_ordinal = self.next_tab_group_ordinal.max(group.ordinal + 1);
        let group_layout = group.pane_root.clone();
        let fallback_tab = group_layout.tab_ids().first().copied().map(str::to_string);
        self.tabs.extend(tabs.into_iter().map(|(_, tab)| tab));
        self.sftp_handles.extend(sftp_handles);
        self.tab_groups.push(group);
        self.home_page_open = false;
        self.active_system_info_tab = None;
        self.active_group = Some(group_id);
        self.pane_root = group_layout;
        self.focused_pane_path.clear();
        self.active_tab = active_tab.or(fallback_tab);
        if let Some(tab_id) = self.active_tab.clone() {
            self.focus_pane_with_id(tab_id);
        }
        self.sync_system_tab_to_active_group();
        self.status = "tab group moved from another window".into();
        cx.notify();
        Ok(())
    }
}
