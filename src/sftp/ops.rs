use gpui::{
    Bounds, Context, ParentElement as _, PathPromptOptions, Pixels, Point, Styled as _, Window,
    div, px,
};
use gpui_component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::Dialog,
    h_flex, v_flex,
};
use rust_i18n::t;

#[derive(Clone)]
enum SftpDownloadRequest {
    Entries {
        handle: SftpHandle,
        remote_paths: Vec<String>,
    },
    Archive {
        handle: SftpHandle,
        remote_paths: Vec<String>,
        suggested_name: String,
    },
}

use crate::{
    SftpContextMenuState, TinyShell,
    sftp::{RemoteEntry, SftpHandle},
    terminal,
};

pub(crate) fn minimal_sftp_tree_scroll_offset_y(
    current_offset_y: Pixels,
    viewport: Bounds<Pixels>,
    target: Bounds<Pixels>,
    bottom_inset: Pixels,
) -> Pixels {
    if target.bottom() <= viewport.top() {
        current_offset_y + viewport.top() - target.top()
    } else if target.top() >= viewport.bottom() {
        let visible_bottom = (viewport.bottom() - bottom_inset).max(viewport.top());
        current_offset_y + visible_bottom - target.bottom()
    } else {
        current_offset_y
    }
}

pub(crate) fn centered_sftp_tree_scroll_offset_y(
    current_offset_y: Pixels,
    viewport: Bounds<Pixels>,
    target: Bounds<Pixels>,
) -> Pixels {
    current_offset_y + viewport.center().y - target.center().y
}

pub(crate) fn is_editable_text_file(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    let ext = std::path::Path::new(&lower)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let known_exts = [
        "txt",
        "conf",
        "json",
        "yaml",
        "yml",
        "xml",
        "ini",
        "sh",
        "bash",
        "zsh",
        "py",
        "rs",
        "js",
        "mjs",
        "cjs",
        "ts",
        "html",
        "htm",
        "css",
        "scss",
        "md",
        "toml",
        "csv",
        "log",
        "cfg",
        "properties",
        "service",
        "env",
        "sql",
        "lua",
        "rb",
        "php",
        "go",
        "java",
        "kt",
        "swift",
        "c",
        "h",
        "cpp",
        "cc",
        "cxx",
        "hpp",
        "gradle",
    ];
    if known_exts.contains(&ext) {
        return true;
    }
    let known_names = ["dockerfile", "makefile", ".gitignore", ".env"];
    if known_names.contains(&lower.as_str()) {
        return true;
    }
    false
}

impl TinyShell {
    pub(crate) fn active_sftp(&self) -> Option<&terminal::SftpUiState> {
        self.workspace()
            .active_group_id()
            .and_then(|id| self.workspace().tab_groups().iter().find(|g| g.id == id))
            .and_then(|g| g.sftp.as_ref())
    }

    pub(crate) fn active_sftp_mut(&mut self) -> Option<&mut terminal::SftpUiState> {
        let active_id = self.workspace().active_group_id().map(str::to_owned)?;
        self.window_state_mut()
            .workspace_state_mut()
            .tab_groups_mut()
            .iter_mut()
            .find(|g| g.id == active_id)
            .and_then(|g| g.sftp.as_mut())
    }

    pub(crate) fn active_sftp_handle(&self) -> Option<&SftpHandle> {
        self.workspace()
            .active_group_id()
            .and_then(|id| self.sftp_handles.get(id))
    }

    pub(crate) fn reset_sftp_tree_for_active_group(&mut self) {
        let current_path = self.active_sftp().map(|sftp| sftp.current_path.clone());
        let tree_offset = self.sftp_workspace.tree_scroll_handle.offset();
        self.sftp_workspace.tree_scroll_handle.set_offset(Point {
            x: px(0.),
            y: tree_offset.y,
        });
        self.sftp_workspace.tree_scroll_target_bounds = None;
        self.sftp_workspace.pending_tree_scroll_path = current_path;
        self.sftp_workspace.center_pending_tree_scroll = false;
    }

