use gpui::{Context, PathPromptOptions, Pixels, Point, Window};

#[derive(Clone)]
pub(crate) struct SftpTreeRow {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub expanded: bool,
}

use crate::{
    TinyShell, SftpContextMenuState,
    sftp::{RemoteEntry, SftpHandle},
    terminal,
};

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
        self.active_group
            .as_ref()
            .and_then(|id| self.tab_groups.iter().find(|g| &g.id == id))
            .and_then(|g| g.sftp.as_ref())
    }

    pub(crate) fn active_sftp_mut(&mut self) -> Option<&mut terminal::SftpUiState> {
        let active_id = self.active_group.clone()?;
        self.tab_groups
            .iter_mut()
            .find(|g| g.id == active_id)
            .and_then(|g| g.sftp.as_mut())
    }

    pub(crate) fn active_sftp_handle(&self) -> Option<&SftpHandle> {
        self.active_group
            .as_ref()
            .and_then(|id| self.sftp_handles.get(id))
    }

    /// 双击文本文件时调用:下载文件内容到内存,打开独立编辑器窗口。
    /// 若同一会话的编辑器已打开该文件,则直接激活窗口并切换 tab。
    pub(crate) fn open_file_in_editor(&mut self, remote_path: String, cx: &mut Context<Self>) {
        let Some(session_id) = self.active_group.clone() else {
            return;
        };
        if crate::app::sftp_editor_window::focus_path(&session_id, &remote_path, cx) {
            return;
        }
        if let Some(handle) = self.sftp_handles.get(&session_id) {
            tracing::info!("[sftp] opening in-memory editor: '{}'", remote_path);
            handle.download_file_content(remote_path);
            cx.notify();
        }
    }

    pub(crate) fn navigate_sftp(&mut self, path: String, cx: &mut Context<Self>) {
        if let Some(group_id) = self.active_group.clone()
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
            .tab_groups
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
        if self.active_group.as_deref() == Some(group_id) {
            self.pending_sftp_path_sync = Some(path.clone());
            self.pending_sftp_tree_scroll_path = Some(path.clone());
        }

        for directory in missing_ancestors {
            handle.list_directory_tree(directory);
        }
        handle.list_dir(path);
        true
    }

    pub(crate) fn sync_sftp_tree_scroll(&mut self) {
        let Some(path) = self.pending_sftp_tree_scroll_path.clone() else {
            return;
        };
        let Some(sftp) = self.active_sftp() else {
            return;
        };
        let Some(index) = sftp_tree_rows(sftp, self.show_hidden_files)
            .iter()
            .position(|row| row.path == path)
        else {
            return;
        };
        self.sftp_tree_scroll_handle.scroll_to_item(index);
        self.pending_sftp_tree_scroll_path = None;
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
        let Some(path) = self.pending_sftp_path_sync.take() else {
            return;
        };
        self.sftp_path_input.update(cx, |state, cx| {
            state.set_value(path, window, cx);
        });
    }

    pub(crate) fn open_sftp_context_menu(
        &mut self,
        remote_path: String,
        is_dir: bool,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu_epoch = self.context_menu_epoch.wrapping_add(1);
        self.sftp_context_menu = Some(SftpContextMenuState {
            remote_path,
            is_dir,
            position,
        });
        cx.notify();
    }

    pub(crate) fn dismiss_sftp_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.sftp_context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn trigger_sftp_context_download(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.sftp_context_menu.take() else {
            return;
        };
        self.download_sftp_entry(menu.remote_path, window, cx);
        cx.notify();
    }

    pub(crate) fn trigger_sftp_context_edit(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.sftp_context_menu.take() else {
            return;
        };
        if let Some(handle) = self.active_sftp_handle() {
            tracing::info!("[sftp] triggering edit for file: '{}'", menu.remote_path);
            handle.edit_file(menu.remote_path);
        }
        cx.notify();
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
        let path_prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select Download Folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            match path_prompt.await {
                Ok(Ok(Some(mut paths))) => {
                    if let Some(folder) = paths.pop() {
                        let local_path = folder.to_string_lossy().to_string();
                        tracing::info!(
                            "[sftp] initiating download of '{}' to '{}'",
                            remote_path,
                            local_path
                        );
                        handle.download(remote_path, local_path);
                        this.update(cx, |this, cx| {
                            this.show_transfers_dialog = true;
                            cx.notify();
                        })?;
                    }
                }
                Ok(Err(err)) => {
                    this.update(cx, |this, cx| {
                        this.status = format!("download picker failed: {err}").into();
                        cx.notify();
                    })?;
                }
                _ => {}
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    pub(crate) fn upload_sftp_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(handle) = self.active_sftp_handle().cloned() else {
            return;
        };
        let remote_dir = self
            .active_sftp()
            .map(|sftp| sftp.current_path.clone())
            .unwrap_or_else(|| "/".into());
        let path_prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select File to Upload".into()),
        });
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
                        this.update(cx, |this, cx| {
                            this.show_transfers_dialog = true;
                            cx.notify();
                        })?;
                    }
                }
                Ok(Err(err)) => {
                    this.update(cx, |this, cx| {
                        this.status = format!("upload picker failed: {err}").into();
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
            prompt: Some("Select Folder to Upload".into()),
        });
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
                        this.update(cx, |this, cx| {
                            this.show_transfers_dialog = true;
                            cx.notify();
                        })?;
                    }
                }
                Ok(Err(err)) => {
                    this.update(cx, |this, cx| {
                        this.status = format!("upload picker failed: {err}").into();
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

        let path_prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select Download Folder".into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = path_prompt.await {
                if let Some(folder) = paths.pop() {
                    let local_dir = folder.to_string_lossy().to_string();
                    tracing::info!(
                        "[sftp] initiating batch download of {} entries to '{}'",
                        selected.len(),
                        local_dir
                    );
                    for remote in selected {
                        let _ = handle.commands.send(crate::sftp::SftpCommand::Download {
                            remote,
                            local_dir: local_dir.clone(),
                        });
                    }

                    let _ = this.update(cx, |this, cx| {
                        if let Some(sftp_mut) = this.active_sftp_mut() {
                            sftp_mut.selected_entries.clear();
                        }
                        this.show_transfers_dialog = true;
                        cx.notify();
                    });
                }
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    pub(crate) fn upload_sftp_files_batch(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
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
                let _ = handle.commands.send(crate::sftp::SftpCommand::UploadPaths {
                    locals: paths,
                    remote_dir: sftp.current_path.clone(),
                });
                self.show_transfers_dialog = true;
                cx.notify();
            }
        }
    }
}

