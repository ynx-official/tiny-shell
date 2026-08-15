use std::collections::{HashMap, HashSet};

use gpui::{
    AnyWindowHandle, App, Context, Entity, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Window, px,
};
use rust_i18n::t;
use uuid::Uuid;

use crate::{
    PaneLayout, SelectorEntry, TabGroup, TinyShell,
    app::{
        AuxiliaryWindowsState, IncomingTabDrag, PaneDirection, SystemInfoTab,
        constants::{DEFAULT_COLS, DEFAULT_ROWS},
        tab_drag::{
            DockZone, DropIntent, TAB_DRAG_THRESHOLD, cursor_inside_viewport,
            local_position_in_window, reorder_index_at_x, should_close_empty_source,
            should_offer_detach, tab_merge_target_at,
        },
    },
    backend::{local, ssh},
    session::config::{AuthMethod, ConfigStore, Session},
    terminal::{BackendCommand, RenderSnapshot, TabKind, TerminalTab},
};

pub(crate) struct GroupTransfer {
    group: TabGroup,
    group_index: usize,
    tabs: Vec<(usize, TerminalTab)>,
    sftp_handles: HashMap<String, crate::sftp::SftpHandle>,
    route_ids: Vec<String>,
    active_tab: Option<String>,
    system_info_tabs: Vec<SystemInfoTab>,
    active_system_info_tab: Option<String>,
    was_active_group: bool,
}

struct NativeTabDropTarget {
    window: AnyWindowHandle,
    entity: Entity<TinyShell>,
    zone: DockZone,
}

impl TinyShell {
    fn workspace_state_mut(
        &mut self,
    ) -> &mut crate::app::terminal_workspace::TerminalWorkspaceState {
        self.window_state_mut().workspace_state_mut()
    }

    // ── 系统信息标签 ──

    pub(crate) fn open_system_info_tab(&mut self, cx: &mut Context<Self>) {
        let source_tab_id = self.system_tab_id.clone().or_else(|| {
            let active_id = self.workspace().active_tab_id()?.to_owned();
            self.workspace()
                .tabs()
                .iter()
                .find(|tab| tab.id == active_id && tab.kind == TabKind::Ssh)
                .map(|tab| tab.id.clone())
        });
        let Some(source_tab_id) = source_tab_id else {
            return;
        };

        let info_id = if let Some(existing) = self
            .workspace()
            .system_info_tabs()
            .iter()
            .find(|tab| tab.source_tab_id == source_tab_id)
        {
            existing.id.clone()
        } else {
            let host_title = self
                .workspace()
                .tabs()
                .iter()
                .find(|tab| tab.id == source_tab_id)
                .map(|tab| tab.title.clone())
                .unwrap_or_else(|| t!("system_information").to_string());
            let id = Uuid::new_v4().to_string();
            self.workspace_state_mut()
                .push_system_info_tab(SystemInfoTab {
                    id: id.clone(),
                    source_tab_id: source_tab_id.clone(),
                    title: format!("{} · {}", host_title, t!("system_information")),
                });
            id
        };

        self.workspace_state_mut()
            .set_active_tab(Some(source_tab_id.clone()));
        self.system_tab_id = Some(source_tab_id);
        self.workspace_state_mut()
            .set_active_system_info_tab(Some(info_id));
        self.home_page_open = false;
        self.request_active_system_snapshot();
        cx.notify();
    }

    pub(crate) fn close_system_info_tab(&mut self, id: String, cx: &mut Context<Self>) {
        let was_active = self.workspace().active_system_info_tab_id() == Some(id.as_str());
        self.workspace_state_mut().remove_system_info_tab(&id);
        if was_active {
            self.workspace_state_mut().clear_active_system_info_tab();
        }
        cx.notify();
    }

    // ── 本地终端 ──