    /// 双击文本文件时调用:下载文件内容到内存,打开独立编辑器窗口。
    /// 若同一会话的编辑器已打开该文件,则直接激活窗口并切换 tab。
    pub(crate) fn open_file_in_editor(&mut self, remote_path: String, cx: &mut Context<Self>) {
        let Some(session_id) = self.workspace().active_group_id().map(str::to_owned) else {
            return;
        };
        if crate::app::sftp_editor_window::focus_path(
            &session_id,
            self.session_owner_id,
            &remote_path,
            cx,
        ) {
            return;
        }
        if let Some(handle) = self.sftp_handles.get(&session_id) {
            tracing::info!("[sftp] opening in-memory editor: '{}'", remote_path);
            handle.download_file_content(remote_path);
            cx.notify();
        }
    }

    pub(crate) fn navigate_sftp(&mut self, path: String, cx: &mut Context<Self>) {
        if let Some(group_id) = self.workspace().active_group_id().map(str::to_owned)
            && self.navigate_sftp_group(&group_id, path)
        {
            cx.notify();
        }
    }

    pub(crate) fn navigate_sftp_group(&mut self, group_id: &str, path: String) -> bool {
        let Some(handle) = self.sftp_handles.get(group_id).cloned() else {
            return false;
        };
        let Some(sftp) = self
            .window_state_mut()
            .workspace_state_mut()
            .tab_groups_mut()
            .iter_mut()
            .find(|group| group.id == group_id)
            .and_then(|group| group.sftp.as_mut())
        else {
            return false;
        };
        let path = Self::normalize_sftp_path(&path, &sftp.home_dir);

        tracing::info!("[sftp] navigating to directory: '{}'", path);
        let mut ancestors = Self::sftp_path_chain(&path);
        ancestors.pop();
        let missing_ancestors = ancestors
            .into_iter()
            .filter(|directory| !sftp.directory_entries.contains_key(directory))
            .collect::<Vec<_>>();

        Self::expand_sftp_tree_to_path(sftp, &path);
        sftp.current_path = path.clone();
        sftp.entries.clear();
        sftp.selected_path = None;
        sftp.selected_entries.clear();
        if self.workspace().active_group_id() == Some(group_id) {
            let tree_offset = self.sftp_workspace.tree_scroll_handle.offset();
            self.sftp_workspace.tree_scroll_handle.set_offset(Point {
                x: px(0.),
                y: tree_offset.y,
            });
            self.sftp_workspace.tree_scroll_target_bounds = None;
            self.sftp_workspace.pending_path_sync = Some(path.clone());
            self.sftp_workspace.pending_tree_scroll_path = Some(path.clone());
            self.sftp_workspace.center_pending_tree_scroll = false;
        }

        for directory in missing_ancestors {
            handle.list_directory_tree(directory);
        }
        handle.list_dir(path);
        true
    }

    pub(crate) fn sync_sftp_tree_scroll(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.sftp_workspace.pending_tree_scroll_path.clone() else {
            return;
        };
        let Some(sftp) = self.active_sftp() else {
            return;
        };
        if !sftp_tree_paths(sftp, self.sftp_panel.show_hidden_files)
            .iter()
            .any(|row| row == &path)
        {
            return;
        }
        let Some((target_path, target_bounds)) =
            self.sftp_workspace.tree_scroll_target_bounds.clone()
        else {
            return;
        };
        if target_path != path {
            self.sftp_workspace.tree_scroll_target_bounds = None;
            return;
        }

        let scroll_handle = &self.sftp_workspace.tree_scroll_handle;
        let viewport = scroll_handle.bounds();
        if viewport.size.height <= px(0.) {
            return;
        }
        let max_offset = scroll_handle.max_offset();
        let bottom_inset = if max_offset.x > px(0.) {
            px(16.)
        } else {
            px(0.)
        };
        let current_offset = scroll_handle.offset();
        let next_offset_y = if self.sftp_workspace.center_pending_tree_scroll {
            centered_sftp_tree_scroll_offset_y(current_offset.y, viewport, target_bounds)
        } else {
            minimal_sftp_tree_scroll_offset_y(
                current_offset.y,
                viewport,
                target_bounds,
                bottom_inset,
            )
        }
        .clamp(-max_offset.y, px(0.));

        scroll_handle.set_offset(Point {
            x: px(0.),
            y: next_offset_y,
        });
        self.sftp_workspace.pending_tree_scroll_path = None;
        self.sftp_workspace.center_pending_tree_scroll = false;
        self.sftp_workspace.tree_scroll_target_bounds = None;
        cx.notify();
    }

