pub mod config;
pub mod connection_archive;
pub mod connection_catalog;
pub mod connection_import;
pub mod quick_commands;
pub mod ssh_config;
pub mod ssh_keys;
pub mod store;

mod terminal_preferences;

use gpui::{AppContext as _, Context, Entity, SharedString, Window, px};
use gpui_component::{Theme, WindowExt as _, input::InputState};
use rust_i18n::t;
use uuid::Uuid;

use self::{
    config::{AuthMethod, ManagedKey, Session, TerminalDisplayStyle},
    terminal_preferences::{terminal_cell_width_for, terminal_line_height_for},
};

use crate::{
    PaneLayout, TabGroup, TinyShell,
    app::constants::{DEFAULT_COLS, DEFAULT_ROWS},
    backend::{local, ssh},
    terminal::{BackendCommand, TerminalTab},
};

impl TinyShell {
    pub(crate) fn set_input_value(
        input: &Entity<InputState>,
        value: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        input.update(cx, |state, cx| state.set_value(value, window, cx));
    }

    #[allow(dead_code)]
    pub(crate) fn load_session_into_form(
        &mut self,
        session: &Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_session_id = Some(session.id.clone());
        self.session_group_selection = session.group.clone();
        self.ssh_auth_method = session.auth;
        // Restore managed key selection or custom path mode.
        self.managed_key_selected = session.managed_key_id.clone();
        self.using_custom_key_path =
            matches!(session.auth, AuthMethod::Key | AuthMethod::KeyPending)
                && session.managed_key_id.is_none()
                && !session.private_key_path.is_empty();
        Self::set_input_value(&self.session_name_input, session.name.clone(), window, cx);
        Self::set_input_value(&self.host_input, session.host.clone(), window, cx);
        Self::set_input_value(&self.port_input, session.port.to_string(), window, cx);
        Self::set_input_value(&self.user_input, session.user.clone(), window, cx);
        Self::set_input_value(&self.password_input, session.password.clone(), window, cx);
        Self::set_input_value(
            &self.key_path_input,
            session.private_key_path.clone(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.key_inline_input,
            session.private_key_inline.clone(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.passphrase_input,
            session.passphrase.clone(),
            window,
            cx,
        );
        self.ssh_proxy_type = if session.proxy_type.is_empty() {
            "none".to_string()
        } else {
            session.proxy_type.clone()
        };
        Self::set_input_value(
            &self.proxy_host_input,
            session.proxy_host.clone(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.proxy_port_input,
            session
                .proxy_port
                .map(|p| p.to_string())
                .unwrap_or_default(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.proxy_user_input,
            session.proxy_user.clone(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.proxy_password_input,
            session.proxy_password.clone(),
            window,
            cx,
        );
    }

    pub(crate) fn pick_ssh_key_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cancellation = self.async_runtime.supervisor.start("pick-ssh-key-path");
        let start_dir = directories::BaseDirs::new()
            .map(|d| d.home_dir().join(".ssh"))
            .unwrap_or_else(|| std::path::PathBuf::from("/"));

        let file_dialog = rfd::AsyncFileDialog::new()
            .set_directory(start_dir)
            .pick_file();

        cx.spawn_in(window, async move |this, cx| {
            if let Some(file) = file_dialog.await {
                if cancellation.is_cancelled() {
                    return Ok::<(), anyhow::Error>(());
                }
                let _ = gpui::AsyncWindowContext::update(cx, |window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        Self::set_input_value(
                            &this.key_path_input,
                            file.path().to_string_lossy().to_string(),
                            window,
                            cx,
                        );
                    });
                });
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    // ── Managed SSH keys ────────────────────────────────────────────

    pub(crate) fn open_managed_key_selector_for_editor(
        &mut self,
        editor: Entity<crate::app::connection_manager::ssh_editor_window::SshEditorWindow>,
        selected: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.managed_key_editor_target = Some(editor);
        self.managed_key_dialog_selection = selected;
        self.active_dialog = None;
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                this.show_managed_key_selector_dialog(window, cx);
            });
        });
    }

    pub(crate) fn open_managed_key_selector(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.managed_key_editor_target = None;
        self.managed_key_dialog_selection = self.managed_key_selected.clone();
        self.active_dialog = None;
        window.close_dialog(cx);
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                this.show_managed_key_selector_dialog(window, cx);
            });
        });
    }

    pub(crate) fn select_managed_key_candidate(&mut self, key_id: String, cx: &mut Context<Self>) {
        self.managed_key_dialog_selection = Some(key_id);
        cx.notify();
    }

    pub(crate) fn begin_managed_key_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key_id) = self.managed_key_dialog_selection.clone() else {
            return;
        };
        let Some(key) = self.managed_keys.iter().find(|key| key.id == key_id) else {
            return;
        };
        self.editing_managed_key_id = Some(key_id);
        Self::set_input_value(&self.key_import_remark_input, key.name.clone(), window, cx);
        crate::app::input_focus::defer_focus_input_at_end(
            self.key_import_remark_input.clone(),
            window,
            cx,
        );
        cx.notify();
    }

    pub(crate) fn save_managed_key_rename(&mut self, cx: &mut Context<Self>) {
        let Some(key_id) = self.editing_managed_key_id.clone() else {
            return;
        };
        let name = self
            .key_import_remark_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if name.is_empty() {
            return;
        }
        self.rename_managed_key(key_id, name, cx);
    }

    pub(crate) fn cancel_managed_key_rename(&mut self, cx: &mut Context<Self>) {
        self.editing_managed_key_id = None;
        cx.notify();
    }

    pub(crate) fn confirm_managed_key_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.managed_key_editor_target.take() {
            let selected = self.managed_key_dialog_selection.take();
            self.editing_managed_key_id = None;
            self.active_dialog = None;
            window.close_dialog(cx);
            editor.update(cx, |editor, cx| {
                editor.apply_managed_key_selection(selected, cx);
            });
            cx.notify();
            return;
        }

        self.managed_key_selected = self.managed_key_dialog_selection.clone();
        self.using_custom_key_path = false;
        self.return_to_ssh_dialog(window, cx);
    }

    pub(crate) fn return_to_ssh_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managed_key_editor_target.take().is_some() {
            self.editing_managed_key_id = None;
            self.managed_key_dialog_selection = None;
            self.active_dialog = None;
            window.close_dialog(cx);
            cx.notify();
            return;
        }

        self.editing_managed_key_id = None;
        self.managed_key_dialog_selection = None;
        self.active_dialog = None;
        window.close_dialog(cx);
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| this.show_ssh_dialog(window, cx));
        });
    }

    pub(crate) fn open_key_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_managed_key_id = None;
        self.key_import.open();
        Self::set_input_value(&self.key_import_remark_input, "", window, cx);
        Self::set_input_value(&self.key_import_passphrase_input, "", window, cx);
        self.active_dialog = None;
        window.close_dialog(cx);
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                this.show_managed_key_import_dialog(window, cx);
            });
        });
    }

    pub(crate) fn close_key_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.key_import.close();
        Self::set_input_value(&self.key_import_remark_input, "", window, cx);
        Self::set_input_value(&self.key_import_passphrase_input, "", window, cx);
        self.active_dialog = None;
        window.close_dialog(cx);
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                this.show_managed_key_selector_dialog(window, cx);
            });
        });
    }

    pub(crate) fn pick_managed_key_import_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cancellation = self
            .async_runtime
            .supervisor
            .start("pick-managed-key-import-file");
        let start_dir = directories::BaseDirs::new()
            .map(|d| d.home_dir().join(".ssh"))
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        let file_dialog = rfd::AsyncFileDialog::new()
            .set_directory(start_dir)
            .pick_file();

        cx.spawn_in(window, async move |this, cx| {
            if let Some(file) = file_dialog.await {
                if cancellation.is_cancelled() {
                    return Ok::<(), anyhow::Error>(());
                }
                let path = file.path().to_path_buf();
                let display_path = path.to_string_lossy().to_string();
                let default_name = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("imported-key")
                    .to_string();

                let _ = gpui::AsyncWindowContext::update(cx, |_, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.key_import.begin_file_validation(display_path.clone());
                        cx.notify();
                    });
                });

                let (result_tx, result_rx) = futures::channel::oneshot::channel();
                std::thread::spawn(move || {
                    let _ = result_tx.send(std::fs::read_to_string(&path));
                });
                let read_result = result_rx.await.unwrap_or_else(|_| {
                    Err(std::io::Error::other("private key validation task stopped"))
                });
                if cancellation.is_cancelled() {
                    return Ok::<(), anyhow::Error>(());
                }
                let _ = gpui::AsyncWindowContext::update(cx, |window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        if !this.key_import.open || this.key_import.path != display_path {
                            return;
                        }
                        if this
                            .key_import_remark_input
                            .read(cx)
                            .value()
                            .trim()
                            .is_empty()
                        {
                            Self::set_input_value(
                                &this.key_import_remark_input,
                                default_name,
                                window,
                                cx,
                            );
                        }
                        match read_result {
                            Ok(content) => {
                                let passphrase = this
                                    .key_import_passphrase_input
                                    .read(cx)
                                    .value()
                                    .to_string();
                                this.key_import.set_file(
                                    display_path,
                                    content,
                                    &passphrase,
                                    &this.managed_keys,
                                );
                            }
                            Err(err) => this
                                .key_import
                                .set_read_error(display_path, format!("{err:#}")),
                        }
                        cx.notify();
                    });
                });
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    pub(crate) fn confirm_managed_key_import(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let crate::app::ssh_key_import::KeyImportValidation::Valid {
            key_type,
            fingerprint,
        } = self.key_import.validation.clone()
        else {
            return;
        };

        let fallback_name = std::path::Path::new(&self.key_import.path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("imported-key");
        let remark = self
            .key_import_remark_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let name = if remark.is_empty() {
            fallback_name.to_string()
        } else {
            remark
        };
        let passphrase = self
            .key_import_passphrase_input
            .read(cx)
            .value()
            .to_string();
        let key = ManagedKey {
            id: Uuid::new_v4().to_string(),
            name,
            key_type,
            fingerprint,
            inline_content: self.key_import.content.clone(),
            passphrase,
            created_at: chrono::Local::now().timestamp(),
        };
        let key_id = key.id.clone();
        self.config.upsert_managed_key(key);
        if let Err(err) = crate::app::config_persistence::save_full(&self.config) {
            tracing::warn!("failed to save config: {err:#}");
            self.status = format!("{}: {err:#}", t!("key_import_failed")).into();
            cx.notify();
            return;
        }
        self.managed_keys = self.config.managed_keys().to_vec();
        self.managed_key_dialog_selection = Some(key_id);
        self.status = t!("key_imported").to_string().into();
        self.close_key_import(window, cx);
    }

    /// Open a file picker to import a private key into managed storage.
    /// Reads the file content, validates it, detects type + fingerprint,
    /// and saves a new `ManagedKey`.
    pub(crate) fn import_managed_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cancellation = self.async_runtime.supervisor.start("import-managed-key");
        let start_dir = directories::BaseDirs::new()
            .map(|d| d.home_dir().join(".ssh"))
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        let file_dialog = rfd::AsyncFileDialog::new()
            .set_directory(start_dir)
            .pick_file();

        cx.spawn_in(window, async move |this, cx| {
            if let Some(file) = file_dialog.await {
                if cancellation.is_cancelled() {
                    return Ok::<(), anyhow::Error>(());
                }
                let path = file.path().to_path_buf();
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let file_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("imported-key")
                    .to_string();
                let _ = gpui::AsyncWindowContext::update(cx, |_window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.finalize_key_import(content, file_name, cx);
                    });
                });
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    /// Validate raw key content and save as a new ManagedKey.
    /// Reports errors via `self.status`.
    fn finalize_key_import(
        &mut self,
        content: String,
        default_name: String,
        cx: &mut Context<Self>,
    ) {
        let passphrase = self.passphrase_input.read(cx).value().to_string();
        let pass_opt = (!passphrase.is_empty()).then_some(passphrase.as_str());

        match crate::session::ssh_keys::validate_and_inspect(&content, pass_opt) {
            Ok((key_type, fingerprint)) => {
                // Check for duplicate fingerprint
                if self
                    .config
                    .managed_keys()
                    .iter()
                    .any(|k| k.fingerprint == fingerprint)
                {
                    self.status = t!("key_duplicate_fingerprint").to_string().into();
                    cx.notify();
                    return;
                }
                let key = ManagedKey {
                    id: Uuid::new_v4().to_string(),
                    name: default_name,
                    key_type,
                    fingerprint,
                    inline_content: content,
                    passphrase,
                    created_at: chrono::Local::now().timestamp(),
                };
                self.config.upsert_managed_key(key.clone());
                if let Err(err) = crate::app::config_persistence::save_full(&self.config) {
                    tracing::warn!("failed to save config: {err:#}");
                }
                self.managed_keys = self.config.managed_keys().to_vec();
                self.status = t!("key_imported").to_string().into();
                cx.notify();
            }
            Err(err) => {
                self.status = format!("{}: {err:#}", t!("key_import_failed")).into();
                cx.notify();
            }
        }
    }

    /// Rename a managed key.
    pub(crate) fn rename_managed_key(
        &mut self,
        key_id: String,
        new_name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self
            .config
            .managed_keys()
            .iter()
            .find(|k| k.id == key_id)
            .cloned()
        else {
            return;
        };
        let mut updated = key;
        updated.name = new_name;
        self.config.upsert_managed_key(updated);
        if let Err(err) = crate::app::config_persistence::save_full(&self.config) {
            tracing::warn!("failed to save config: {err:#}");
        }
        self.managed_keys = self.config.managed_keys().to_vec();
        self.editing_managed_key_id = None;
        cx.notify();
    }

    /// Delete a managed key by id. Also clears the reference from any
    /// session that used it.
    pub(crate) fn delete_managed_key(&mut self, key_id: String, cx: &mut Context<Self>) {
        self.config.remove_managed_key(&key_id);
        if let Err(err) = crate::app::config_persistence::save_full(&self.config) {
            tracing::warn!("failed to save config: {err:#}");
        }
        self.managed_keys = self.config.managed_keys().to_vec();
        if self.managed_key_selected.as_deref() == Some(&key_id) {
            self.managed_key_selected = None;
        }
        if self.managed_key_dialog_selection.as_deref() == Some(&key_id) {
            self.managed_key_dialog_selection = None;
        }
        cx.notify();
    }

    pub(crate) fn delete_selected_managed_key(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(key_id) = self.managed_key_dialog_selection.clone() else {
            return;
        };
        self.request_managed_key_deletion(key_id, window, cx);
    }

    /// Switch the connection form back to managed-key mode.
    pub(crate) fn use_managed_key(&mut self, cx: &mut Context<Self>) {
        self.using_custom_key_path = false;
        cx.notify();
    }

    /// Switch the connection form to use a custom key path (file picker).
    pub(crate) fn use_custom_key_path(&mut self, cx: &mut Context<Self>) {
        self.managed_key_selected = None;
        self.using_custom_key_path = true;
        cx.notify();
    }

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

    pub(crate) fn terminal_cell_width(&self) -> f32 {
        terminal_cell_width_for(self.terminal_font_size, self.terminal_display_style)
    }

    pub(crate) fn terminal_line_height(&self) -> f32 {
        terminal_line_height_for(self.terminal_font_size, self.terminal_display_style)
    }

    pub(crate) fn change_terminal_display_style(
        &mut self,
        style: TerminalDisplayStyle,
        cx: &mut Context<Self>,
    ) {
        self.terminal_display_style = style;
        self.config.set_terminal_display_style(style);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(crate) fn change_terminal_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.terminal_font_size = (self.terminal_font_size + delta).clamp(10.0, 24.0);
        self.config.set_terminal_font_size(self.terminal_font_size);
        self.mark_config_preferences_dirty();
        self.status = format!("terminal font size: {:.0}px", self.terminal_font_size).into();
        cx.notify();
    }

    pub(crate) fn change_ui_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.ui_font_size = (self.ui_font_size + delta).clamp(8.0, 24.0);
        self.config.set_ui_font_size(self.ui_font_size);
        self.mark_config_preferences_dirty();
        Theme::global_mut(cx).font_size = px(self.ui_font_size);
        self.status = format!("UI font size: {:.0}px", self.ui_font_size).into();
        cx.notify();
    }

    pub(crate) fn change_ui_font_family(
        &mut self,
        family: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ui_font_family = family.into();
        self.config.set_ui_font_family(family);
        self.mark_config_preferences_dirty();
        crate::app::theme::set_theme_font_names(Theme::global_mut(cx), &self.ui_font_family);
        cx.notify();
        window.refresh();
    }

    pub(crate) fn change_terminal_font_family(&mut self, family: &str, cx: &mut Context<Self>) {
        self.terminal_font_family = family.into();
        self.config.set_terminal_font_family(family);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(crate) fn change_cursor_style(
        &mut self,
        style: crate::session::config::CursorStyle,
        cx: &mut Context<Self>,
    ) {
        self.cursor_style = style;
        self.config.set_cursor_style(style);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(crate) fn reset_layout(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.config.set_layout_state(None, None, None);
        if let Err(error) = crate::app::config_persistence::save_full(&self.config) {
            tracing::warn!("failed to persist reset layout: {error:#}");
        }

        self.is_layout_reset = true;
        self.workspace_panels = cx.new(|_| crate::app::resizable::ResizableState::default());
        self.body_panels = cx.new(|_| crate::app::resizable::ResizableState::default());

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
                &self.session_name_input,
                entry.host_alias.clone(),
                window,
                cx,
            );
            Self::set_input_value(&self.host_input, entry.hostname.clone(), window, cx);
            Self::set_input_value(&self.port_input, entry.port.to_string(), window, cx);
            // If no user specified in config, use current system user
            let user = if entry.user.is_empty() {
                std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "root".to_string())
            } else {
                entry.user.clone()
            };
            Self::set_input_value(&self.user_input, user, window, cx);
            Self::set_input_value(
                &self.key_path_input,
                entry.identity_files.first().cloned().unwrap_or_default(),
                window,
                cx,
            );
            Self::set_input_value(&self.password_input, String::new(), window, cx);
            Self::set_input_value(&self.key_inline_input, String::new(), window, cx);
            Self::set_input_value(&self.passphrase_input, String::new(), window, cx);
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
        self.active_system_info_tab = None;
        self.home_page_open = false;
        let ordinal = self.next_tab_group_ordinal;
        self.next_tab_group_ordinal += 1;
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
            if let Err(err) = crate::app::config_persistence::save_full(&self.config) {
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
            id.clone(),
            session.clone(),
            proxy_config.clone(),
            DEFAULT_COLS,
            DEFAULT_ROWS,
            events.clone(),
        );
        self.tabs.push(TerminalTab::new_ssh(
            id.clone(),
            &session,
            backend,
            events.clone(),
        ));
        if let Some(tab) = self.tabs.last_mut() {
            tab.feed_status_line(&rust_i18n::t!("starting_connection"));
        }
        self.active_tab = Some(id.clone());
        self.pane_root = PaneLayout::Single(id.clone());
        self.focused_pane_path = vec![];
        let group_id = Uuid::new_v4().to_string();
        self.tab_groups.push(TabGroup {
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
                preview: None,
                selected_entries: std::collections::HashSet::new(),
                home_dir: String::new(),
                follow_terminal_cwd: false,
                initial_terminal_cwd_synced: false,
                latency_ms: None,
            }),
        });
        self.active_group = Some(group_id.clone());
        self.tabs_scroll_handle.scroll_to_item(self.tabs.len() - 1);
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
        let sftp_handle = crate::sftp::spawn_sftp(
            self.runtime.handle(),
            group_id.clone(),
            session,
            proxy_config,
            events,
        );
        self.sftp_handles.insert(group_id.clone(), sftp_handle);
        self.active_tab = Some(id.clone());
        self.pending_sftp_path_sync = Some("/".into());
        self.status = t!("ssh_tab_opened").into();
        cx.notify();
    }

    pub(crate) fn remove_saved_session(&mut self, session_id: String, cx: &mut Context<Self>) {
        self.config.remove(&session_id);
        if let Err(err) = crate::app::config_persistence::save_full(&self.config) {
            tracing::warn!("failed to save config: {err:#}");
        }
        self.status = t!("session_removed").into();
        cx.notify();
    }

    /// Retry a single disconnected tab by its ID.
    /// For SSH tabs: spawns a new SSH connection and restarts SFTP.
    /// For local tabs: spawns a new local shell.
    ///
    /// The existing `TerminalTab` (including its `term` scrollback history)
    /// is preserved — only the backend is swapped via `set_backend()`.
    pub(crate) fn retry_disconnected_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        if self.tabs[ix].connected || self.tabs[ix].disconnected_reason.is_none() {
            return;
        }
        self.terminal_completions.remove(tab_id);

        let is_ssh = self.tabs[ix].session.is_some();
        let session = self.tabs[ix].session.clone();
        let new_generation = self.tabs[ix].backend_generation + 1;
        let cols = self.tabs[ix].cols;
        let rows = self.tabs[ix].rows;
        let events = self.backend_events_sender(cx);
        let proxy_config = self.config.clone();
        self.register_backend_route(tab_id.to_string(), cx);

        // Close old backend (sends Close through the shared Arc<Mutex>)
        self.tabs[ix].send_backend(BackendCommand::Close);

        if let Some(session) = session {
            // SSH tab: spawn new SSH connection
            let backend = ssh::spawn_ssh_terminal(
                self.runtime.handle(),
                tab_id.to_string(),
                session.clone(),
                proxy_config.clone(),
                cols,
                rows,
                events.clone(),
            );

            // Swap the backend — the Term's internal listener shares the
            // same Arc<Mutex<BackendTx>>, so user input is automatically
            // routed to the new backend. Terminal history is preserved.
            self.tabs[ix].set_backend(backend);
            self.tabs[ix].connected = false;
            self.tabs[ix].status = "connecting".into();
            self.tabs[ix].disconnected_reason = None;
            self.tabs[ix].backend_generation = new_generation;
            self.tabs[ix].backend_initialized = false;
            self.tabs[ix].feed_status_line(&rust_i18n::t!("starting_connection"));

            // Restart SFTP for the group containing this tab
            if let Some(group) = self
                .tab_groups
                .iter()
                .find(|g| g.pane_root.contains(tab_id))
            {
                let group_id = group.id.clone();
                let group_session = self
                    .tabs
                    .iter()
                    .find(|t| group.pane_root.contains(&t.id) && t.session.is_some())
                    .and_then(|t| t.session.clone());

                if let Some(session) = group_session {
                    if let Some(old_handle) = self.sftp_handles.remove(&group_id) {
                        old_handle.close();
                    }
                    self.register_backend_route(group_id.clone(), cx);
                    let sftp_handle = crate::sftp::spawn_sftp(
                        self.runtime.handle(),
                        group_id.clone(),
                        session,
                        proxy_config.clone(),
                        events.clone(),
                    );
                    self.sftp_handles.insert(group_id.clone(), sftp_handle);

                    if let Some(group) = self.tab_groups.iter_mut().find(|g| g.id == group_id) {
                        if let Some(sftp) = group.sftp.as_mut() {
                            sftp.status = rust_i18n::t!("sftp_connecting").to_string();
                        }
                    }
                }
            }
        } else {
            // Local tab: spawn new local shell
            match local::spawn_local_terminal(tab_id.to_string(), cols, rows, events) {
                Ok(backend) => {
                    // Swap the backend — preserves terminal history.
                    self.tabs[ix].set_backend(backend);
                    self.tabs[ix].connected = true;
                    self.tabs[ix].status = "local shell".into();
                    self.tabs[ix].disconnected_reason = None;
                    self.tabs[ix].backend_generation = new_generation;
                    self.tabs[ix].backend_initialized = false;
                    // Resize the new PTY to match the pane dimensions.
                    self.tabs[ix].send_backend(BackendCommand::Resize { cols, rows });
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
