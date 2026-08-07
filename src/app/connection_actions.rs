use gpui::{Context, Window};
use rust_i18n::t;
use uuid::Uuid;

use crate::{
    PaneLayout, TabGroup, TinyShell,
    app::config_persistence,
    backend::{local, ssh},
    session::config::{AuthMethod, Session},
    terminal::{BackendCommand, TerminalTab},
};

impl TinyShell {
    pub(crate) fn edit_saved_session(
        &mut self,
        session_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.config.get(&session_id).cloned() else {
            self.status = t!("saved_session_not_found").into();
            cx.notify();
            return;
        };
        let owner = cx.entity();
        window.defer(cx, move |_, cx| {
            crate::app::connection_manager::ssh_editor_window::open(
                owner,
                crate::app::connection_manager::ssh_editor_window::SshEditorRequest::Edit {
                    session,
                },
                cx,
            );
        });
    }

    pub(crate) fn clone_saved_session(
        &mut self,
        session_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.config.get(&session_id).cloned() else {
            self.status = t!("saved_session_not_found").into();
            cx.notify();
            return;
        };
        let owner = cx.entity();
        window.defer(cx, move |_, cx| {
            crate::app::connection_manager::ssh_editor_window::open(
                owner,
                crate::app::connection_manager::ssh_editor_window::SshEditorRequest::Clone {
                    session,
                },
                cx,
            );
        });
    }

    pub(crate) fn remove_saved_session(&mut self, session_id: String, cx: &mut Context<Self>) {
        self.config.remove(&session_id);
        if let Err(err) = config_persistence::save_full(&self.config_repository, &self.config) {
            tracing::warn!("failed to save config: {err:#}");
        }
        self.status = t!("session_removed").into();
        cx.notify();
    }

    pub(crate) fn set_ssh_auth_method(&mut self, method: AuthMethod, cx: &mut Context<Self>) {
        self.ssh_auth_method = method;
        if method == AuthMethod::Config {
            self.refresh_ssh_config();
            self.ssh_config_selected = None;
        }
        cx.notify();
    }

    pub(crate) fn refresh_ssh_config(&mut self) {
        self.ssh_config_entries =
            crate::session::ssh_config::parse_ssh_config().unwrap_or_default();
    }