    pub(crate) fn locate_current_sftp_tree_directory(&mut self, cx: &mut Context<Self>) {
        let Some(current_path) = self.active_sftp().map(|sftp| sftp.current_path.clone()) else {
            return;
        };
        let current_offset = self.sftp_workspace.tree_scroll_handle.offset();
        self.sftp_workspace.tree_scroll_handle.set_offset(Point {
            x: px(0.),
            y: current_offset.y,
        });
        self.sftp_workspace.tree_scroll_target_bounds = None;
        self.sftp_workspace.pending_tree_scroll_path = Some(current_path);
        self.sftp_workspace.center_pending_tree_scroll = true;
        cx.notify();
    }

    pub(crate) fn toggle_sftp_tree_directory(&mut self, path: String, cx: &mut Context<Self>) {
        let should_load = if let Some(sftp) = self.active_sftp_mut() {
            if path == "/" {
                sftp.expanded_directories.insert(path.clone());
                !sftp.directory_entries.contains_key(&path)
            } else if sftp.expanded_directories.remove(&path) {
                false
            } else {
                sftp.expanded_directories.insert(path.clone());
                !sftp.directory_entries.contains_key(&path)
            }
        } else {
            false
        };
        if should_load {
            if let Some(handle) = self.active_sftp_handle() {
                handle.list_directory_tree(path);
            }
        }
        cx.notify();
    }

    pub(crate) fn select_sftp_tree_directory(&mut self, path: String, cx: &mut Context<Self>) {
        self.navigate_sftp(path, cx);
    }

    pub(crate) fn expand_sftp_tree_to_path(sftp: &mut terminal::SftpUiState, path: &str) {
        for directory in Self::sftp_path_chain(path) {
            sftp.expanded_directories.insert(directory);
        }
    }

    fn sftp_path_chain(path: &str) -> Vec<String> {
        let mut chain = vec!["/".to_string()];
        let mut current = String::new();
        for component in path.split('/').filter(|component| !component.is_empty()) {
            current.push('/');
            current.push_str(component);
            chain.push(current.clone());
        }
        chain
    }

    fn normalize_sftp_path(path: &str, home_dir: &str) -> String {
        let resolved = if path == "~" {
            home_dir.to_string()
        } else if let Some(rest) = path.strip_prefix("~/") {
            crate::sftp::join_remote(home_dir, rest)
        } else if !path.starts_with('/') {
            format!("/{path}")
        } else {
            path.to_string()
        };
        if resolved == "/" {
            resolved
        } else {
            resolved.trim_end_matches('/').to_string()
        }
    }

    pub(crate) fn select_sftp_entry(&mut self, entry: RemoteEntry, cx: &mut Context<Self>) {
        self.mark_sftp_entry_selected(&entry.full_path, cx);
        if let Some(sftp) = self.active_sftp_mut() {
            if !sftp.selected_entries.remove(&entry.full_path) {
                sftp.selected_entries.insert(entry.full_path);
            }
        }
    }

    pub(crate) fn mark_sftp_entry_selected(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(sftp) = self.active_sftp_mut() {
            sftp.selected_path = Some(path.to_string());
        }
        cx.notify();
    }

    pub(crate) fn sftp_parent_path(path: &str) -> String {
        if path == "/" {
            return "/".to_string();
        }
        path.trim_end_matches('/')
            .rsplit_once('/')
            .map(|(parent, _)| {
                if parent.is_empty() {
                    "/".to_string()
                } else {
                    parent.to_string()
                }
            })
            .unwrap_or_else(|| "/".to_string())
    }

    pub(crate) fn refresh_sftp(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.active_sftp().map(|sftp| sftp.current_path.clone()) {
            self.navigate_sftp(path, cx);
        }
    }