    pub(crate) fn open_local(&mut self, cx: &mut Context<Self>) {
        self.workspace_state_mut().clear_active_system_info_tab();
        self.home_page_open = false;
        let ordinal = self.workspace_state_mut().reserve_tab_group_ordinal();
        let id = Uuid::new_v4().to_string();
        let events = self.backend_events_sender(cx);
        match local::spawn_local_terminal(id.clone(), DEFAULT_COLS, DEFAULT_ROWS, events.clone(), 1)
        {
            Ok(backend) => {
                let title = if cfg!(windows) {
                    t!("local_terminal_powershell").to_string()
                } else {
                    t!("local_terminal").to_string()
                };
                let mut tab = TerminalTab::new_local(id.clone(), title.clone(), backend, events);
                tab.resize(DEFAULT_COLS, DEFAULT_ROWS);
                let group_id = Uuid::new_v4().to_string();
                self.install_terminal_tab(
                    tab,
                    TabGroup {
                        id: group_id.clone(),
                        drag_id: crate::app::next_tab_drag_id(),
                        ordinal,
                        title,
                        pane_root: PaneLayout::Single(id.clone()),
                        sftp: None,
                    },
                );
                self.register_backend_route(id.clone(), cx);
                let tab_count = self.workspace().tab_count();
                self.tabs_scroll_handle.scroll_to_item(tab_count - 1);
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
        let Some((session, proxy_config)) = self.prepare_ssh_session(cx) else {
            return;
        };

        self.spawn_ssh_backend(session, proxy_config, cx);
        self.editing_session_id = None;
        self.session_group_selection = self.connection_group_filter.clone();
        if let Some(token) = self.selector_dialog_token.take() {
            self.dismiss_dialog(token, window, cx);
        }
        cx.notify();
    }

    fn prepare_ssh_session(&mut self, cx: &mut Context<Self>) -> Option<(Session, ConfigStore)> {
        let session_name = self
            .connection_inputs
            .session_name_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let host = self
            .connection_inputs
            .host_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let port = self
            .connection_inputs
            .port_input
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .unwrap_or(22);
        let user = self
            .connection_inputs
            .user_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let password = self
            .connection_inputs
            .password_input
            .read(cx)
            .value()
            .to_string();
        let key_path = self
            .connection_inputs
            .key_path_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let key_inline = self
            .connection_inputs
            .key_inline_input
            .read(cx)
            .value()
            .to_string();
        let passphrase = self
            .connection_inputs
            .passphrase_input
            .read(cx)
            .value()
            .to_string();

        if host.is_empty() || user.is_empty() {
            self.status = t!("host_and_user_required").into();
            cx.notify();
            return None;
        }

        if self.ssh_proxy_type != "none" {
            let proxy_host = self
                .connection_inputs
                .proxy_host_input
                .read(cx)
                .value()
                .trim()
                .to_string();
            let proxy_port_str = self
                .connection_inputs
                .proxy_port_input
                .read(cx)
                .value()
                .trim()
                .to_string();
            let proxy_port = proxy_port_str.parse::<u16>().ok();
            if proxy_host.is_empty() || proxy_port.is_none() {
                self.status = t!("ssh_editor_proxy_required").into();
                cx.notify();
                return None;
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
        session.proxy_host = self
            .connection_inputs
            .proxy_host_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        session.proxy_port = self
            .connection_inputs
            .proxy_port_input
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .ok();
        session.proxy_user = self
            .connection_inputs
            .proxy_user_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        session.proxy_password = self
            .connection_inputs
            .proxy_password_input
            .read(cx)
            .value()
            .to_string();

        Some((session, self.config.clone()))
    }

    fn spawn_ssh_backend(
        &mut self,
        session: Session,
        _proxy_config: ConfigStore,
        cx: &mut Context<Self>,
    ) {
        // Persist the edited/new session before opening it.
        let mut staged = self.config.clone();
        staged.upsert(session.clone());
        self.commit_staged_config_async(
            staged,
            move |this, cx| this.open_ssh_session(session, cx),
            |this, error, cx| {
                tracing::warn!("failed to save SSH session: {error:#}");
                this.status = t!("config_save_failed", error = format!("{error:#}")).into();
                cx.notify();
            },
            cx,
        );
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

        let selector_token = self.selector_dialog_token.take();
        if let Some(token) = selector_token {
            self.dismiss_dialog(token, window, cx);
        }
        match entry {
            SelectorEntry::Local => {
                self.open_local(cx);
            }
            SelectorEntry::NewSsh => {
                self.open_new_ssh_dialog(window, cx);
            }
            SelectorEntry::Saved(session_id) => {
                self.connect_saved_session(session_id, window, cx);
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

    pub(crate) fn close_tab(&mut self, id: String, cx: &mut Context<Self>) {
        let closing_sftp_group = self
            .workspace()
            .tab_groups()
            .iter()
            .find(|group| group.pane_root.contains(&id))
            .filter(|group| group.pane_root.total_panes() <= 1)
            .filter(|group| self.sftp_handles.contains_key(&group.id))
            .map(|group| group.id.clone());

        if let Some(session_id) = closing_sftp_group {
            if !crate::app::sftp_editor_window::request_session_close(
                &session_id,
                self.session_owner_id,
                id.clone(),
                cx.entity(),
                cx,
            ) {
                return;
            }
        }

        let route_ids = self
            .workspace()
            .tab_groups()
            .iter()
            .find(|group| group.pane_root.contains(&id))
            .map(|group| {
                let mut ids = group
                    .pane_root
                    .tab_ids()
                    .iter()
                    .filter(|tab_id| **tab_id == id || group.pane_root.total_panes() <= 1)
                    .map(|tab_id| (*tab_id).to_string())
                    .collect::<Vec<_>>();
                if group.pane_root.total_panes() <= 1 {
                    ids.push(group.id.clone());
                }
                ids
            })
            .unwrap_or_else(|| vec![id.clone()]);
        for route_id in route_ids {
            self.unregister_backend_route(&route_id, cx);
        }

        self.handle_tab_close(id, cx);
        cx.notify();
    }

    pub(crate) fn disconnect_tab_group(&mut self, group_id: &str, cx: &mut Context<Self>) {
        let Some(group) = self.tab_group(group_id) else {
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

        for tab_id in &tab_ids {
            self.unregister_backend_route(tab_id, cx);
        }
        self.unregister_backend_route(group_id, cx);

        for tab_id in tab_ids {
            if let Some(tab) = self.workspace_state_mut().terminal_tab_mut(&tab_id) {
                if tab.kind == TabKind::Ssh && tab.connected {
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

    pub(crate) fn handle_tab_close(&mut self, id: String, cx: &mut Context<Self>) {
        if self.window_state.search_target_tab.as_deref() == Some(id.as_str()) {
            self.window_state.search_target_tab = None;
            self.window_state.search_matches.clear();
            self.window_state.search_query.clear();
            self.window_state.search_current = 0;
        }
        self.terminal_completions.remove(&id);

        let active_info_id = self
            .workspace()
            .active_system_info_tab_id()
            .map(str::to_owned);
        let removed_active_info = self.workspace().system_info_tabs().iter().any(|tab| {
            tab.source_tab_id == id && active_info_id.as_deref() == Some(tab.id.as_str())
        });
        let info_ids = self
            .workspace()
            .system_info_tabs()
            .iter()
            .filter(|tab| tab.source_tab_id == id)
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        for info_id in info_ids {
            self.workspace_state_mut().remove_system_info_tab(&info_id);
        }
        if removed_active_info {
            self.workspace_state_mut().clear_active_system_info_tab();
        }

        let Some((group_index, tab_index)) = self.find_tab_and_group(&id) else {
            tracing::info!(
                "[handle_tab_close] no group found for tab '{}', closing individually",
                id
            );
            let mut tabs = self.workspace_state_mut().take_tabs();
            if let Some(ix) = tabs.iter().position(|tab| tab.id == id) {
                tabs[ix].send_backend(BackendCommand::Close);
                tabs.remove(ix);
            }
            self.workspace_state_mut().append_tabs(tabs);
            self.monitoring.remote_system_snapshots.remove(&id);
            if self.workspace().tab_count() == 0 {
                self.auxiliary_windows = AuxiliaryWindowsState::default();
                crate::app::close_auxiliary_windows(self.session_owner_id, cx);
            }
            return;
        };

        let (was_active, next_active_id) = self.resolve_next_active(group_index, tab_index);
        self.close_tab_at(group_index, tab_index);

        if self.workspace().tab_count() == 0 || self.workspace().tab_groups().is_empty() {
            self.workspace_state_mut().clear();
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
            self.auxiliary_windows = AuxiliaryWindowsState::default();
            crate::app::close_auxiliary_windows(self.session_owner_id, cx);
            return;
        }

        self.activate_tab_after_close(was_active, next_active_id);
        self.sync_system_tab_to_active_group();
    }

    fn find_tab_and_group(&self, tab_id: &str) -> Option<(usize, usize)> {
        let group_index = self
            .workspace()
            .tab_groups()
            .iter()
            .position(|g| g.pane_root.contains(tab_id))?;
        let tab_index = self.workspace().tab_groups()[group_index]
            .pane_root
            .tab_ids()
            .iter()
            .position(|&s| s == tab_id)?;
        Some((group_index, tab_index))
    }

    fn resolve_next_active(&self, group_index: usize, tab_index: usize) -> (bool, Option<String>) {
        let group = &self.workspace().tab_groups()[group_index];
        let tabs_in_group = group.pane_root.tab_ids();
        let id = tabs_in_group[tab_index];
        let was_active = self.workspace().active_tab_id() == Some(id);
        let mut next_active_id = None;

        if was_active {
            if tab_index > 0 {
                next_active_id = Some(tabs_in_group[tab_index - 1].to_string());
            } else if tab_index + 1 < tabs_in_group.len() {
                next_active_id = Some(tabs_in_group[tab_index + 1].to_string());
            } else if let Some(pos) = self
                .workspace()
                .tab_groups()
                .iter()
                .position(|g| g.id == group.id)
            {
                if pos > 0 {
                    next_active_id = self.workspace().tab_groups()[pos - 1]
                        .pane_root
                        .tab_ids()
                        .first()
                        .copied()
                        .map(String::from);
                } else if pos + 1 < self.workspace().tab_groups().len() {
                    next_active_id = self.workspace().tab_groups()[pos + 1]
                        .pane_root
                        .tab_ids()
                        .first()
                        .copied()
                        .map(String::from);
                }
            }
        }

        (was_active, next_active_id)
    }

    fn close_tab_at(&mut self, group_index: usize, tab_index: usize) {
        let group = self.workspace_state_mut().tab_groups()[group_index].clone();
        let pane_ids = group.pane_root.tab_ids();
        let id = pane_ids[tab_index].to_string();
        let is_group_close = pane_ids.len() <= 1;

        tracing::info!(
            "[handle_tab_close] id='{}' group_panes={:?} is_group_close={}",
            id,
            pane_ids,
            is_group_close
        );

        if is_group_close {
            let tab_ids: Vec<String> = pane_ids.iter().map(|s| s.to_string()).collect();
            for tab_id in &tab_ids {
                if let Some(ix) = self
                    .workspace_state_mut()
                    .tabs()
                    .iter()
                    .position(|tab| tab.id == *tab_id)
                {
                    self.workspace_state_mut().tabs()[ix].send_backend(BackendCommand::Close);
                    self.workspace_state_mut()
                        .tabs_mut()
                        .retain(|t| t.id != *tab_id);
                }
                self.monitoring.remote_system_snapshots.remove(tab_id);
            }
            if let Some(handle) = self.sftp_handles.remove(&group.id) {
                handle.close();
            }
            self.workspace_state_mut().remove_group_at(group_index);
            self.workspace_state_mut().pane_root_mut().remove_tab(&id);
        } else {
            if let Some(ix) = self
                .workspace_state_mut()
                .tabs()
                .iter()
                .position(|tab| tab.id == id)
            {
                self.workspace_state_mut().tabs()[ix].send_backend(BackendCommand::Close);
                self.workspace_state_mut().tabs_mut().retain(|t| t.id != id);
            }
            self.monitoring.remote_system_snapshots.remove(&id);
            if let Some(g) = self
                .workspace_state_mut()
                .tab_groups_mut()
                .iter_mut()
                .find(|g| g.pane_root.contains(&id))
            {
                g.pane_root.remove_tab(&id);
            }
            self.workspace_state_mut().pane_root_mut().remove_tab(&id);
            self.sync_pane_root_to_group();
        }

        let tab_ids: HashSet<String> = self
            .workspace_state_mut()
            .tabs()
            .iter()
            .map(|tab| tab.id.clone())
            .collect();
        self.workspace_state_mut()
            .system_info_tabs_mut()
            .retain(|info| tab_ids.contains(&info.source_tab_id));
        if self
            .workspace()
            .active_system_info_tab_id()
            .as_ref()
            .is_some_and(|active_id| {
                !self
                    .workspace()
                    .system_info_tabs()
                    .iter()
                    .any(|info| &info.id == active_id)
            })
        {
            self.workspace_state_mut().clear_active_system_info_tab();
        }
    }

    fn activate_tab_after_close(&mut self, was_active: bool, next_active_id: Option<String>) {
        if was_active
            || self
                .workspace()
                .active_tab_id()
                .as_ref()
                .is_some_and(|active_id| {
                    !self
                        .workspace()
                        .tabs()
                        .iter()
                        .any(|tab| &tab.id == active_id)
                })
        {
            let new_id = next_active_id.or_else(|| {
                self.workspace()
                    .pane_root()
                    .tab_ids()
                    .first()
                    .copied()
                    .map(String::from)
                    .or_else(|| self.workspace().tabs().first().map(|t| t.id.clone()))
            });
            if let Some(new_id) = new_id {
                self.workspace_state_mut()
                    .set_active_tab(Some(new_id.clone()));
                let next_group = self
                    .workspace()
                    .tab_groups()
                    .iter()
                    .find(|g| g.pane_root.contains(&new_id))
                    .map(|g| (g.id.clone(), g.pane_root.clone()));
                if let Some((group_id, pane_root)) = next_group {
                    let is_group_switch =
                        self.workspace().active_group_id() != Some(group_id.as_str());
                    self.workspace_state_mut().set_active_group(Some(group_id));
                    self.workspace_state_mut().set_pane_root(pane_root);
                    if is_group_switch {
                        self.reset_sftp_tree_for_active_group();
                    }
                }
                self.focus_pane_with_id(new_id);
            }
        } else if let Some(active_id) = self.workspace_state_mut().active_tab_value() {
            // Pane root structure may have changed (e.g. sibling removed), recalc path.
            self.focus_pane_with_id(active_id);
        }
    }

    pub(crate) fn focus_terminal(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // If the search bar is visible and the click is inside it, let the
        // search bar handle the event instead of switching pane focus.
        if self.window_state.search_active {
            if let Some(bounds) = self.window_state.search_bar_bounds {
                if bounds.contains(&event.position) {
                    return;
                }
            }
        }
        self.focus_handle.focus(window, cx);
        // Check if click is in a different pane and focus it
        let click_pos = event.position;
        let current_active = self.workspace_state_mut().active_tab_value();
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
                            let _ = crate::app::platform::open_url(&url);
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
        self.workspace()
            .active_tab_id()
            .and_then(|id| self.workspace().terminal_tab(id))
            .map(|t| t.render_snapshot(self.config.keyword_highlight()))
    }

    pub(crate) fn active_kind(&self) -> Option<TabKind> {
        self.workspace()
            .active_tab_id()
            .and_then(|id| self.workspace().terminal_tab(id))
            .map(|tab| tab.kind)
    }

    pub(crate) fn active_session_id(&self) -> Option<&str> {
        self.workspace()
            .active_tab_id()
            .and_then(|id| self.workspace().terminal_tab(id))
            .and_then(|tab| tab.session.as_ref())
            .map(|session| session.id.as_str())
    }

    // ── 面板分割 ──

    pub(crate) fn split_current_pane(&mut self, direction: PaneDirection, cx: &mut Context<Self>) {
        let workspace = self.workspace();
        tracing::info!(
            "[split] direction={:?} pane_root={:?} focused_path={:?} active_tab={:?} tabs={}",
            direction,
            workspace.pane_root(),
            workspace.focused_pane_path(),
            workspace.active_tab_id(),
            workspace.tabs().len(),
        );
        let Some((current_id, title)) = self.find_focused_tab() else {
            return;
        };
        let path = self.workspace().focused_pane_path().to_vec();
        self.split_pane_at(&path, current_id, title, direction, cx);
    }

    fn find_focused_tab(&self) -> Option<(String, String)> {
        let workspace = self.workspace();
        let id = workspace
            .pane_root()
            .focused_tab_id(workspace.focused_pane_path())?;
        if id.is_empty() {
            return None;
        }
        let title = workspace.terminal_tab(id)?.title.clone();
        Some((id.to_owned(), title))
    }

    fn split_pane_at(
        &mut self,
        path: &[usize],
        tab_id: String,
        _title: String,
        direction: PaneDirection,
        cx: &mut Context<Self>,
    ) {
        let (current_kind, current_session) = match self.workspace().terminal_tab(&tab_id) {
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
                    1,
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
                    ssh::SshTerminalRequest::new(
                        new_id.clone(),
                        session.clone(),
                        proxy_config.clone(),
                        DEFAULT_COLS,
                        DEFAULT_ROWS,
                        1,
                    ),
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
            TabKind::Rdp => {
                self.status = t!("rdp_split_not_supported").into();
                cx.notify();
                return;
            }
        };
        tab.resize(DEFAULT_COLS, DEFAULT_ROWS);
        // Do NOT add to tab_groups — pane stays within the existing group
        self.workspace_state_mut().push_tab(tab);
        // Do NOT scroll tab bar or add tab bar entry

        let current_pane = PaneLayout::Single(tab_id);
        let new_pane = PaneLayout::Single(new_id.clone());

        let split_layout = match direction {
            PaneDirection::Left | PaneDirection::Right => {
                let children = match direction {
                    PaneDirection::Left => vec![new_pane, current_pane],
                    PaneDirection::Right => vec![current_pane, new_pane],
                    _ => unreachable!("vertical direction matched horizontal split"),
                };
                PaneLayout::Vertical(children, 0.5)
            }
            PaneDirection::Up | PaneDirection::Down => {
                let children = match direction {
                    PaneDirection::Up => vec![new_pane, current_pane],
                    PaneDirection::Down => vec![current_pane, new_pane],
                    _ => unreachable!("horizontal direction matched vertical split"),
                };
                PaneLayout::Horizontal(children, 0.5)
            }
        };

        self.workspace_state_mut()
            .pane_root_mut()
            .replace_at(path, split_layout);
        self.sync_pane_root_to_group();
        // Update focused_pane_path: the new pane is at the indicated child index
        let mut new_full_path = path.to_vec();
        if matches!(direction, PaneDirection::Right | PaneDirection::Down) {
            new_full_path.push(1);
        } else {
            new_full_path.push(0);
        }
        self.workspace_state_mut()
            .set_focused_pane_path(new_full_path);
        self.workspace_state_mut().set_active_tab(Some(new_id));
        self.status = t!("pane_split_done").into();
        let workspace = self.workspace();
        tracing::info!(
            "[split] DONE: pane_root={:?} focused_path={:?} active_tab={:?} tabs={}",
            workspace.pane_root(),
            workspace.focused_pane_path(),
            workspace.active_tab_id(),
            workspace.tabs().len(),
        );
        cx.notify();
    }

    pub(crate) fn focus_adjacent_pane(&mut self, direction: PaneDirection) {
        if self.workspace_state_mut().focused_pane_path().is_empty() {
            return;
        }
        let path = self.workspace().focused_pane_path().to_vec();
        let pane_root = self.workspace().pane_root().clone();
        if let Some(new_path) = Self::find_adjacent_pane(&pane_root, &path, direction) {
            let workspace = self.workspace_state_mut();
            workspace.set_focused_pane_path(new_path);
            let focused_path = workspace.focused_pane_path().to_vec();
            if let Some(id) = workspace.pane_root().focused_tab_id(&focused_path) {
                let id_owned = id.to_string();
                let changed = workspace.active_tab_id() != Some(id_owned.as_str());
                workspace.set_active_tab(Some(id_owned));
                // Clear stale search state when switching to a different pane.
                if changed && self.window_state.search_active {
                    self.window_state_mut().search_query.clear();
                    self.window_state_mut().search_matches.clear();
                    self.window_state_mut().search_current = 0;
                    self.window_state_mut().search_target_tab = None;
                }
            }
        }
    }

    fn first_leaf_path(layout: &PaneLayout) -> Vec<usize> {
        match layout {
            PaneLayout::Empty => vec![],
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
            PaneLayout::Empty => vec![],
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
        direction: PaneDirection,
    ) -> Option<Vec<usize>> {
        if path.is_empty() {
            return None;
        }
        Self::adjacent_pane_in_layout(layout, path, direction)
    }

    fn adjacent_pane_in_layout(
        layout: &PaneLayout,
        path: &[usize],
        direction: PaneDirection,
    ) -> Option<Vec<usize>> {
        match layout {
            PaneLayout::Empty => None,
            PaneLayout::Single(_) => None,
            PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                let is_horizontal = matches!(layout, PaneLayout::Horizontal(_, _));
                let idx = path[0];

                // Does this split level match the movement direction?
                let vert = matches!(direction, PaneDirection::Up | PaneDirection::Down);
                let horiz = matches!(direction, PaneDirection::Left | PaneDirection::Right);
                // PaneLayout::Horizontal renders as v_flex (vertical stack),
                // PaneLayout::Vertical renders as h_flex (horizontal row).
                // So for a Vertical (h_flex), h/l moves between children;
                // for a Horizontal (v_flex), j/k moves between children.
                let moves_in_this_split = (vert && is_horizontal) || (horiz && !is_horizontal);

                if path.len() == 1 {
                    // Direct child level
                    if moves_in_this_split {
                        let delta: i32 =
                            if matches!(direction, PaneDirection::Up | PaneDirection::Left) {
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
                        Self::adjacent_pane_in_layout(&children[idx], &path[1..], direction)
                    {
                        child_path.insert(0, idx);
                        Some(child_path)
                    } else if moves_in_this_split {
                        // Try sibling at this level
                        let delta: i32 =
                            if matches!(direction, PaneDirection::Up | PaneDirection::Left) {
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
        let is_group_switch = self.workspace().active_group_id() != Some(group_id.as_str());
        self.home_page_open = false;
        self.workspace_state_mut().clear_active_system_info_tab();
        // The active group owns its layout; switching the active id is enough
        // to make `workspace().pane_root()` resolve to the new group.
        if let Some((pane_root, ids)) = self.tab_group(&group_id).map(|group| {
            (
                group.pane_root.clone(),
                group
                    .pane_root
                    .tab_ids()
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            )
        }) {
            self.workspace_state_mut().set_active_group(Some(group_id));
            self.workspace_state_mut().set_pane_root(pane_root);
            if let Some(first_id) = ids.first() {
                self.workspace_state_mut()
                    .set_active_tab(Some(first_id.clone()));
                self.focus_pane_with_id(first_id.clone());
            }
            self.focus_handle.focus(window, cx);
            if is_group_switch {
                self.reset_sftp_tree_for_active_group();
            }
        }
        self.sync_system_tab_to_active_group();
        cx.notify();
    }

    pub(crate) fn sync_pane_root_to_group(&mut self) {
        // Kept as a compatibility hook for older action paths. Layout reads
        // and mutations now resolve directly to the active TabGroup.
    }

    pub(crate) fn sync_system_tab_to_active_group(&mut self) {
        let mut group_ssh_tabs = vec![];
        if let Some(group_id) = self.workspace().active_group_id().map(str::to_owned) {
            let ids = self
                .tab_group(&group_id)
                .map(|group| group.pane_root.tab_ids())
                .unwrap_or_default();
            for id in ids {
                if let Some(tab) = self.workspace().terminal_tab(id) {
                    if tab.kind == TabKind::Ssh {
                        group_ssh_tabs.push(tab.id.clone());
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
        let is_horizontal =
            Self::is_layout_horizontal_at(self.workspace_state_mut().pane_root_mut(), parent_path);
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
        Self::adjust_split_ratio(
            self.workspace_state_mut().pane_root_mut(),
            parent_path,
            child_idx,
            ratio_delta,
        );
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
                PaneLayout::Empty => false,
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
        if find_path(
            self.workspace_state_mut().pane_root_mut(),
            &tab_id,
            &mut path,
        ) {
            let changed =
                self.workspace_state_mut().active_tab_value().as_deref() != Some(tab_id.as_str());
            self.workspace_state_mut().set_focused_pane_path(path);
            self.workspace_state_mut()
                .set_active_tab(Some(tab_id.clone()));
            if !self.sync_initial_sftp_to_terminal_tab(&tab_id) {
                self.sync_sftp_to_terminal_tab(&tab_id, true);
            }
            // Clear stale search state when switching to a different pane.
            // The user can press Enter to re-search in the new pane.
            if changed && self.window_state.search_active {
                self.window_state_mut().search_query.clear();
                self.window_state_mut().search_matches.clear();
                self.window_state_mut().search_current = 0;
                self.window_state_mut().search_target_tab = None;
            }
        }
    }

    // ─── Multi-window support ────────────────────────────────────────

    /// Open a new blank window.
    pub(crate) fn open_new_window(&mut self, cx: &mut Context<Self>) {
        crate::app::startup::open_new_window(
            None,
            Some(self.session_store.clone()),
            self.config_repository.clone(),
            cx,
        );
        self.status = t!("new_window_opened").into();
        cx.notify();
    }

    /// Schedule the active tab group to move into a new native window after
    /// the current input callback has released its window and entity borrows.
    pub(crate) fn detach_tab_to_new_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let group_id = self
            .workspace()
            .active_group_id()
            .map(str::to_owned)
            .filter(|group_id| {
                self.workspace()
                    .tab_groups()
                    .iter()
                    .any(|group| group.id == *group_id)
            })
            .or_else(|| {
                let active_tab = self.workspace().active_tab_id().map(str::to_owned)?;
                self.workspace()
                    .tab_groups()
                    .iter()
                    .find(|group| group.pane_root.tab_ids().contains(&active_tab.as_str()))
                    .map(|group| group.id.clone())
            });
        let Some(group_id) = group_id else {
            self.status = t!("cannot_detach_tab_group").into();
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

    pub(crate) fn defer_groups_detach(
        &mut self,
        group_ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if group_ids.len() <= 1 {
            if let Some(group_id) = group_ids.into_iter().next() {
                self.defer_group_detach(group_id, window, cx);
            }
            return;
        }
        let source = cx.entity();
        window.defer(cx, move |_window, cx| {
            Self::detach_groups_to_new_window(source, group_ids, cx);
        });
    }

    fn detach_groups_to_new_window(source: Entity<Self>, group_ids: Vec<String>, cx: &mut App) {
        let prepared = source.update(cx, |this, _| {
            let mut transfers = Vec::new();
            for group_id in &group_ids {
                match this.take_group_transfer(group_id) {
                    Ok(transfer) => transfers.push(transfer),
                    Err(message) => return Err((message, transfers)),
                }
            }
            Ok((
                transfers,
                this.session_owner_id,
                this.session_store.clone(),
                this.config_repository.clone(),
            ))
        });

        let (transfers, source_owner_id, session_store, config_repository) = match prepared {
            Ok(prepared) => prepared,
            Err((message, transfers)) => {
                source.update(cx, |this, cx| {
                    for transfer in transfers.into_iter().rev() {
                        this.restore_group_transfer(transfer, cx);
                    }
                    this.status = message.into();
                    cx.notify();
                });
                return;
            }
        };

        match crate::app::startup::open_new_window_with_groups(
            transfers,
            source_owner_id,
            session_store,
            config_repository,
            cx,
        ) {
            Ok(()) => {
                source.update(cx, |this, cx| {
                    this.status = t!("tab_groups_detached").into();
                    cx.notify();
                });
            }
            Err((message, transfers)) => {
                source.update(cx, |this, cx| {
                    for transfer in transfers.into_iter().rev() {
                        this.restore_group_transfer(transfer, cx);
                    }
                    this.status = t!("tab_group_detach_failed", error = message).into();
                    cx.notify();
                });
            }
        }
    }

    /// Detach a complete tab group to a new window without recreating its
    /// terminal or SFTP backends. Window creation and route handoff form the
    /// prepare step; any failure restores the original group in place.
    fn detach_group_to_new_window(source: Entity<Self>, group_id: String, cx: &mut App) {
        tracing::info!(group_id, "[tab-drag] preparing detached window");
        let prepared = source.update(cx, |this, _| {
            this.take_group_transfer(&group_id).map(|transfer| {
                (
                    transfer,
                    this.session_owner_id,
                    this.session_store.clone(),
                    this.config_repository.clone(),
                )
            })
        });

        let (transfer, source_owner_id, session_store, config_repository) = match prepared {
            Ok(prepared) => prepared,
            Err(message) => {
                source.update(cx, |this, cx| {
                    this.status = message.into();
                    cx.notify();
                });
                return;
            }
        };

        let moved_search_target = transfer.tabs.iter().any(|(_, tab)| {
            source.read(cx).window_state.search_target_tab.as_deref() == Some(tab.id.as_str())
        });
        let result = crate::app::startup::open_new_window_with_group(
            transfer,
            source_owner_id,
            session_store,
            config_repository,
            cx,
        );

        source.update(cx, |this, cx| {
            match result {
                Ok(()) => {
                    tracing::info!(group_id, "[tab-drag] detached window opened");
                    if moved_search_target {
                        this.window_state.search_target_tab = None;
                        this.window_state.search_matches.clear();
                        this.window_state.search_query.clear();
                        this.window_state.search_current = 0;
                    }
                    this.status = t!("tab_group_detached").into();
                }
                Err((message, transfer)) => {
                    tracing::warn!(group_id, %message, "[tab-drag] detached window failed");
                    this.restore_group_transfer(*transfer, cx);
                    this.status = t!("tab_group_detach_failed", error = message).into();
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
            if let Err(err) = crate::app::config_persistence::save_full_async(
                &self.config_repository,
                &self.config,
            ) {
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
        if self
            .tab_drag
            .promote_if_needed(event.position, TAB_DRAG_THRESHOLD)
        {
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
                .workspace()
                .tab_groups()
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
            self.workspace_state_mut().tab_groups().len(),
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

    fn selected_drag_group_ids(&self, anchor: &str) -> Vec<String> {
        self.tab_drag.ordered_drag_groups(
            anchor,
            self.workspace()
                .tab_groups()
                .iter()
                .map(|group| group.id.clone()),
        )
    }

    fn native_tab_drag_payload(
        &self,
        source_window: AnyWindowHandle,
        source: Entity<Self>,
    ) -> Option<IncomingTabDrag> {
        let group_id = self.tab_drag.dragging_group()?.to_string();
        let drag_id = self
            .workspace()
            .tab_groups()
            .iter()
            .find(|group| group.id == group_id)?
            .drag_id;
        Some(IncomingTabDrag {
            drag_id,
            source_window,
            source,
            group_id,
        })
    }

    pub(crate) fn native_tab_drop_zone(&self, position: Point<Pixels>) -> Option<DockZone> {
        let terminal_bounds = self.terminal_panel_bounds?;
        tab_merge_target_at(position, terminal_bounds, self.tab_bar_bounds)
            .then_some(DockZone::Center)
    }

    fn native_tab_drop_target(
        &self,
        source_position: Point<Pixels>,
        source_window: &Window,
        cx: &App,
    ) -> Option<NativeTabDropTarget> {
        if cursor_inside_viewport(source_position, source_window.viewport_size()) {
            return None;
        }

        let source_handle = source_window.window_handle();
        let screen_position = Self::screen_position(source_window, source_position);
        let (target_window, target, target_bounds) =
            crate::app::find_window_at_screen_pos(&source_handle, screen_position)?;
        let target_position = local_position_in_window(screen_position, target_bounds);
        let zone = {
            let target_state = target.read(cx);
            target_state.native_tab_drop_zone(target_position)?
        };
        Some(NativeTabDropTarget {
            window: target_window,
            entity: target,
            zone,
        })
    }

    /// Promote the source-side drag state using a cursor event delivered to a
    /// target window. This keeps TinyShell's 10px threshold intact even on
    /// platforms that stop sending move events to the source window.
    pub(crate) fn promote_native_tab_drag_from_target(
        drag: &IncomingTabDrag,
        target_position: Point<Pixels>,
        target_window: &Window,
        cx: &mut App,
    ) -> bool {
        if drag.source.read(cx).tab_drag.dragging_group() == Some(drag.group_id.as_str()) {
            return true;
        }
        let Some(source_bounds) = drag.source.read(cx).last_registered_window_bounds else {
            return false;
        };
        let screen_position = Self::screen_position(target_window, target_position);
        let source_position = local_position_in_window(screen_position, source_bounds);
        drag.source.update(cx, |source, cx| {
            if source
                .tab_drag
                .promote_if_needed(source_position, TAB_DRAG_THRESHOLD)
            {
                cx.notify();
            }
            source.tab_drag.dragging_group() == Some(drag.group_id.as_str())
        })
    }

    fn set_incoming_native_tab_drag(
        &mut self,
        drag: IncomingTabDrag,
        zone: DockZone,
        cx: &mut Context<Self>,
    ) -> bool {
        let same_drag = self
            .incoming_tab_drag
            .as_ref()
            .is_some_and(|current| current.drag_id == drag.drag_id);
        if same_drag && self.incoming_tab_drop_zone == Some(zone) {
            return false;
        }
        self.incoming_tab_drag = Some(drag);
        self.incoming_tab_drop_zone = Some(zone);
        cx.notify();
        true
    }

    fn clear_incoming_native_tab_drag(&mut self, drag_id: u64, cx: &mut Context<Self>) -> bool {
        if !self
            .incoming_tab_drag
            .as_ref()
            .is_some_and(|drag| drag.drag_id == drag_id)
        {
            return false;
        }
        self.incoming_tab_drag = None;
        self.incoming_tab_drop_zone = None;
        cx.notify();
        true
    }

    fn retain_native_tab_hover(
        drag_id: u64,
        target_window: AnyWindowHandle,
        window: &mut Window,
        cx: &mut App,
    ) {
        let generation = crate::app::set_tab_drag_hover(drag_id, target_window);
        window.defer(cx, move |_window, cx| {
            if crate::app::tab_drag_hover_is_current(drag_id, target_window, generation) {
                crate::app::clear_incoming_tab_drag_except(drag_id, Some(target_window), cx);
            }
        });
    }

    fn defer_native_tab_hover_clear(drag_id: u64, window: &mut Window, cx: &mut App) {
        window.defer(cx, move |_window, cx| {
            if !crate::app::tab_drag_hover_exists(drag_id) {
                crate::app::clear_incoming_tab_drag_except(drag_id, None, cx);
            }
        });
    }

    /// Track a native tab drag from both possible platform event paths. Some
    /// platforms keep delivering the move to the source window while others
    /// deliver it to the window under the pointer.
    pub(crate) fn on_native_tab_drag_move(
        &mut self,
        drag: IncomingTabDrag,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current_window = window.window_handle();
        if drag.source_window != current_window {
            if !Self::promote_native_tab_drag_from_target(&drag, position, window, cx) {
                self.clear_incoming_native_tab_drag(drag.drag_id, cx);
                return;
            }
            let zone = self.native_tab_drop_zone(position);
            if let Some(zone) = zone {
                let changed = self.set_incoming_native_tab_drag(drag.clone(), zone, cx);
                if changed || !crate::app::tab_drag_hover_targets(drag.drag_id, current_window) {
                    Self::retain_native_tab_hover(drag.drag_id, current_window, window, cx);
                }
            } else {
                self.clear_incoming_native_tab_drag(drag.drag_id, cx);
                if crate::app::clear_tab_drag_hover_for_target(drag.drag_id, current_window) {
                    Self::defer_native_tab_hover_clear(drag.drag_id, window, cx);
                }
            }
            return;
        }

        let promoted = self
            .tab_drag
            .promote_if_needed(position, TAB_DRAG_THRESHOLD);
        if promoted {
            cx.notify();
        }
        if self.tab_drag.dragging_group() != Some(drag.group_id.as_str()) {
            return;
        }

        let target = self.native_tab_drop_target(position, window, cx);
        let outside_source = !cursor_inside_viewport(position, window.viewport_size());
        let over_other_window = target.is_some()
            || (outside_source
                && crate::app::find_window_at_screen_pos(
                    &current_window,
                    Self::screen_position(window, position),
                )
                .is_some());
        let group_count = self.workspace_state_mut().tab_groups().len();
        let reorder_changed = self.tab_drag.set_reorder_index(None);
        let detach_changed = self.tab_drag.set_outside(
            outside_source
                && should_offer_detach(
                    group_count,
                    position,
                    self.tab_bar_bounds,
                    over_other_window,
                ),
        );
        if reorder_changed || detach_changed {
            cx.notify();
        }

        if let Some(target) = target {
            let target_window = target.window;
            let changed = target.entity.update(cx, |target_state, cx| {
                target_state.set_incoming_native_tab_drag(drag.clone(), target.zone, cx)
            });
            if changed || !crate::app::tab_drag_hover_targets(drag.drag_id, target_window) {
                Self::retain_native_tab_hover(drag.drag_id, target_window, window, cx);
            }
        } else if crate::app::clear_tab_drag_hover_for_drag(drag.drag_id) {
            Self::defer_native_tab_hover_clear(drag.drag_id, window, cx);
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
                .workspace()
                .tab_groups()
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
            self.workspace_state_mut().tab_groups().len(),
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
                let group_ids = self.selected_drag_group_ids(&group_id);
                self.reorder_tab_groups(&group_ids, index, window, cx);
            }
            DropIntent::Detach { group_id } => {
                let group_ids = self.selected_drag_group_ids(&group_id);
                self.defer_groups_detach(group_ids, window, cx);
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

        let fallback_merge = self
            .native_tab_drag_payload(window.window_handle(), cx.entity())
            .zip(self.native_tab_drop_target(event.position, window, cx));

        // A source-window release has no reliable target drop callback. Clear
        // GPUI's process-wide drag before either merging into an existing
        // window or opening a detached one while mouse-up dispatch unwinds.
        cx.stop_active_drag(window);
        if let Some((drag, target)) = fallback_merge {
            self.tab_drag.set_outside(false);
            tracing::info!(
                zone = ?target.zone,
                "[tab-drag] committing source-window cross-window drop fallback"
            );
            Self::defer_native_cross_window_tab_drop(
                drag,
                target.window,
                target.entity,
                target.zone,
                window,
                cx,
            );
            return;
        }

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
                let group_ids = self.selected_drag_group_ids(&group_id);
                self.defer_groups_detach(group_ids, window, cx);
            }
            DropIntent::Reorder { .. } | DropIntent::None | DropIntent::Cancelled => cx.notify(),
        }
    }

    pub(crate) fn defer_native_cross_window_tab_drop(
        drag: IncomingTabDrag,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        zone: DockZone,
        window: &mut Window,
        cx: &mut App,
    ) {
        if drag.source_window == target_window {
            return;
        }
        crate::app::clear_tab_drag_hover();
        window.defer(cx, move |_window, cx| {
            crate::app::clear_incoming_tab_drag_except(drag.drag_id, None, cx);
            Self::finish_native_tab_drop(drag, target_window, target, zone, cx);
        });
    }

    fn close_empty_source_in_window(source: Entity<TinyShell>, window: &mut Window, cx: &mut App) {
        let should_remove = source.update(cx, |source, cx| {
            if !source.workspace().tab_groups().is_empty() {
                return false;
            }
            source.finalize_main_window_close(window, cx);
            true
        });
        if should_remove {
            window.remove_window();
        }
    }

    /// Wait until the current window callback has returned before resolving the
    /// source handle. GPUI temporarily removes the active window from its
    /// window table, so a nested update of that same window reports "not found".
    fn defer_close_empty_source_window(
        source_window: AnyWindowHandle,
        source: Entity<TinyShell>,
        cx: &mut App,
    ) {
        cx.defer(move |cx| {
            if let Err(error) = source_window.update(cx, move |_, window, cx| {
                Self::close_empty_source_in_window(source, window, cx);
            }) {
                tracing::warn!("[tab-drag] failed to close empty source window: {error:?}");
            }
        });
    }

    pub(crate) fn finish_native_tab_drop(
        drag: IncomingTabDrag,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        zone: DockZone,
        cx: &mut App,
    ) {
        if drag.source_window == target_window {
            return;
        }

        let source_window = drag.source_window;
        let source = drag.source;
        let anchor = drag.group_id;
        let should_close_source = source.update(cx, |source, cx| {
            source.tab_drag.cancel();
            if !source
                .workspace()
                .tab_groups()
                .iter()
                .any(|group| group.id == anchor)
            {
                cx.notify();
                return false;
            }
            let group_ids = source.selected_drag_group_ids(&anchor);
            let merged = source.commit_groups_merge(group_ids, target_window, target, zone, cx);
            should_close_empty_source(
                merged,
                source.workspace().tab_groups().is_empty(),
                &source_window,
                &target_window,
            )
        });
        if should_close_source {
            Self::defer_close_empty_source_window(source_window, source, cx);
        }
    }

    pub(crate) fn finish_native_local_tab_drop(
        &mut self,
        group_id: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let group_ids = self.selected_drag_group_ids(&group_id);
        self.tab_drag.cancel();
        if !self
            .workspace_state_mut()
            .tab_groups()
            .iter()
            .any(|group| group.id == group_id)
        {
            cx.notify();
            return;
        }

        if self
            .tab_bar_bounds
            .as_ref()
            .is_some_and(|bounds| bounds.contains(&position))
        {
            let ordered_bounds = self
                .workspace()
                .tab_groups()
                .iter()
                .filter_map(|group| {
                    self.tab_group_bounds
                        .get(&group.id)
                        .copied()
                        .map(|bounds| (group.id.clone(), bounds))
                })
                .collect::<Vec<_>>();
            if let Some(index) = reorder_index_at_x(&group_id, position.x, &ordered_bounds) {
                self.reorder_tab_groups(&group_ids, index, window, cx);
                return;
            }
        } else if self.workspace_state_mut().tab_groups().len() > group_ids.len() {
            self.defer_groups_detach(group_ids, window, cx);
            return;
        }

        cx.notify();
    }

    fn commit_groups_merge(
        &mut self,
        group_ids: Vec<String>,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        zone: DockZone,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut moved_any = false;
        for group_id in group_ids {
            if !self.commit_group_merge(group_id, target_window, target.clone(), zone, cx) {
                return false;
            }
            moved_any = true;
        }
        moved_any
    }

    fn commit_group_merge(
        &mut self,
        group_id: String,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        zone: DockZone,
        cx: &mut Context<Self>,
    ) -> bool {
        target.update(cx, |target, cx| {
            target.incoming_tab_drag = None;
            cx.notify();
        });
        let source_owner_id = self.session_owner_id;
        let merged = match self.take_group_transfer(&group_id) {
            Ok(transfer) => {
                let moved_search_target = transfer.tabs.iter().any(|(_, tab)| {
                    self.window_state.search_target_tab.as_deref() == Some(tab.id.as_str())
                });
                let result = target.update(cx, |target, cx| {
                    if zone.is_split() {
                        target.receive_group_transfer_docked(transfer, source_owner_id, zone, cx)
                    } else {
                        target.receive_group_transfer(transfer, source_owner_id, cx)
                    }
                });
                match result {
                    Ok(()) => {
                        let focus_handle = target.read(cx).focus_handle.clone();
                        crate::app::activate_window_with_retry(target_window, focus_handle, cx);
                        if moved_search_target {
                            self.window_state.search_target_tab = None;
                            self.window_state.search_matches.clear();
                            self.window_state.search_query.clear();
                            self.window_state.search_current = 0;
                        }
                        self.status = t!("tab_group_moved").into();
                        true
                    }
                    Err((message, transfer)) => {
                        self.restore_group_transfer(*transfer, cx);
                        self.status = t!("tab_group_move_failed", error = message).into();
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

    fn reorder_tab_groups(
        &mut self,
        group_ids: &[String],
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = group_ids.iter().cloned().collect::<HashSet<_>>();
        if selected.is_empty() {
            return;
        }
        let original = self.workspace().tab_groups().to_vec();
        let mut moving = original
            .iter()
            .filter(|group| selected.contains(&group.id))
            .cloned()
            .collect::<Vec<_>>();
        if moving.is_empty() {
            self.status = t!("cannot_reorder_tab_group").into();
            cx.notify();
            return;
        }
        let mut remaining = original
            .into_iter()
            .filter(|group| !selected.contains(&group.id))
            .collect::<Vec<_>>();
        let insert_at = index.min(remaining.len());
        let active_id = moving.first().map(|group| group.id.clone());
        for (offset, group) in moving.drain(..).enumerate() {
            remaining.insert(insert_at + offset, group);
        }
        *self.workspace_state_mut().tab_groups_mut() = remaining;
        if let Some(group_id) = active_id {
            self.activate_group(group_id, window, cx);
        }
        self.tabs_scroll_handle.scroll_to_item(insert_at);
        self.status = t!("tab_group_reordered").into();
        window.activate_window();
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn take_group_transfer(&mut self, group_id: &str) -> Result<GroupTransfer, String> {
        let transfer = self.prepare_group_for_transfer(group_id)?;
        self.clear_transferred_group(transfer.group_index);
        Ok(transfer)
    }

    fn prepare_group_for_transfer(&mut self, group_id: &str) -> Result<GroupTransfer, String> {
        let group_index = self
            .workspace()
            .tab_groups()
            .iter()
            .position(|group| group.id == group_id)
            .ok_or_else(|| "cannot move: source group no longer exists".to_string())?;
        let group = self.workspace_state_mut().tab_groups()[group_index].clone();
        let tab_ids = group
            .pane_root
            .tab_ids()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if tab_ids.is_empty() || tab_ids.iter().any(String::is_empty) {
            return Err("cannot move: source group has no terminal panes".to_string());
        }
        let tab_id_set = tab_ids.iter().cloned().collect::<HashSet<_>>();
        if tab_id_set.len() != tab_ids.len() {
            return Err("cannot move: source group contains duplicate terminal ids".to_string());
        }
        if tab_ids.iter().any(|tab_id| {
            !self
                .workspace_state_mut()
                .tabs()
                .iter()
                .any(|tab| tab.id == *tab_id)
        }) {
            return Err("cannot move: a source terminal no longer exists".to_string());
        }

        let was_active_group =
            self.workspace_state_mut().active_group_value().as_deref() == Some(group_id);
        let active_tab = self
            .workspace()
            .active_tab_id()
            .map(str::to_owned)
            .filter(|tab_id| tab_id_set.contains(tab_id.as_str()));
        let mut tabs = Vec::with_capacity(tab_ids.len());
        let mut remaining_tabs = Vec::with_capacity(self.workspace().tabs().len() - tab_ids.len());
        for (index, tab) in self
            .workspace_state_mut()
            .take_tabs()
            .into_iter()
            .enumerate()
        {
            if tab_id_set.contains(tab.id.as_str()) {
                tabs.push((index, tab));
            } else {
                remaining_tabs.push(tab);
            }
        }
        self.workspace_state_mut().replace_tabs(remaining_tabs);

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

        let mut route_ids = tab_ids.clone();
        route_ids.extend(sftp_handles.keys().cloned());
        route_ids.sort();
        route_ids.dedup();

        let (system_info_tabs, active_system_info_tab) =
            crate::app::terminal_workspace::extract_system_info_transfer(
                self.workspace().system_info_tabs(),
                self.workspace().active_system_info_tab_id(),
                &tab_ids,
            );

        Ok(GroupTransfer {
            group,
            group_index,
            tabs,
            sftp_handles,
            route_ids,
            active_tab,
            system_info_tabs,
            active_system_info_tab,
            was_active_group,
        })
    }

    fn clear_transferred_group(&mut self, group_index: usize) {
        let active_group_id = self.workspace().active_group_id().map(str::to_owned);
        let group_id = self.workspace().tab_groups()[group_index].id.clone();
        let was_active_group = active_group_id.as_deref() == Some(group_id.as_str());
        let transferred_tab_ids = self.workspace().tab_groups()[group_index]
            .pane_root
            .tab_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let removed_active_info =
            self.workspace()
                .active_system_info_tab_id()
                .is_some_and(|id| {
                    self.workspace().system_info_tabs().iter().any(|info| {
                        info.id == id && transferred_tab_ids.contains(&info.source_tab_id)
                    })
                });
        self.workspace_state_mut()
            .system_info_tabs_mut()
            .retain(|info| !transferred_tab_ids.contains(&info.source_tab_id));
        if removed_active_info {
            self.workspace_state_mut().clear_active_system_info_tab();
        }
        self.workspace_state_mut().remove_group_at(group_index);
        if was_active_group {
            self.activate_after_group_extraction(group_index);
        } else {
            self.sync_system_tab_to_active_group();
        }
    }

    fn activate_after_group_extraction(&mut self, removed_index: usize) {
        if self.workspace_state_mut().tab_groups().is_empty() {
            self.workspace_state_mut().set_pane_root(PaneLayout::Empty);
            self.workspace_state_mut().clear_focused_pane_path();
            self.workspace_state_mut().set_active_tab(None);
            self.workspace_state_mut().set_active_group(None);
            self.reset_sftp_tree_for_active_group();
            self.home_page_open = true;
            self.sync_system_tab_to_active_group();
            return;
        }

        let next_index = removed_index.min(self.workspace_state_mut().tab_groups().len() - 1);
        let next_group = &self.workspace_state_mut().tab_groups()[next_index];
        let next_group_id = next_group.id.clone();
        let next_layout = next_group.pane_root.clone();
        let next_tab = next_layout.tab_ids().first().copied().map(str::to_string);
        self.workspace_state_mut()
            .set_active_group(Some(next_group_id));
        self.workspace_state_mut().set_pane_root(next_layout);
        self.workspace_state_mut().clear_focused_pane_path();
        self.workspace_state_mut().set_active_tab(next_tab.clone());
        if let Some(tab_id) = next_tab {
            self.focus_pane_with_id(tab_id);
        }
        self.reset_sftp_tree_for_active_group();
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

        let group_index = transfer
            .group_index
            .min(self.workspace_state_mut().tab_groups().len());
        let group_id = transfer.group.id.clone();
        let group_layout = transfer.group.pane_root.clone();
        let system_info_tabs = transfer.system_info_tabs.clone();
        let active_system_info_tab = transfer.active_system_info_tab.clone();
        self.workspace_state_mut()
            .insert_group(group_index, transfer.group);
        transfer.tabs.sort_by_key(|(index, _)| *index);
        for (index, tab) in transfer.tabs {
            let insert_at = index.min(self.workspace().tabs().len());
            self.workspace_state_mut().insert_tab(insert_at, tab);
        }
        self.sftp_handles.extend(transfer.sftp_handles);

        let restored_active_info = crate::app::terminal_workspace::restore_system_info_transfer(
            self.workspace_state_mut().system_info_tabs_mut(),
            system_info_tabs,
            active_system_info_tab,
        );

        if transfer.was_active_group {
            self.workspace_state_mut().set_active_group(Some(group_id));
            self.workspace_state_mut().set_pane_root(group_layout);
            self.workspace_state_mut().clear_focused_pane_path();
            self.workspace_state_mut()
                .set_active_system_info_tab(restored_active_info);
            let active_tab = transfer.active_tab.or_else(|| {
                self.workspace()
                    .pane_root()
                    .tab_ids()
                    .first()
                    .copied()
                    .map(str::to_string)
            });
            self.workspace_state_mut().set_active_tab(active_tab);
            if let Some(tab_id) = self.workspace_state_mut().active_tab_value() {
                self.focus_pane_with_id(tab_id);
            }
            self.reset_sftp_tree_for_active_group();
        }
        self.sync_system_tab_to_active_group();
        cx.notify();
    }

    /// Receive an intact group from another window without recreating any
    /// terminal or SFTP backend. The group remains a separate top-level tab
    /// because `TabGroup` owns a single SFTP UI state.
    pub(crate) fn receive_group_transfer(
        &mut self,
        transfer: GroupTransfer,
        source_owner_id: crate::session::store::WindowOwnerId,
        cx: &mut Context<Self>,
    ) -> Result<(), (String, Box<GroupTransfer>)> {
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
                Box::new(transfer),
            ));
        }
        if self
            .workspace()
            .tab_groups()
            .iter()
            .any(|group| group.id == transfer.group.id)
        {
            return Err((
                "target already contains this group".to_string(),
                Box::new(transfer),
            ));
        }
        if self
            .workspace()
            .tabs()
            .iter()
            .any(|tab| tab_ids.contains(tab.id.as_str()))
        {
            return Err((
                "target already contains one of the transferred terminals".to_string(),
                Box::new(transfer),
            ));
        }
        if transfer
            .sftp_handles
            .keys()
            .any(|handle_id| self.sftp_handles.contains_key(handle_id))
        {
            return Err((
                "target already contains one of the transferred SFTP handles".to_string(),
                Box::new(transfer),
            ));
        }

        let target_owner_id = self.session_owner_id;
        let routes_moved = self.session_store.update(cx, |store, _| {
            store.move_event_routes(&transfer.route_ids, source_owner_id, target_owner_id)
        });
        if !routes_moved {
            return Err((
                "backend event routes changed before the move could commit".to_string(),
                Box::new(transfer),
            ));
        }

        let GroupTransfer {
            group,
            mut tabs,
            sftp_handles,
            active_tab,
            system_info_tabs,
            active_system_info_tab,
            ..
        } = transfer;
        let valid_tab_ids = tabs
            .iter()
            .map(|(_, tab)| tab.id.clone())
            .collect::<Vec<_>>();
        let active_tab = crate::app::terminal_workspace::choose_transfer_active_tab(
            active_tab.as_deref(),
            &group.pane_root,
            &valid_tab_ids,
        );
        let pane_order = group.pane_root.tab_ids();
        tabs.sort_by_key(|(_, tab)| {
            pane_order
                .iter()
                .position(|id| *id == tab.id)
                .unwrap_or(usize::MAX)
        });
        let group_id = self.create_group_for_transfer(group, cx);
        let _fallback_tab = self
            .workspace()
            .pane_root()
            .tab_ids()
            .first()
            .copied()
            .map(str::to_string);
        self.adopt_transferred_tabs(
            group_id,
            tabs,
            sftp_handles,
            active_tab,
            system_info_tabs,
            active_system_info_tab,
            cx,
        );
        Ok(())
    }

    fn receive_group_transfer_docked(
        &mut self,
        transfer: GroupTransfer,
        source_owner_id: crate::session::store::WindowOwnerId,
        zone: DockZone,
        cx: &mut Context<Self>,
    ) -> Result<(), (String, Box<GroupTransfer>)> {
        if !zone.is_split() || self.workspace().active_group_id().is_none() {
            return self.receive_group_transfer(transfer, source_owner_id, cx);
        }
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
                Box::new(transfer),
            ));
        }
        if self
            .workspace()
            .tabs()
            .iter()
            .any(|tab| tab_ids.contains(tab.id.as_str()))
        {
            return Err((
                "target already contains one of the transferred terminals".to_string(),
                Box::new(transfer),
            ));
        }
        if transfer
            .sftp_handles
            .keys()
            .any(|id| self.sftp_handles.contains_key(id))
        {
            return Err((
                "target already contains one of the transferred SFTP handles".to_string(),
                Box::new(transfer),
            ));
        }

        let target_owner_id = self.session_owner_id;
        if !self.session_store.update(cx, |store, _| {
            store.move_event_routes(&transfer.route_ids, source_owner_id, target_owner_id)
        }) {
            return Err((
                "backend event routes changed before the move could commit".to_string(),
                Box::new(transfer),
            ));
        }

        let GroupTransfer {
            group,
            tabs,
            sftp_handles,
            active_tab,
            system_info_tabs,
            active_system_info_tab,
            ..
        } = transfer;
        let incoming_layout = group.pane_root;
        let existing_layout = self.workspace().pane_root().clone();
        let merged = match zone {
            DockZone::Left => PaneLayout::Vertical(vec![incoming_layout, existing_layout], 0.5),
            DockZone::Right => PaneLayout::Vertical(vec![existing_layout, incoming_layout], 0.5),
            DockZone::Up => PaneLayout::Horizontal(vec![incoming_layout, existing_layout], 0.5),
            DockZone::Down => PaneLayout::Horizontal(vec![existing_layout, incoming_layout], 0.5),
            DockZone::Center => unreachable!("center docking handled as a tab merge"),
        };
        self.workspace_state_mut().set_pane_root(merged.clone());
        if let Some(group_id) = self.workspace().active_group_id().map(str::to_owned)
            && let Some(target_group) = self.tab_group_mut(&group_id)
        {
            target_group.pane_root = merged;
        }
        self.workspace_state_mut()
            .append_tabs(tabs.into_iter().map(|(_, tab)| tab).collect());
        self.sftp_handles.extend(sftp_handles);
        let restored_info = crate::app::terminal_workspace::restore_system_info_transfer(
            self.workspace_state_mut().system_info_tabs_mut(),
            system_info_tabs,
            active_system_info_tab,
        );
        self.workspace_state_mut()
            .set_active_system_info_tab(restored_info);
        if let Some(active_tab) = active_tab {
            self.workspace_state_mut()
                .set_active_tab(Some(active_tab.clone()));
            self.focus_pane_with_id(active_tab);
        }
        self.home_page_open = false;
        self.reset_sftp_tree_for_active_group();
        self.sync_system_tab_to_active_group();
        cx.notify();
        Ok(())
    }

    pub(crate) fn move_group_to_window(
        &mut self,
        group_id: String,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        source_window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        let merged =
            self.commit_groups_merge(vec![group_id], target_window, target, DockZone::Center, cx);
        if should_close_empty_source(
            merged,
            self.workspace().tab_groups().is_empty(),
            &source_window,
            &target_window,
        ) {
            let source = cx.entity();
            Self::defer_close_empty_source_window(source_window, source, cx);
        }
    }

    pub(crate) fn move_active_group_to_adjacent_window(
        &mut self,
        source_window: AnyWindowHandle,
        reverse: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(group_id) = self.workspace().active_group_id().map(str::to_owned) else {
            return;
        };
        let mut targets = crate::app::other_main_windows(source_window);
        if reverse {
            targets.reverse();
        }
        let Some((target_window, target)) = targets.into_iter().next() else {
            self.status = t!("tab_move_no_target_window").into();
            cx.notify();
            return;
        };
        self.move_group_to_window(group_id, target_window, target, source_window, cx);
    }

    pub(crate) fn merge_window_into(
        &mut self,
        source_window: AnyWindowHandle,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        cx: &mut Context<Self>,
    ) {
        let group_ids = self
            .workspace()
            .tab_groups()
            .iter()
            .map(|group| group.id.clone())
            .collect::<Vec<_>>();
        if group_ids.is_empty() {
            return;
        }
        let merged =
            self.commit_groups_merge(group_ids, target_window, target, DockZone::Center, cx);
        if merged && self.workspace().tab_groups().is_empty() {
            let source = cx.entity();
            Self::defer_close_empty_source_window(source_window, source, cx);
        }
    }

    pub(crate) fn dock_pane(
        &mut self,
        group_id: &str,
        source_tab_id: &str,
        target_tab_id: &str,
        zone: DockZone,
        cx: &mut Context<Self>,
    ) {
        if source_tab_id == target_tab_id || !zone.is_split() {
            return;
        }
        if self.workspace().active_group_id() != Some(group_id) {
            return;
        }
        let mut layout = self.workspace().pane_root().clone();
        if !layout.contains(source_tab_id) || !layout.contains(target_tab_id) {
            return;
        }
        if !layout.remove_tab(source_tab_id) {
            return;
        }

        fn dock_at(
            layout: &mut PaneLayout,
            target: &str,
            source: PaneLayout,
            zone: DockZone,
        ) -> bool {
            match layout {
                PaneLayout::Single(id) if id == target => {
                    let current = PaneLayout::Single(id.clone());
                    *layout = match zone {
                        DockZone::Left => PaneLayout::Vertical(vec![source, current], 0.5),
                        DockZone::Right => PaneLayout::Vertical(vec![current, source], 0.5),
                        DockZone::Up => PaneLayout::Horizontal(vec![source, current], 0.5),
                        DockZone::Down => PaneLayout::Horizontal(vec![current, source], 0.5),
                        DockZone::Center => return false,
                    };
                    true
                }
                PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                    for child in children {
                        if dock_at(child, target, source.clone(), zone) {
                            return true;
                        }
                    }
                    false
                }
                PaneLayout::Empty | PaneLayout::Single(_) => false,
            }
        }

        if dock_at(
            &mut layout,
            target_tab_id,
            PaneLayout::Single(source_tab_id.to_string()),
            zone,
        ) {
            self.workspace_state_mut().set_pane_root(layout.clone());
            if let Some(group) = self.tab_group_mut(group_id) {
                group.pane_root = layout;
            }
            self.workspace_state_mut()
                .set_active_tab(Some(source_tab_id.to_string()));
            self.focus_pane_with_id(source_tab_id.to_string());
            cx.notify();
        }
    }

    fn create_group_for_transfer(&mut self, group: TabGroup, _cx: &mut Context<Self>) -> String {
        let group_id = group.id.clone();
        let pane_root = group.pane_root.clone();
        let ordinal = group.ordinal;
        let next_ordinal = self.workspace().next_tab_group_ordinal().max(ordinal + 1);
        self.workspace_state_mut()
            .set_next_tab_group_ordinal(next_ordinal);
        self.workspace_state_mut().push_group(group);
        self.home_page_open = false;
        self.workspace_state_mut().clear_active_system_info_tab();
        self.workspace_state_mut()
            .set_active_group(Some(group_id.clone()));
        self.workspace_state_mut().set_pane_root(pane_root);
        self.workspace_state_mut().clear_focused_pane_path();
        self.reset_sftp_tree_for_active_group();
        group_id
    }

    #[allow(clippy::too_many_arguments)]
    fn adopt_transferred_tabs(
        &mut self,
        _group_id: String,
        tabs: Vec<(usize, TerminalTab)>,
        sftp_handles: HashMap<String, crate::sftp::SftpHandle>,
        active_tab: Option<String>,
        system_info_tabs: Vec<SystemInfoTab>,
        active_system_info_tab: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.workspace_state_mut()
            .append_tabs(tabs.into_iter().map(|(_, tab)| tab).collect());
        self.sftp_handles.extend(sftp_handles);
        let restored_active_info = crate::app::terminal_workspace::restore_system_info_transfer(
            self.workspace_state_mut().system_info_tabs_mut(),
            system_info_tabs,
            active_system_info_tab,
        );
        self.workspace_state_mut()
            .set_active_system_info_tab(restored_active_info);
        self.workspace_state_mut().set_active_tab(active_tab);
        if let Some(tab_id) = self.workspace_state_mut().active_tab_value() {
            self.focus_pane_with_id(tab_id);
        }
        self.sync_system_tab_to_active_group();
        self.status = t!("tab_group_received").into();
        cx.notify();
    }
}