pub(crate) fn sftp_tree_rows(
    sftp: &terminal::SftpUiState,
    show_hidden_files: bool,
) -> Vec<SftpTreeRow> {
    fn append_rows(
        rows: &mut Vec<SftpTreeRow>,
        visited: &mut std::collections::HashSet<String>,
        sftp: &terminal::SftpUiState,
        show_hidden_files: bool,
        path: &str,
        name: String,
        depth: usize,
    ) {
        if depth > 32 || !visited.insert(path.to_string()) {
            return;
        }
        let expanded = path == "/" || sftp.expanded_directories.contains(path);
        rows.push(SftpTreeRow {
            path: path.to_string(),
            name,
            depth,
            expanded,
        });

        if !expanded {
            return;
        }

        if let Some(entries) = sftp.directory_entries.get(path) {
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
                    &entry.full_path,
                    entry.name.clone(),
                    depth + 1,
                );
            }
        }
    }

    let root = "/".to_string();
    let mut rows = Vec::new();
    let mut visited = std::collections::HashSet::new();
    append_rows(
        &mut rows,
        &mut visited,
        sftp,
        show_hidden_files,
        &root,
        "/".to_string(),
        0,
    );
    rows
}

#[cfg(test)]
mod tests {
    use crate::TinyShell;

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
        assert_eq!(TinyShell::normalize_sftp_path("data/logs", "/root"), "/data/logs");
    }
}