    pub(crate) fn sync_sftp_path_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.sftp_workspace.pending_path_sync.take() else {
            return;
        };
        self.sftp_workspace.path_input.update(cx, |state, cx| {
            state.set_value(path, window, cx);
        });
    }

    pub(crate) fn open_sftp_context_menu(
        &mut self,
        remote_path: Option<String>,
        is_dir: bool,
        permissions: Option<u32>,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu_epoch = self.context_menu_epoch.wrapping_add(1);
        self.sftp_workspace.context_menu = Some(SftpContextMenuState {
            remote_path,
            is_dir,
            permissions,
            position,
        });
        cx.notify();
    }

    pub(crate) fn dismiss_sftp_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.sftp_workspace.context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn trigger_sftp_context_download(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.sftp_workspace.context_menu.take() else {
            return;
        };
        let Some(remote_path) = menu.remote_path else {
            return;
        };
        self.download_sftp_entry(remote_path, window, cx);
        cx.notify();
    }

    pub(crate) fn trigger_sftp_context_open(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.sftp_workspace.context_menu.take() else {
            return;
        };
        let Some(remote_path) = menu.remote_path else {
            return;
        };
        if menu.is_dir {
            self.navigate_sftp(remote_path, cx);
        } else if is_editable_text_file(&remote_path) {
            self.open_file_in_editor(remote_path, cx);
        } else if let Some(handle) = self.active_sftp_handle() {
            handle.edit_file(remote_path);
        }
        cx.notify();
    }

    pub(crate) fn trigger_sftp_context_internal_editor(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.sftp_workspace.context_menu.take() else {
            return;
        };
        if let Some(remote_path) = menu.remote_path {
            self.open_file_in_editor(remote_path, cx);
        }
    }

    pub(crate) fn trigger_sftp_context_system_open(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.sftp_workspace.context_menu.take() else {
            return;
        };
        if let Some(remote_path) = menu.remote_path
            && let Some(handle) = self.active_sftp_handle()
        {
            handle.edit_file(remote_path);
        }
        cx.notify();
    }

    pub(crate) fn trigger_sftp_context_external_editor(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.sftp_workspace.context_menu.take() else {
            return;
        };
        let editor = self.config.sftp_external_editor().to_string();
        if editor.is_empty() {
            self.status = t!("sftp_external_editor_not_set").into();
            cx.notify();
            return;
        }
        if let Some(remote_path) = menu.remote_path
            && let Some(handle) = self.active_sftp_handle()
        {
            handle.edit_file_with(remote_path, editor);
        }
        cx.notify();
    }

    pub(crate) fn choose_sftp_external_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sftp_workspace.context_menu = None;
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("sftp_select_external_editor").to_string().into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = prompt.await
                && let Some(path) = paths.pop()
            {
                this.update(cx, |this, cx| {
                    this.config
                        .set_sftp_external_editor(path.to_string_lossy().to_string());
                    if let Err(err) = crate::app::config_persistence::save_full(
                        &this.config_repository,
                        &this.config,
                    ) {
                        this.status = t!(
                            "sftp_external_editor_save_failed",
                            error = format!("{err:#}")
                        )
                        .into();
                    } else {
                        this.status = t!("sftp_external_editor_saved").into();
                    }
                    cx.notify();
                })?;
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    pub(crate) fn trigger_sftp_context_copy_path(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.sftp_workspace.context_menu.take() else {
            return;
        };
        if let Some(remote_path) = menu.remote_path {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(remote_path));
        }
        cx.notify();
    }

    pub(crate) fn trigger_sftp_context_upload(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remote_dir = self
            .sftp_workspace
            .context_menu
            .take()
            .and_then(|menu| menu.is_dir.then_some(menu.remote_path).flatten())
            .or_else(|| self.active_sftp().map(|sftp| sftp.current_path.clone()))
            .unwrap_or_else(|| "/".into());
        self.upload_sftp_files_to(remote_dir, window, cx);
    }

    fn request_sftp_download_destination(
        &mut self,
        request: SftpDownloadRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(directory) = self.config.download_directory() {
            self.start_sftp_download(request, directory, cx);
            self.show_transfers_dialog(window, cx);
            return;
        }

        self.show_sftp_download_directory_prompt(request, window, cx);
    }

    fn show_sftp_download_directory_prompt(
        &mut self,
        request: SftpDownloadRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remember = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let view = cx.entity();
        self.open_dialog(
            crate::app::DialogKind::Transfers,
            window,
            cx,
            move |dialog: Dialog, token, _window, _| {
                let on_close_view = view.clone();
                dialog
                    .title(t!("confirm_download_directory_title").to_string())
                    .w(px(500.))
                    .on_close(move |_, _, cx| {
                        on_close_view.update(cx, |this, cx| {
                            this.dialog_closed(token);
                            cx.notify();
                        });
                    })
                    .content({
                        let remember = remember.clone();
                        move |content, _, cx| {
                            content.child(
                                v_flex()
                                    .w_full()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                t!("confirm_download_directory_desc").to_string(),
                                            ),
                                    )
                                    .child(
                                        Checkbox::new("remember-download-directory")
                                            .label(t!("remember_download_directory").to_string())
                                            .checked(
                                                remember.load(std::sync::atomic::Ordering::Relaxed),
                                            )
                                            .on_click({
                                                let remember = remember.clone();
                                                move |checked, window, _| {
                                                    remember.store(
                                                        *checked,
                                                        std::sync::atomic::Ordering::Relaxed,
                                                    );
                                                    window.refresh();
                                                }
                                            }),
                                    ),
                            )
                        }
                    })
                    .footer(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("cancel-download-directory")
                                    .ghost()
                                    .label(t!("cancel").to_string())
                                    .on_click(_window.listener_for(
                                        &view,
                                        move |this, _, window, cx| {
                                            this.dismiss_dialog(token, window, cx);
                                        },
                                    )),
                            )
                            .child(
                                Button::new("choose-download-directory")
                                    .primary()
                                    .label(t!("choose_directory").to_string())
                                    .on_click({
                                        let remember = remember.clone();
                                        let view = view.clone();
                                        let request = request.clone();
                                        move |_, window, cx| {
                                            let remember =
                                                remember.load(std::sync::atomic::Ordering::Relaxed);
                                            view.update(cx, |this, cx| {
                                                this.dismiss_dialog(token, window, cx);
                                            });
                                            view.update(cx, |this, cx| {
                                                this.pick_sftp_download_destination(
                                                    request.clone(),
                                                    remember,
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }
                                    }),
                            ),
                    )
            },
        );
    }

    fn pick_sftp_download_destination(
        &mut self,
        request: SftpDownloadRequest,
        remember: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("select_download_directory").to_string().into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            match prompt.await {
                Ok(Ok(Some(mut paths))) => {
                    if let Some(directory) = paths.pop() {
                        this.update(cx, |this, cx| {
                            if remember {
                                this.config.set_download_directory(Some(&directory));
                                this.mark_config_preferences_dirty();
                            }
                            this.start_sftp_download(request, directory, cx);
                        })?;
                    }
                }
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.status = t!(
                            "download_directory_picker_failed",
                            error = error.to_string()
                        )
                        .to_string()
                        .into();
                        cx.notify();
                    })?;
                }
                _ => {}
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    fn start_sftp_download(
        &mut self,
        request: SftpDownloadRequest,
        directory: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        match request {
            SftpDownloadRequest::Entries {
                handle,
                remote_paths,
            } => {
                let local_dir = directory.to_string_lossy().to_string();
                if remote_paths.len() == 1 {
                    handle.download(remote_paths[0].clone(), local_dir);
                } else {
                    for remote in remote_paths {
                        handle.send_command(crate::sftp::SftpCommand::Download {
                            remote,
                            local_dir: local_dir.clone(),
                        });
                    }
                }
                if let Some(sftp) = self.active_sftp_mut() {
                    sftp.selected_entries.clear();
                }
            }
            SftpDownloadRequest::Archive {
                handle,
                remote_paths,
                suggested_name,
            } => {
                let local_zip = directory.join(suggested_name);
                handle.send_command(crate::sftp::SftpCommand::PackDownload {
                    remote_paths,
                    local_zip: local_zip.to_string_lossy().to_string(),
                });
            }
        }
        cx.notify();
    }

    pub(crate) fn trigger_sftp_context_pack_download(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.sftp_workspace.context_menu.take() else {
            return;
        };
        let remote_paths = self.sftp_context_paths(&menu);
        if remote_paths.is_empty() {
            return;
        }
        let suggested_name = if remote_paths.len() == 1 {
            format!(
                "{}.zip",
                remote_paths[0]
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("archive")
            )
        } else {
            "selection.zip".to_string()
        };
        let Some(handle) = self.active_sftp_handle().cloned() else {
            return;
        };
        self.request_sftp_download_destination(
            SftpDownloadRequest::Archive {
                handle,
                remote_paths,
                suggested_name,
            },
            window,
            cx,
        );
    }

    pub(crate) fn trigger_sftp_context_new_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sftp_workspace.context_menu = None;
        self.show_sftp_create_dialog(false, window, cx);
    }

    pub(crate) fn trigger_sftp_context_new_folder(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sftp_workspace.context_menu = None;
        self.show_sftp_create_dialog(true, window, cx);
    }

    pub(crate) fn trigger_sftp_context_rename(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.sftp_workspace.context_menu.take() else {
            return;
        };
        if let Some(remote_path) = menu.remote_path {
            self.show_sftp_rename_dialog(remote_path, window, cx);
        }
    }

    pub(crate) fn trigger_sftp_context_delete(
        &mut self,
        quick: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.sftp_workspace.context_menu.take() else {
            return;
        };
        let paths = self.sftp_context_paths(&menu);
        if !paths.is_empty() {
            self.show_sftp_delete_paths_confirm_dialog(paths, quick, window, cx);
        }
    }

    pub(crate) fn trigger_sftp_context_permissions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.sftp_workspace.context_menu.take() else {
            return;
        };
        if let Some(remote_path) = menu.remote_path {
            self.show_sftp_permissions_dialog(
                remote_path,
                menu.is_dir,
                menu.permissions,
                window,
                cx,
            );
        }
    }

    fn sftp_context_paths(&self, menu: &SftpContextMenuState) -> Vec<String> {
        let Some(remote_path) = menu.remote_path.as_ref() else {
            return Vec::new();
        };
        if let Some(sftp) = self.active_sftp()
            && sftp.selected_entries.contains(remote_path)
        {
            return sftp.selected_entries.iter().cloned().collect();
        }
        vec![remote_path.clone()]
    }

    pub(crate) fn download_sftp_entry(
        &mut self,
        remote_path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self.active_sftp_handle().cloned() else {
            return;
        };
        self.request_sftp_download_destination(
            SftpDownloadRequest::Entries {
                handle,
                remote_paths: vec![remote_path],
            },
            window,
            cx,
        );
    }

    pub(crate) fn upload_sftp_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let remote_dir = self
            .active_sftp()
            .map(|sftp| sftp.current_path.clone())
            .unwrap_or_else(|| "/".into());
        self.upload_sftp_files_to(remote_dir, window, cx);
    }

    pub(crate) fn upload_sftp_files_to(
        &mut self,
        remote_dir: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self.active_sftp_handle().cloned() else {
            return;
        };
        let path_prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("sftp_select_file_to_upload").into()),
        });
        let window_handle = window.window_handle();
        cx.spawn_in(window, async move |this, cx| {
            match path_prompt.await {
                Ok(Ok(Some(mut paths))) => {
                    if let Some(file) = paths.pop() {
                        let local_path = file.to_string_lossy().to_string();
                        tracing::info!(
                            "[sftp] initiating upload of file '{}' to '{}'",
                            local_path,
                            remote_dir
                        );
                        handle.upload_paths(vec![local_path], remote_dir);
                        let _ = window_handle.update(cx, |_, window, cx| {
                            this.update(cx, |this, cx| {
                                this.show_transfers_dialog(window, cx);
                                cx.notify();
                            })
                        });
                    }
                }
                Ok(Err(err)) => {
                    this.update(cx, |this, cx| {
                        this.status =
                            t!("sftp_upload_picker_failed", error = err.to_string()).into();
                        cx.notify();
                    })?;
                }
                _ => {}
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    pub(crate) fn upload_sftp_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(handle) = self.active_sftp_handle().cloned() else {
            return;
        };
        let remote_dir = self
            .active_sftp()
            .map(|sftp| sftp.current_path.clone())
            .unwrap_or_else(|| "/".into());
        let path_prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("sftp_select_folder_to_upload").into()),
        });
        let window_handle = window.window_handle();
        cx.spawn_in(window, async move |this, cx| {
            match path_prompt.await {
                Ok(Ok(Some(mut paths))) => {
                    if let Some(folder) = paths.pop() {
                        let local_path = folder.to_string_lossy().to_string();
                        tracing::info!(
                            "[sftp] initiating upload of folder '{}' to '{}'",
                            local_path,
                            remote_dir
                        );
                        handle.upload_paths(vec![local_path], remote_dir);
                        let _ = window_handle.update(cx, |_, window, cx| {
                            this.update(cx, |this, cx| {
                                this.show_transfers_dialog(window, cx);
                                cx.notify();
                            })
                        });
                    }
                }
                Ok(Err(err)) => {
                    this.update(cx, |this, cx| {
                        this.status =
                            t!("sftp_upload_picker_failed", error = err.to_string()).into();
                        cx.notify();
                    })?;
                }
                _ => {}
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    pub(crate) fn toggle_sftp_entry(
        &mut self,
        path: String,
        checked: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(sftp) = self.active_sftp_mut() {
            if checked {
                sftp.selected_entries.insert(path);
            } else {
                sftp.selected_entries.remove(&path);
            }
            cx.notify();
        }
    }

    pub(crate) fn toggle_all_sftp_entries(&mut self, checked: bool, cx: &mut Context<Self>) {
        if let Some(sftp) = self.active_sftp_mut() {
            if checked {
                let paths: Vec<String> = sftp.entries.iter().map(|e| e.full_path.clone()).collect();
                for path in paths {
                    sftp.selected_entries.insert(path);
                }
            } else {
                sftp.selected_entries.clear();
            }
            cx.notify();
        }
    }

    pub(crate) fn download_selected_sftp_entries(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(sftp) = self.active_sftp() else {
            return;
        };
        let selected: Vec<String> = sftp.selected_entries.iter().cloned().collect();
        if selected.is_empty() {
            return;
        }

        let Some(handle) = self.active_sftp_handle().cloned() else {
            return;
        };

        self.request_sftp_download_destination(
            SftpDownloadRequest::Entries {
                handle,
                remote_paths: selected,
            },
            window,
            cx,
        );
    }

    pub(crate) fn upload_sftp_files_batch(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        if let Some(sftp) = self.active_sftp() {
            if let Some(handle) = self.active_sftp_handle() {
                tracing::info!(
                    "[sftp] initiating batch upload of {} files to '{}'",
                    paths.len(),
                    sftp.current_path
                );
                handle.send_command(crate::sftp::SftpCommand::UploadPaths {
                    locals: paths,
                    remote_dir: sftp.current_path.clone(),
                });
                self.show_transfers_dialog(window, cx);
                cx.notify();
            }
        }
    }
}

pub(crate) fn sftp_tree_paths(
    sftp: &terminal::SftpUiState,
    show_hidden_files: bool,
) -> Vec<String> {
    fn append_rows(
        rows: &mut Vec<String>,
        visited: &mut std::collections::HashSet<String>,
        sftp: &terminal::SftpUiState,
        show_hidden_files: bool,
        path: String,
        depth: usize,
    ) {
        if depth > 32 || !visited.insert(path.clone()) {
            return;
        }
        let expanded = path == "/" || sftp.expanded_directories.contains(&path);
        rows.push(path.clone());

        if !expanded {
            return;
        }

        if let Some(entries) = sftp.directory_entries.get(&path) {
            for entry in entries
                .iter()
                .filter(|entry| entry.is_dir)
                .filter(|entry| show_hidden_files || !entry.name.starts_with('.'))
            {
                append_rows(
                    rows,
                    visited,
                    sftp,
                    show_hidden_files,
                    entry.full_path.clone(),
                    depth + 1,
                );
            }
        }
    }

    let mut rows = Vec::new();
    let mut visited = std::collections::HashSet::new();
    append_rows(
        &mut rows,
        &mut visited,
        sftp,
        show_hidden_files,
        "/".to_string(),
        0,
    );
    rows
}

#[cfg(test)]
mod tests {
    use crate::TinyShell;
    use gpui::{Bounds, px};

    fn vertical_bounds(top: f32, height: f32) -> Bounds<gpui::Pixels> {
        Bounds::new(
            gpui::point(px(0.), px(top)),
            gpui::size(px(100.), px(height)),
        )
    }

    #[test]
    fn centers_a_tree_target_for_explicit_location() {
        assert_eq!(
            super::centered_sftp_tree_scroll_offset_y(
                px(-300.),
                vertical_bounds(100., 200.),
                vertical_bounds(250., 30.),
            ),
            px(-365.)
        );
    }

    #[test]
    fn builds_remote_path_chain_from_root() {
        assert_eq!(
            TinyShell::sftp_path_chain("/data/docker/mysql"),
            ["/", "/data", "/data/docker", "/data/docker/mysql"]
        );
    }

    #[test]
    fn normalizes_home_and_trailing_slashes() {
        assert_eq!(TinyShell::normalize_sftp_path("~", "/root"), "/root");
        assert_eq!(
            TinyShell::normalize_sftp_path("~/data/", "/root"),
            "/root/data"
        );
        assert_eq!(TinyShell::normalize_sftp_path("/data/", "/root"), "/data");
        assert_eq!(
            TinyShell::normalize_sftp_path("data/logs", "/root"),
            "/data/logs"
        );
    }
}
