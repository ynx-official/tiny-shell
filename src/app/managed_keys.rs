use gpui::{Context, Entity, SharedString, Window};
use gpui_component::{WindowExt as _, input::InputState};
use rust_i18n::t;
use uuid::Uuid;

use crate::{
    TinyShell,
    app::{config_persistence, input_focus, ssh_key_import::KeyImportValidation},
    session::config::ManagedKey,
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
        input_focus::defer_focus_input_at_end(self.key_import_remark_input.clone(), window, cx);
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
        let KeyImportValidation::Valid {
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
        if let Err(err) = config_persistence::save_full(&self.config) {
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
    pub(crate) fn finalize_key_import(
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
                if let Err(err) = config_persistence::save_full(&self.config) {
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
        if let Err(err) = config_persistence::save_full(&self.config) {
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
        if let Err(err) = config_persistence::save_full(&self.config) {
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
}