    pub(crate) fn select_ssh_config_entry(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ssh_config_selected = Some(index);
        if let Some(entry) = self.ssh_config_entries.get(index) {
            Self::set_input_value(
                &self.connection_inputs.session_name_input,
                entry.host_alias.clone(),
                window,
                cx,
            );
            Self::set_input_value(
                &self.connection_inputs.host_input,
                entry.hostname.clone(),
                window,
                cx,
            );
            Self::set_input_value(
                &self.connection_inputs.port_input,
                entry.port.to_string(),
                window,
                cx,
            );
            // If no user specified in config, use current system user
            let user = if entry.user.is_empty() {
                std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "root".to_string())
            } else {
                entry.user.clone()
            };
            Self::set_input_value(&self.connection_inputs.user_input, user, window, cx);
            Self::set_input_value(
                &self.connection_inputs.key_path_input,
                entry.identity_files.first().cloned().unwrap_or_default(),
                window,
                cx,
            );
            Self::set_input_value(
                &self.connection_inputs.password_input,
                String::new(),
                window,
                cx,
            );
            Self::set_input_value(
                &self.connection_inputs.key_inline_input,
                String::new(),
                window,
                cx,
            );
            Self::set_input_value(
                &self.connection_inputs.passphrase_input,
                String::new(),
                window,
                cx,
            );
            // Auto-connect on selection
            self.connect_ssh(window, cx);
        }
    }

    pub(crate) fn set_ssh_proxy_type(&mut self, proxy_type: String, cx: &mut Context<Self>) {
        self.ssh_proxy_type = proxy_type;
        cx.notify();
    }

    pub(crate) fn connect_saved_session(
        &mut self,
        session_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(
            "[ui] user clicked to connect saved session '{}'",
            session_id
        );
        let Some(session) = self.config.get(&session_id).cloned() else {
            self.status = t!("saved_session_not_found").into();
            cx.notify();
            return;
        };
        if session.requires_credential_prompt() {
            let owner = cx.entity();
            window.defer(cx, move |_, cx| {
                crate::app::connection_manager::ssh_editor_window::open(
                    owner,
                    crate::app::connection_manager::ssh_editor_window::SshEditorRequest::Credentials {
                        session,
                    },
                    cx,
                );
            });
            return;
        }
        self.open_ssh_session(session, cx);
    }

    pub(crate) fn open_ssh_session(&mut self, mut session: Session, cx: &mut Context<Self>) {
        self.set_active_system_info_tab(None);
        self.home_page_open = false;
        let ordinal = self.allocate_tab_group_ordinal();
        tracing::info!(
            "[session] opening ssh tab for session '{}' ({}@{})",
            session.name,
            session.user,
            session.host
        );

        // Persist recency on the saved session, so Overview can show an
        // accurate, stable "recently used" list across application restarts.
        let last_used = chrono::Local::now().to_rfc3339();
        session.last_used = Some(last_used.clone());
        if let Some(mut saved_session) = self.config.get(&session.id).cloned() {
            saved_session.last_used = Some(last_used);
            self.config.upsert(saved_session);
            if let Err(err) =
                config_persistence::save_full_async(&self.config_repository, &self.config)
            {
                tracing::warn!("failed to save session recency: {err:#}");
            }
        }

        // Resolve managed key reference: fill inline content from the
        // ManagedKey so the backend can authenticate without the original file.
        if let Some(mk_id) = &session.managed_key_id {
            if let Some(mk) = self.config.get_managed_key(mk_id) {
                session.private_key_inline = mk.inline_content.clone();
                session.private_key_path.clear();
                if session.passphrase.is_empty() {
                    session.passphrase = mk.passphrase.clone();
                }
            } else {
                tracing::warn!(
                    "[session] managed key '{}' not found, falling back to explicit key",
                    mk_id
                );
            }
        }

        let id = Uuid::new_v4().to_string();
        let events = self.backend_events_sender(cx);
        let proxy_config = self.config.clone();
        self.register_backend_route(id.clone(), cx);
        let backend = ssh::spawn_ssh_terminal(
            self.runtime.handle(),
            ssh::SshTerminalRequest::new(
                id.clone(),
                session.clone(),
                proxy_config.clone(),
                crate::app::constants::DEFAULT_COLS,
                crate::app::constants::DEFAULT_ROWS,
                1,
            ),
            events.clone(),
        );
        let tab = TerminalTab::new_ssh(id.clone(), &session, backend, events.clone());
        let group_id = Uuid::new_v4().to_string();
        let group = TabGroup {
            id: group_id.clone(),
            ordinal,
            title: session.name.clone(),
            pane_root: PaneLayout::Single(id.clone()),
            sftp: Some(crate::terminal::SftpUiState {
                current_path: "/".into(),
                status: rust_i18n::t!("sftp_connecting").to_string(),
                entries: Vec::new(),
                directory_entries: std::collections::HashMap::new(),
                expanded_directories: std::collections::HashSet::new(),
                selected_path: None,
                selected_entries: std::collections::HashSet::new(),
                home_dir: String::new(),
                follow_terminal_cwd: false,
                initial_terminal_cwd_synced: false,
                latency_ms: None,
            }),
        };
        self.install_terminal_tab(tab, group);
        if let Some(tab) = self.terminal_tab_mut(&id) {
            tab.feed_status_line(&rust_i18n::t!("starting_connection"));
        }
        self.sftp_workspace.pending_path_sync = Some("/".into());
        self.tabs_scroll_handle
            .scroll_to_item(self.terminal_tab_count() - 1);
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
        cx.notify();
        self.register_backend_route(group_id.clone(), cx);
        let session_id = session.id.clone();
        let sftp_handle = crate::sftp::spawn_sftp(
            self.runtime.handle(),
            group_id.clone(),
            session,
            proxy_config,
            events,
        );
        self.sftp_handles
            .insert(group_id.clone(), sftp_handle.clone());
        self.transfer_manager.bind_session(session_id, sftp_handle);
        self.status = t!("ssh_tab_opened").into();
        cx.notify();
    }

    /// Retry a single disconnected tab by its ID.
    /// For SSH tabs: spawns a new SSH connection and restarts SFTP.
    /// For local tabs: spawns a new local shell.
    ///
    /// The existing `TerminalTab` (including its `term` scrollback history)
    /// is preserved — only the backend is swapped via `set_backend()`.
    pub(crate) fn retry_disconnected_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let Some(ix) = self
            .window_state_mut()
            .workspace_state_mut()
            .tabs_mut()
            .iter()
            .position(|t| t.id == tab_id)
        else {
            return;
        };
        if self.window_state_mut().workspace_state_mut().tabs_mut()[ix].connected
            || self.window_state_mut().workspace_state_mut().tabs_mut()[ix]
                .disconnected_reason
                .is_none()
        {
            return;
        }
        self.terminal_completions.remove(tab_id);

        let is_ssh = self.window_state_mut().workspace_state_mut().tabs_mut()[ix]
            .session
            .is_some();
        let session = self.window_state_mut().workspace_state_mut().tabs_mut()[ix]
            .session
            .clone();
        let new_generation = self.window_state_mut().workspace_state_mut().tabs_mut()[ix]
            .backend_generation
            .saturating_add(1);
        let cols = self.window_state_mut().workspace_state_mut().tabs_mut()[ix].cols;
        let rows = self.window_state_mut().workspace_state_mut().tabs_mut()[ix].rows;
        let events = self.backend_events_sender(cx);
        let proxy_config = self.config.clone();
        self.register_backend_route(tab_id.to_string(), cx);

        // Close old backend (sends Close through the shared Arc<Mutex>)
        self.window_state_mut().workspace_state_mut().tabs_mut()[ix]
            .send_backend(BackendCommand::Close);

        if let Some(session) = session {
            // SSH tab: spawn new SSH connection
            let backend = ssh::spawn_ssh_terminal(
                self.runtime.handle(),
                ssh::SshTerminalRequest::new(
                    tab_id.to_string(),
                    session.clone(),
                    proxy_config.clone(),
                    cols,
                    rows,
                    new_generation,
                ),
                events.clone(),
            );

            // Swap the backend — the Term's internal listener shares the
            // same Arc<Mutex<BackendTx>>, so user input is automatically
            // routed to the new backend. Terminal history is preserved.
            self.window_state_mut().workspace_state_mut().tabs_mut()[ix].set_backend(backend);
            self.window_state_mut().workspace_state_mut().tabs_mut()[ix].connected = false;
            self.window_state_mut().workspace_state_mut().tabs_mut()[ix].status =
                "connecting".into();
            self.window_state_mut().workspace_state_mut().tabs_mut()[ix].disconnected_reason = None;
            self.window_state_mut().workspace_state_mut().tabs_mut()[ix].backend_generation =
                new_generation;
            self.window_state_mut().workspace_state_mut().tabs_mut()[ix]
                .feed_status_line(&rust_i18n::t!("starting_connection"));

            // Restart SFTP for the group containing this tab
            if let Some(group) = self
                .workspace()
                .tab_groups()
                .iter()
                .find(|g| g.pane_root.contains(tab_id))
            {
                let group_id = group.id.clone();
                let group_session = self
                    .workspace()
                    .tabs()
                    .iter()
                    .find(|t| group.pane_root.contains(&t.id) && t.session.is_some())
                    .and_then(|t| t.session.clone());

                if let Some(session) = group_session {
                    let session_id = session.id.clone();
                    if let Some(old_handle) = self.sftp_handles.remove(&group_id) {
                        old_handle.close();
                    }
                    self.transfer_manager.unbind_session(&session_id);
                    self.register_backend_route(group_id.clone(), cx);
                    let sftp_handle = crate::sftp::spawn_sftp(
                        self.runtime.handle(),
                        group_id.clone(),
                        session,
                        proxy_config.clone(),
                        events.clone(),
                    );
                    self.sftp_handles
                        .insert(group_id.clone(), sftp_handle.clone());
                    self.transfer_manager.bind_session(session_id, sftp_handle);

                    if let Some(group) = self
                        .window_state_mut()
                        .workspace_state_mut()
                        .tab_groups_mut()
                        .iter_mut()
                        .find(|g| g.id == group_id)
                    {
                        if let Some(sftp) = group.sftp.as_mut() {
                            sftp.status = rust_i18n::t!("sftp_connecting").to_string();
                        }
                    }
                }
            }
        } else {
            // Local tab: spawn new local shell
            match local::spawn_local_terminal(
                tab_id.to_string(),
                cols,
                rows,
                events,
                new_generation,
            ) {
                Ok(backend) => {
                    // Swap the backend — preserves terminal history.
                    self.window_state_mut().workspace_state_mut().tabs_mut()[ix]
                        .set_backend(backend);
                    self.window_state_mut().workspace_state_mut().tabs_mut()[ix].connected = true;
                    self.window_state_mut().workspace_state_mut().tabs_mut()[ix].status =
                        t!("local_shell").into();
                    self.window_state_mut().workspace_state_mut().tabs_mut()[ix]
                        .disconnected_reason = None;
                    self.window_state_mut().workspace_state_mut().tabs_mut()[ix]
                        .backend_generation = new_generation;
                    // Resize the new PTY to match the pane dimensions.
                    self.window_state_mut().workspace_state_mut().tabs_mut()[ix]
                        .send_backend(BackendCommand::Resize { cols, rows });
                }
                Err(err) => {
                    self.status =
                        t!("local_terminal_reopen_failed", err = format!("{err:#}")).into();
                    cx.notify();
                    return;
                }
            }
        }

        self.status = if is_ssh {
            t!("ssh_tab_retrying")
        } else {
            t!("local_tab_reopened")
        }
        .into();
        cx.notify();
    }
}
