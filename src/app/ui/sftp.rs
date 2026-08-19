use super::*;
use std::sync::Arc;

const SFTP_TREE_MAX_DEPTH: usize = 32;
const SFTP_TREE_INDENT_PX: f32 = 15.;
const SFTP_TREE_ROW_PADDING_LEFT_PX: f32 = 9.;
const SFTP_TREE_GUIDE_CENTER_PX: f32 = 7.;
const SFTP_TREE_SCROLLBAR_SIZE_PX: f32 = 16.;

#[derive(Clone, Copy)]
struct SftpTreeColors {
    primary: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    secondary: Hsla,
    background: Hsla,
}

#[derive(Clone, Copy)]
struct SftpTreeRenderRow<'a> {
    path: &'a str,
    name: &'a str,
    depth: usize,
    ancestor_continuation_mask: u64,
    is_last_sibling: bool,
    expanded: bool,
    permissions: Option<u32>,
}

struct SftpEntriesRenderSnapshot {
    entries: Arc<[crate::sftp::RemoteEntry]>,
    selected_entries: Arc<HashSet<String>>,
    selected_path: Option<Arc<str>>,
    all_selected: bool,
}

impl SftpEntriesRenderSnapshot {
    fn new(sftp: &terminal::SftpUiState, show_hidden_files: bool) -> Self {
        // `uniform_list` retains a `'static` renderer. Build one owned snapshot
        // for that renderer, then share it with row listeners through `Arc`
        // instead of cloning the table and selection set at each layer.
        let entries = sftp
            .directory_entries
            .get(&sftp.current_path)
            .unwrap_or(&sftp.entries)
            .iter()
            .filter(|entry| show_hidden_files || !entry.name.starts_with('.'))
            .cloned()
            .collect::<Vec<_>>();
        let all_selected = !entries.is_empty()
            && entries
                .iter()
                .all(|entry| sftp.selected_entries.contains(&entry.full_path));

        Self {
            entries: entries.into(),
            selected_entries: Arc::new(sftp.selected_entries.clone()),
            selected_path: sftp.selected_path.as_deref().map(Arc::from),
            all_selected,
        }
    }
}

fn sftp_tree_render_rows(
    sftp: &terminal::SftpUiState,
    show_hidden_files: bool,
) -> Vec<SftpTreeRenderRow<'_>> {
    // Tree rows only live while this render function builds elements, so they
    // can borrow directory data. Each rendered row promotes just its path to
    // `Arc<str>` for the event listeners that outlive this traversal.
    #[allow(clippy::too_many_arguments)]
    fn append_rows<'a>(
        rows: &mut Vec<SftpTreeRenderRow<'a>>,
        visited: &mut HashSet<&'a str>,
        sftp: &'a terminal::SftpUiState,
        show_hidden_files: bool,
        path: &'a str,
        name: &'a str,
        depth: usize,
        ancestor_continuation_mask: u64,
        is_last_sibling: bool,
        permissions: Option<u32>,
    ) {
        if depth > SFTP_TREE_MAX_DEPTH || !visited.insert(path) {
            return;
        }
        let expanded = path == "/" || sftp.expanded_directories.contains(path);
        rows.push(SftpTreeRenderRow {
            path,
            name,
            depth,
            ancestor_continuation_mask,
            is_last_sibling,
            expanded,
            permissions,
        });

        if !expanded || depth == SFTP_TREE_MAX_DEPTH {
            return;
        }
        if let Some(entries) = sftp.directory_entries.get(path) {
            let visible_directories = entries
                .iter()
                .filter(|entry| entry.is_dir)
                .filter(|entry| show_hidden_files || !entry.name.starts_with('.'))
                .collect::<Vec<_>>();
            let child_ancestor_continuation_mask = if depth > 0 && !is_last_sibling {
                ancestor_continuation_mask | (1 << (depth - 1))
            } else {
                ancestor_continuation_mask
            };
            let child_count = visible_directories.len();

            for (index, entry) in visible_directories.into_iter().enumerate() {
                append_rows(
                    rows,
                    visited,
                    sftp,
                    show_hidden_files,
                    &entry.full_path,
                    &entry.name,
                    depth + 1,
                    child_ancestor_continuation_mask,
                    index + 1 == child_count,
                    Some(entry.permissions),
                );
            }
        }
    }

    let mut rows = Vec::new();
    let mut visited = HashSet::new();
    append_rows(
        &mut rows,
        &mut visited,
        sftp,
        show_hidden_files,
        "/",
        "/",
        0,
        0,
        true,
        None,
    );
    rows
}

fn sftp_tree_branch_guides(
    row: SftpTreeRenderRow<'_>,
    ancestor_color: Hsla,
    branch_color: Hsla,
) -> Vec<AnyElement> {
    if row.depth == 0 {
        return Vec::new();
    }

    let mut guides = Vec::with_capacity(row.depth + 1);
    for level in 0..row.depth.saturating_sub(1) {
        if row.ancestor_continuation_mask & (1 << level) == 0 {
            continue;
        }
        let left = SFTP_TREE_ROW_PADDING_LEFT_PX
            + level as f32 * SFTP_TREE_INDENT_PX
            + SFTP_TREE_GUIDE_CENTER_PX;
        guides.push(
            div()
                .absolute()
                .left(px(left))
                .top(px(-1.))
                .bottom(px(-1.))
                .w(px(1.))
                .bg(ancestor_color)
                .into_any_element(),
        );
    }

    let branch_left = SFTP_TREE_ROW_PADDING_LEFT_PX
        + (row.depth - 1) as f32 * SFTP_TREE_INDENT_PX
        + SFTP_TREE_GUIDE_CENTER_PX;
    guides.push(
        div()
            .absolute()
            .left(px(branch_left))
            .top(px(-1.))
            .w(px(1.))
            .when(row.is_last_sibling, |this| this.bottom(relative(0.5)))
            .when(!row.is_last_sibling, |this| this.bottom(px(-1.)))
            .bg(branch_color)
            .into_any_element(),
    );
    guides.push(
        div()
            .absolute()
            .left(px(branch_left))
            .top(relative(0.5))
            .w(px(SFTP_TREE_INDENT_PX - SFTP_TREE_GUIDE_CENTER_PX))
            .h(px(1.))
            .bg(branch_color)
            .into_any_element(),
    );
    guides
}

impl TinyShell {
    pub(crate) fn toggle_clean_mode(&mut self, cx: &mut Context<Self>) {
        self.workspace_mode.toggle_clean();
        cx.notify();
    }

    pub(crate) fn toggle_sftp_minimized(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.body_panels.clone();
        let presentation = self.workspace_mode.presentation(self.sftp_panel.minimized);
        let minimized = presentation.sftp_minimized;
        self.sftp_panel.minimize_epoch = self.sftp_panel.minimize_epoch.wrapping_add(1);

        if !minimized {
            let sizes = state.read(cx).sizes();
            if sizes.len() > 1 {
                self.monitoring.prev_monitoring_size = Some(sizes[1]);
            }
        } else {
            let monitoring_position =
                MonitoringPosition::from_config(self.config.monitoring_position());
            let prev_size = self
                .monitoring
                .prev_monitoring_size
                .or_else(|| {
                    self.config
                        .body_panels()
                        .and_then(|sizes| sizes.get(1).copied())
                        .map(px)
                })
                .unwrap_or_else(|| {
                    px(
                        workspace_body_metrics(monitoring_position, presentation.clean)
                            .default_panel_height,
                    )
                });

            cx.on_next_frame(
                window,
                move |_this: &mut crate::app::TinyShell,
                      window: &mut gpui::Window,
                      cx: &mut gpui::Context<crate::app::TinyShell>| {
                    cx.on_next_frame(
                        window,
                        move |this: &mut crate::app::TinyShell,
                              window: &mut gpui::Window,
                              cx: &mut gpui::Context<crate::app::TinyShell>| {
                            this.body_panels.update(cx, |state, cx| {
                                let sizes = state.sizes();
                                let c_size_f32: f32 = sizes.iter().map(|s| s.as_f32()).sum();
                                let c_size = px(c_size_f32);

                                if c_size > px(0.0) && prev_size < c_size {
                                    let target_p0 = c_size - prev_size;
                                    state.resize_panel(0, target_p0, window, cx);
                                }
                            });
                            cx.notify();
                        },
                    );
                },
            );
        }

        if presentation.clean {
            self.workspace_mode.toggle_clean_sftp();
        } else {
            self.sftp_panel.minimized = !minimized;
            self.config
                .set_sftp_panel_minimized(self.sftp_panel.minimized);
            self.mark_config_preferences_dirty();
        }
        cx.notify();
    }

    fn render_sftp_tree_row(
        &self,
        row: SftpTreeRenderRow<'_>,
        current_path: &str,
        colors: SftpTreeColors,
        capture_scroll_target: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path: Arc<str> = Arc::from(row.path);
        let toggle_path = Arc::clone(&path);
        let context_path = Arc::clone(&path);
        let context_permissions = row.permissions;
        let view = cx.entity();
        let scroll_target_view = view.clone();
        let scroll_target_path = Arc::clone(&context_path);
        let is_current = current_path == path.as_ref();
        let folder_icon = if row.expanded {
            IconName::FolderOpen
        } else {
            IconName::Folder
        };
        let branch_guides = sftp_tree_branch_guides(
            row,
            colors.muted_foreground.opacity(0.14),
            colors.muted_foreground.opacity(0.24),
        );
        let tree_toggle = if path.as_ref() == "/" {
            div().w(px(16.)).flex_none().into_any_element()
        } else {
            div()
                .w(px(16.))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(colors.muted_foreground)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.toggle_sftp_tree_directory(toggle_path.as_ref().to_owned(), cx);
                        cx.stop_propagation();
                    }),
                )
                .child(
                    Icon::new(if row.expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .with_size(Size::Small),
                )
                .into_any_element()
        };

        h_flex()
            .id(format!("sftp-tree-row-{}", context_path.as_ref()))
            .relative()
            .when(capture_scroll_target, move |this| {
                this.on_prepaint(move |bounds, _, cx| {
                    scroll_target_view.update(cx, |this, _| {
                        if this.sftp_workspace.pending_tree_scroll_path.as_deref()
                            == Some(scroll_target_path.as_ref())
                        {
                            this.sftp_workspace.tree_scroll_target_bounds =
                                Some((scroll_target_path.to_string(), bounds));
                        }
                    });
                })
            })
            .min_w_full()
            .h(px(30.))
            .flex_shrink_0()
            .pl(px(
                SFTP_TREE_ROW_PADDING_LEFT_PX + row.depth as f32 * SFTP_TREE_INDENT_PX
            ))
            .pr(px(SFTP_TREE_SCROLLBAR_SIZE_PX + 8.))
            .items_center()
            .gap(px(5.))
            .rounded_sm()
            .cursor_pointer()
            .bg(if is_current {
                colors.secondary.opacity(0.62)
            } else {
                colors.background.opacity(0.)
            })
            .when(!is_current, |this| {
                this.hover(|style| style.bg(colors.secondary.opacity(0.38)))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.select_sftp_tree_directory(path.as_ref().to_owned(), cx);
                }),
            )
            .children(branch_guides)
            .child(tree_toggle)
            .child(
                Icon::new(folder_icon)
                    .with_size(Size::Small)
                    .text_color(if is_current {
                        colors.primary
                    } else {
                        colors.muted_foreground
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_size(rems(0.92))
                    .text_color(if is_current {
                        colors.foreground
                    } else {
                        colors.muted_foreground
                    })
                    .when(is_current, |style| style.font_weight(FontWeight::MEDIUM))
                    .child(row.name.to_string()),
            )
            .context_menu(move |menu, window, cx| {
                Self::build_sftp_tree_context_menu(
                    menu,
                    view.clone(),
                    context_path.as_ref().to_owned(),
                    context_permissions,
                    window,
                    cx,
                )
            })
            .into_any_element()
    }

    pub(super) fn render_sftp_directory_tree(
        &self,
        sftp: &terminal::SftpUiState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = {
            let theme = cx.theme();
            SftpTreeColors {
                primary: theme.primary,
                foreground: theme.foreground,
                muted_foreground: theme.muted_foreground,
                secondary: theme.secondary,
                background: theme.background,
            }
        };
        let rows = sftp_tree_render_rows(sftp, self.sftp_panel.show_hidden_files)
            .into_iter()
            .map(|row| {
                let capture_scroll_target =
                    self.sftp_workspace.pending_tree_scroll_path.as_deref() == Some(row.path);
                self.render_sftp_tree_row(
                    row,
                    &sftp.current_path,
                    colors,
                    capture_scroll_target,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let empty_context_path = sftp.current_path.clone();
        let view = cx.entity();

        v_flex()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h(px(26.))
                    .px_3()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().muted.opacity(0.8))
                    .text_size(rems(0.917))
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::FolderOpen).with_size(Size::Small))
                    .child(t!("remote_files")),
            )
            .child(
                div().relative().flex_1().min_h(px(0.)).child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right(px(4.))
                        .bottom_0()
                        .child(
                            v_flex()
                                .id("sftp-directory-tree")
                                .size_full()
                                .items_start()
                                .track_scroll(&self.sftp_workspace.tree_scroll_handle)
                                .overflow_scroll()
                                .child(
                                    v_flex()
                                        .min_w_full()
                                        .min_h(relative(1.))
                                        .flex_shrink_0()
                                        .items_stretch()
                                        .pt_1()
                                        .gap(px(1.))
                                        .children(rows)
                                        .child(
                                            div()
                                                .id("sftp-tree-empty-area")
                                                .w_full()
                                                .min_h(px(36.))
                                                .flex_1()
                                                .context_menu(move |menu, window, cx| {
                                                    Self::build_sftp_tree_empty_context_menu(
                                                        menu,
                                                        view.clone(),
                                                        empty_context_path.clone(),
                                                        window,
                                                        cx,
                                                    )
                                                }),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .child(
                                    Scrollbar::new(&self.sftp_workspace.tree_scroll_handle)
                                        .scrollbar_show(ScrollbarShow::Scrolling),
                                ),
                        ),
                ),
            )
            .into_any_element()
    }

    pub(super) fn set_sftp_panel_view(&mut self, view: SftpPanelView, cx: &mut Context<Self>) {
        if self.sftp_panel.view == view {
            return;
        }
        self.sftp_panel.view = view;
        self.selected_quick_command = None;
        self.config.set_sftp_panel_view(match view {
            SftpPanelView::Files => "files",
            SftpPanelView::Commands => "commands",
        });
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(super) fn toggle_sftp_toolbar_item(
        &mut self,
        item: SftpToolbarItem,
        cx: &mut Context<Self>,
    ) {
        let mut visibility = self.config.sftp_toolbar_visibility();
        match item {
            SftpToolbarItem::SyncCwd => visibility.sync_cwd = !visibility.sync_cwd,
            SftpToolbarItem::HiddenFiles => visibility.hidden_files = !visibility.hidden_files,
            SftpToolbarItem::Refresh => visibility.refresh = !visibility.refresh,
            SftpToolbarItem::NewFolder => visibility.new_folder = !visibility.new_folder,
            SftpToolbarItem::Delete => visibility.delete = !visibility.delete,
            SftpToolbarItem::UploadFile => visibility.upload_file = !visibility.upload_file,
            SftpToolbarItem::UploadFolder => visibility.upload_folder = !visibility.upload_folder,
            SftpToolbarItem::Download => visibility.download = !visibility.download,
        }
        self.config.set_sftp_toolbar_visibility(visibility);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(super) fn toggle_sftp_footer_item(&mut self, item: SftpFooterItem, cx: &mut Context<Self>) {
        let mut visibility = self.config.sftp_footer_visibility();
        match item {
            SftpFooterItem::SyncStatus => visibility.webdav = !visibility.webdav,
            SftpFooterItem::Latency => visibility.latency = !visibility.latency,
            SftpFooterItem::Transfers => visibility.transfers = !visibility.transfers,
            SftpFooterItem::PanelToggle => visibility.panel_toggle = !visibility.panel_toggle,
        }
        self.config.set_sftp_footer_visibility(visibility);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(super) fn build_sftp_toolbar_visibility_menu(
        mut menu: PopupMenu,
        view: gpui::Entity<TinyShell>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let visibility = view.read(cx).config.sftp_toolbar_visibility();
        let items = [
            (
                SftpToolbarItem::SyncCwd,
                t!("sync_cwd").to_string(),
                visibility.sync_cwd,
            ),
            (
                SftpToolbarItem::HiddenFiles,
                t!("hidden").to_string(),
                visibility.hidden_files,
            ),
            (
                SftpToolbarItem::Refresh,
                t!("refresh").to_string(),
                visibility.refresh,
            ),
            (
                SftpToolbarItem::NewFolder,
                t!("new_folder").to_string(),
                visibility.new_folder,
            ),
            (
                SftpToolbarItem::Delete,
                t!("delete_selected").to_string(),
                visibility.delete,
            ),
            (
                SftpToolbarItem::UploadFile,
                t!("upload_file").to_string(),
                visibility.upload_file,
            ),
            (
                SftpToolbarItem::UploadFolder,
                t!("upload_folder").to_string(),
                visibility.upload_folder,
            ),
            (
                SftpToolbarItem::Download,
                t!("download").to_string(),
                visibility.download,
            ),
        ];
        for (item, label, checked) in items {
            menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                window.listener_for(&view, move |this, _, _, cx| {
                    this.toggle_sftp_toolbar_item(item, cx);
                }),
            ));
        }
        menu
    }

    pub(super) fn build_sftp_footer_visibility_menu(
        mut menu: PopupMenu,
        view: gpui::Entity<TinyShell>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let visibility = view.read(cx).config.sftp_footer_visibility();
        let items = [
            (
                SftpFooterItem::SyncStatus,
                t!("sftp_footer_latest_sync_status").to_string(),
                visibility.webdav,
            ),
            (
                SftpFooterItem::Latency,
                t!("sftp_latency").to_string(),
                visibility.latency,
            ),
            (
                SftpFooterItem::Transfers,
                t!("transfers").to_string(),
                visibility.transfers,
            ),
            (
                SftpFooterItem::PanelToggle,
                t!("sftp_footer_panel_toggle").to_string(),
                visibility.panel_toggle,
            ),
        ];
        for (item, label, checked) in items {
            menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                window.listener_for(&view, move |this, _, _, cx| {
                    this.toggle_sftp_footer_item(item, cx);
                }),
            ));
        }
        menu
    }

    pub(super) fn render_quick_commands(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let categories = self
            .config
            .quick_command_categories()
            .unwrap_or_default()
            .to_vec();
        if !categories.is_empty() {
            self.quick_command_category = self.quick_command_category.min(categories.len() - 1);
        } else {
            self.quick_command_category = 0;
        }
        let selected_commands = categories
            .get(self.quick_command_category)
            .map(|category| category.commands.clone())
            .unwrap_or_default();
        let selected_category_id = categories
            .get(self.quick_command_category)
            .map(|category| category.id.clone());
        let quick_command_detail_visible =
            self.selected_quick_command
                .as_ref()
                .is_some_and(|(category_id, command_id)| {
                    selected_category_id.as_deref() == Some(category_id.as_str())
                        && selected_commands
                            .iter()
                            .any(|command| command.id == *command_id)
                });
        let has_categories = !categories.is_empty();

        v_flex()
            .flex_1()
            .min_h(px(0.))
            .child(
                h_flex()
                    .flex_none()
                    .h(px(36.))
                    .items_center()
                    .gap_1()
                    .px_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .children(categories.iter().enumerate().map(|(index, category)| {
                        Button::new(("quick-command-category", index))
                            .ghost()
                            .small()
                            .selected(self.quick_command_category == index)
                            .icon(IconName::Folder)
                            .label(category.name.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.quick_command_category = index;
                                this.selected_quick_command = None;
                                cx.notify();
                            }))
                    }))
                    .child(div().flex_1())
                    .child(
                        Button::new("quick-command-manage")
                            .ghost()
                            .small()
                            .icon(IconName::Settings)
                            .label(t!("quick_command_manage").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.home_page_open = true;
                                this.set_home_page(HomePage::Commands, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(
                        h_resizable("sftp-quick-command-detail-split")
                            .child(
                                resizable_panel()
                                    .size_range(px(240.)..Pixels::MAX)
                                    .child(
                                div()
                                    .size_full()
                                    .min_w(px(0.))
                                    .min_h(px(0.))
                                    .overflow_y_scrollbar()
                                    .p_3()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .items_start()
                                            .gap_2()
                                            .children(selected_commands.into_iter().enumerate().map(
                                        |(index, command)| {
                                            let category_id = selected_category_id
                                                .clone()
                                                .unwrap_or_default();
                                            let command_id = command.id.clone();
                                            let edit_category_id = category_id.clone();
                                            let edit_command_id = command.id.clone();
                                            let selected = self.selected_quick_command.as_ref()
                                                == Some(&(category_id.clone(), command.id.clone()));
                                            div()
                                                .id(("quick-command", index))
                                                .flex_none()
                                                .cursor_pointer()
                                                .rounded_md()
                                                .border_1()
                                                .border_color(if selected {
                                                    cx.theme().primary
                                                } else {
                                                    cx.theme().border
                                                })
                                                .bg(if selected {
                                                    cx.theme().tab_active
                                                } else {
                                                    cx.theme().background
                                                })
                                                .hover(|this| this.bg(cx.theme().secondary))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                                        if event.click_count >= 2 {
                                                            this.execute_quick_command(
                                                                category_id.clone(),
                                                                command_id.clone(),
                                                                false,
                                                                window,
                                                                cx,
                                                            );
                                                        } else {
                                                            this.select_quick_command(
                                                                category_id.clone(),
                                                                command_id.clone(),
                                                                window,
                                                                cx,
                                                            );
                                                        }
                                                    }),
                                                )
                                                .context_menu({
                                                    let view = cx.entity();
                                                    move |menu, window, _| {
                                                        menu.item(
                                                            PopupMenuItem::new(t!("edit").to_string())
                                                                .on_click(window.listener_for(&view, {
                                                                    let category_id = edit_category_id.clone();
                                                                    let command_id = edit_command_id.clone();
                                                                    let dialog_view = view.clone();
                                                                    move |_this, _, window, cx| {
                                                                        cx.stop_propagation();
                                                                        let category_id = category_id.clone();
                                                                        let command_id = command_id.clone();
                                                                        let dialog_view = dialog_view.clone();
                                                                        window.defer(cx, move |window, cx| {
                                                                            dialog_view.update(cx, |this, cx| {
                                                                                this.show_quick_command_dialog(
                                                                                    category_id,
                                                                                    Some(command_id),
                                                                                    window,
                                                                                    cx,
                                                                                );
                                                                            });
                                                                        });
                                                                    }
                                                                })),
                                                        )
                                                    }
                                                })
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .px_3()
                                                        .py_2()
                                                        .child(
                                                            Icon::new(IconName::SquareTerminal)
                                                                .with_size(Size::Small),
                                                        )
                                                        .child(command.name),
                                                )
                                        },
                                            ))
                                            .when(!has_categories, |this| {
                                                this.child(
                                                    v_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .justify_center()
                                                        .gap_2()
                                                        .py_5()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(t!("command_manager_empty_title")),
                                                )
                                            }),
                                    ),
                                    ),
                            )
                            .child(
                                resizable_panel()
                                    .size(px(420.))
                                    .size_range(px(280.)..px(720.))
                                    .flex_none()
                                    .visible(quick_command_detail_visible)
                                    .child(self.render_quick_command_detail(false, true, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn sftp_transfer_summary(
        &self,
        kind: crate::terminal::TransferType,
    ) -> Option<(String, String, f32)> {
        let active = self
            .transfers
            .iter()
            .filter(|transfer| {
                matches!(
                    transfer.state,
                    crate::terminal::TransferState::Running
                        | crate::terminal::TransferState::Paused
                ) && transfer.info.kind == kind
            })
            .collect::<Vec<_>>();
        if active.is_empty() {
            return None;
        }

        if active.len() == 1 {
            let transfer = active[0];
            let percent = transfer.total.and_then(|total| {
                if total > 0 {
                    Some((transfer.transferred as f64 / total as f64 * 100.0) as f32)
                } else {
                    None
                }
            });
            return Some(match percent {
                Some(percent) => (
                    transfer.info.name.clone(),
                    format!("{percent:.0}%"),
                    percent,
                ),
                None => (transfer.info.name.clone(), "-".to_string(), 0.0),
            });
        }

        let total_transferred = active
            .iter()
            .map(|transfer| transfer.transferred)
            .sum::<u64>();
        let total_size = active
            .iter()
            .filter_map(|transfer| transfer.total)
            .sum::<u64>();
        let label = match kind {
            crate::terminal::TransferType::Download => {
                t!("files_downloading", count = active.len()).to_string()
            }
            crate::terminal::TransferType::Upload => {
                t!("files_uploading", count = active.len()).to_string()
            }
        };
        if total_size == 0 {
            return Some((label, "-".to_string(), 0.0));
        }

        let percent = (total_transferred as f64 / total_size as f64 * 100.0) as f32;
        Some((label, format!("{percent:.0}%"), percent))
    }

    pub(super) fn render_sftp_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let visibility = self.config.sftp_footer_visibility();
        let latency = self.active_sftp().and_then(|sftp| sftp.latency_ms);
        let dl_summary = self.sftp_transfer_summary(crate::terminal::TransferType::Download);
        let ul_summary = self.sftp_transfer_summary(crate::terminal::TransferType::Upload);
        let has_transfers = dl_summary.is_some() || ul_summary.is_some();
        let presentation = self.workspace_mode.presentation(self.sftp_panel.minimized);
        let webdav_enabled = self.config.sync_enabled() && self.config.sync_backend() == "webdav";
        let sync_failed = self.sync_runtime.failed;
        let latest_sync_status = self.sync_runtime.status.clone();
        let view = cx.entity();
        h_flex()
            .w_full()
            .flex_none()
            .h(px(24.))
            .px_3()
            .items_center()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tab_bar)
            .occlude()
            .child(div().flex_1())
            .when(visibility.webdav && webdav_enabled, |this| {
                this.child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .mr_3()
                        .text_size(rems(0.72))
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::Globe).with_size(Size::Small))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("WebDAV"))
                        .child(div().child("·"))
                        .child(
                            div()
                                .text_color(if sync_failed {
                                    cx.theme().danger
                                } else if self.sync_runtime.in_progress {
                                    cx.theme().primary
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .child(latest_sync_status),
                        ),
                )
            })
            .when(visibility.latency, |this| {
                this.child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .mr_2()
                        .text_size(rems(0.833))
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::Network).with_size(Size::Small))
                        .child(t!(
                            "sftp_latency_value",
                            latency = latency
                                .map(|latency| latency.to_string())
                                .unwrap_or_else(|| "--".to_string())
                        )),
                )
            })
            .when(visibility.transfers, |this| {
                this.child(
                    Button::new("open-transfers")
                        .ghost()
                        .small()
                        .when(has_transfers, |this| {
                            let mut content = h_flex().items_center().gap_2();
                            if let Some((ref label, ref pct_display, pct)) = dl_summary {
                                content = content.child(
                                    h_flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            Icon::new(IconName::ArrowDown)
                                                .with_size(Size::Small)
                                                .text_color(cx.theme().primary),
                                        )
                                        .child(
                                            div()
                                                .text_size(rems(0.833))
                                                .text_color(cx.theme().primary)
                                                .italic()
                                                .child(label.clone()),
                                        )
                                        .child(
                                            Progress::new("sftp-status-dl")
                                                .with_size(px(4.))
                                                .value(pct)
                                                .color(cx.theme().primary)
                                                .w(px(50.0)),
                                        )
                                        .child(
                                            div()
                                                .text_size(rems(0.833))
                                                .text_color(cx.theme().primary)
                                                .italic()
                                                .child(pct_display.clone()),
                                        ),
                                );
                            }
                            if let Some((ref label, ref pct_display, pct)) = ul_summary {
                                if dl_summary.is_some() {
                                    content = content.child(div().w(px(6.)));
                                }
                                content = content.child(
                                    h_flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            Icon::new(IconName::ArrowUp)
                                                .with_size(Size::Small)
                                                .text_color(cx.theme().primary),
                                        )
                                        .child(
                                            div()
                                                .text_size(rems(0.833))
                                                .text_color(cx.theme().primary)
                                                .italic()
                                                .child(label.clone()),
                                        )
                                        .child(
                                            Progress::new("sftp-status-ul")
                                                .with_size(px(4.))
                                                .value(pct)
                                                .color(cx.theme().primary)
                                                .w(px(50.0)),
                                        )
                                        .child(
                                            div()
                                                .text_size(rems(0.833))
                                                .text_color(cx.theme().primary)
                                                .italic()
                                                .child(pct_display.clone()),
                                        ),
                                );
                            }
                            this.child(content)
                        })
                        .when(!has_transfers, |this| {
                            this.icon(IconName::ArrowDown)
                                .label(t!("transfers").to_string())
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.show_transfers_dialog(window, cx);
                        })),
                )
            })
            .when(visibility.panel_toggle, |this| {
                this.child(
                    Button::new("sftp-footer-panel-toggle")
                        .ghost()
                        .xsmall()
                        .icon(if presentation.sftp_minimized {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .tooltip(if presentation.sftp_minimized {
                            t!("panel_expand").to_string()
                        } else {
                            t!("panel_minimize").to_string()
                        })
                        .disabled(self.active_kind() != Some(TabKind::Ssh))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_sftp_minimized(window, cx);
                        })),
                )
            })
            .context_menu({
                let view = view.clone();
                move |menu, window, cx| {
                    Self::build_sftp_footer_visibility_menu(menu, view.clone(), window, cx)
                }
            })
            .into_any_element()
    }

    pub(super) fn render_sftp_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active_sftp = self.active_sftp();
        let presentation = self.workspace_mode.presentation(self.sftp_panel.minimized);
        // 目录内容更新不应触发整个面板淡入，否则每次进入子目录都会闪烁。
        // 动画只由面板视图和最小化状态变化触发。
        let sftp_content_epoch = (match self.sftp_panel.view {
            SftpPanelView::Files => 0,
            SftpPanelView::Commands => 1,
        } as u64)
            .wrapping_add(self.sftp_panel.minimize_epoch);
        let toolbar_visibility = self.config.sftp_toolbar_visibility();
        let view = cx.entity();

        let header = h_flex()
            .flex_none()
            .h(px(32.))
            .px_2()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tab_bar)
            .child(
                Button::new("sftp-view-files")
                    .ghost()
                    .small()
                    .selected(self.sftp_panel.view == SftpPanelView::Files)
                    .icon(IconName::FolderOpen)
                    .label(t!("remote_files").to_string())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_sftp_panel_view(SftpPanelView::Files, cx);
                    })),
            )
            .child(
                Button::new("sftp-view-commands")
                    .ghost()
                    .small()
                    .selected(self.sftp_panel.view == SftpPanelView::Commands)
                    .icon(IconName::SquareTerminal)
                    .label(t!("quick_commands").to_string())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_sftp_panel_view(SftpPanelView::Commands, cx);
                    })),
            )
            .child(div().flex_1())
            .when_some(
                (self.sftp_panel.view == SftpPanelView::Files)
                    .then_some(active_sftp)
                    .flatten(),
                |this, sftp| {
                    let selected_count = sftp.selected_entries.len();
                    this.when(toolbar_visibility.sync_cwd, |this| {
                        this.child(
                            Checkbox::new("sftp-sync-cwd")
                                .small()
                                .label(t!("sync_cwd").to_string())
                                .checked(sftp.follow_terminal_cwd)
                                .tab_stop(false)
                                .on_click(cx.listener(|this, checked, _, cx| {
                                    this.set_follow_terminal_cwd(*checked, cx);
                                })),
                        )
                    })
                    .when(toolbar_visibility.hidden_files, |this| {
                        this.child(
                            Checkbox::new("sftp-show-hidden")
                                .small()
                                .label(t!("hidden").to_string())
                                .checked(self.sftp_panel.show_hidden_files)
                                .tab_stop(false)
                                .on_click(cx.listener(|this, checked, _, cx| {
                                    if this.sftp_panel.show_hidden_files == *checked {
                                        return;
                                    }
                                    this.sftp_panel.show_hidden_files = *checked;
                                    this.config.set_show_hidden_files(*checked);
                                    this.mark_config_preferences_dirty();
                                    cx.notify();
                                })),
                        )
                    })
                    .when(toolbar_visibility.refresh, |this| {
                        this.child(
                            Button::new("sftp-refresh")
                                .ghost()
                                .small()
                                .icon(IconName::ArrowRight)
                                .label(t!("refresh").to_string())
                                .on_click(cx.listener(|this, _, _, cx| this.refresh_sftp(cx))),
                        )
                    })
                    .when(toolbar_visibility.new_folder, |this| {
                        this.child(
                            Button::new("sftp-new-folder")
                                .ghost()
                                .small()
                                .icon(IconName::Folder)
                                .label(t!("new_folder").to_string())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.sftp_workspace.creating_folder = true;
                                    this.sftp_workspace
                                        .new_folder_input
                                        .update(cx, |input, cx| {
                                            input.set_value("", window, cx);
                                        });
                                    crate::app::input_focus::defer_focus_input_at_end(
                                        this.sftp_workspace.new_folder_input.clone(),
                                        window,
                                        cx,
                                    );
                                    cx.notify();
                                })),
                        )
                    })
                    .when(toolbar_visibility.delete, |this| {
                        this.child(
                            Button::new("sftp-delete-selected")
                                .ghost()
                                .small()
                                .icon(IconName::Close)
                                .label(if selected_count == 0 {
                                    t!("delete_selected").to_string()
                                } else {
                                    format!("{} ({selected_count})", t!("delete_selected"))
                                })
                                .disabled(selected_count == 0)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.show_delete_confirm_dialog(window, cx);
                                })),
                        )
                    })
                    .when(toolbar_visibility.upload_file, |this| {
                        this.child(
                            Button::new("sftp-upload-file")
                                .ghost()
                                .small()
                                .icon(IconName::Plus)
                                .label(t!("upload_file").to_string())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.upload_sftp_files(window, cx)
                                })),
                        )
                    })
                    .when(toolbar_visibility.upload_folder, |this| {
                        this.child(
                            Button::new("sftp-upload-folder")
                                .ghost()
                                .small()
                                .icon(IconName::Folder)
                                .label(t!("upload_folder").to_string())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.upload_sftp_folder(window, cx)
                                })),
                        )
                    })
                    .when(toolbar_visibility.download, |this| {
                        this.child(
                            Button::new("sftp-download-selected")
                                .ghost()
                                .small()
                                .icon(IconName::ArrowDown)
                                .label(if selected_count == 0 {
                                    t!("download").to_string()
                                } else {
                                    t!("download_count", count = selected_count).to_string()
                                })
                                .disabled(selected_count == 0)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.download_selected_sftp_entries(window, cx);
                                })),
                        )
                    })
                },
            )
            .context_menu({
                let view = view.clone();
                move |menu, window, cx| {
                    Self::build_sftp_toolbar_visibility_menu(menu, view.clone(), window, cx)
                }
            });

        let Some(sftp) = active_sftp else {
            let outer = v_flex()
                .gap_0()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .flex_1()
                .min_h(px(0.))
                .relative()
                .overflow_hidden()
                .child(
                    v_flex()
                        .flex_1()
                        .min_h(px(0.))
                        .when(presentation.sftp_minimized, |this| this.hidden())
                        .child(header)
                        .child(
                            v_flex()
                                .flex_1()
                                .items_center()
                                .justify_center()
                                .p_3()
                                .child(
                                    div()
                                        .text_size(rems(1.0))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t!("open_ssh_tab_sftp")),
                                ),
                        ),
                );
            return outer
                .with_animation(
                    ElementId::NamedInteger("sftp-content-fade".into(), sftp_content_epoch),
                    Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                    |this, delta| this.opacity(delta * delta),
                )
                .into_any_element();
        };

        if self.sftp_panel.view == SftpPanelView::Commands {
            return v_flex()
                .gap_0()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .flex_1()
                .min_h(px(0.))
                .relative()
                .overflow_hidden()
                .child(
                    v_flex()
                        .flex_1()
                        .min_h(px(0.))
                        .when(presentation.sftp_minimized, |this| this.hidden())
                        .child(header)
                        .child(self.render_quick_commands(window, cx)),
                )
                .with_animation(
                    ElementId::NamedInteger("sftp-content-fade".into(), sftp_content_epoch),
                    Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                    |this, delta| this.opacity(delta * delta),
                )
                .into_any_element();
        }

        let entries_snapshot =
            SftpEntriesRenderSnapshot::new(sftp, self.sftp_panel.show_hidden_files);
        let entries = entries_snapshot.entries;
        let selected_entries = entries_snapshot.selected_entries;
        let selected_path = entries_snapshot.selected_path;
        let all_selected = entries_snapshot.all_selected;
        let parent_path = Self::sftp_parent_path(&sftp.current_path);
        let view = cx.entity();
        let icon_col_width = px(14.);
        let size_col_width = px(96.);
        let size_col_min_width = px(56.);
        let modified_col_width = px(152.);
        let modified_col_min_width = px(112.);
        let name_col_min_width = px(56.);

        let mut outer = v_flex()
            .gap_0()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .flex_1()
            .min_h(px(0.))
            .relative()
            .overflow_hidden()
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, window, cx| {
                    let paths_to_upload: Vec<String> = paths
                        .0
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect();
                    this.upload_sftp_files_batch(paths_to_upload, window, cx);
                }),
            );

        outer = outer.child(
            v_flex()
                .flex_1()
                .min_h(px(0.))
                .when(presentation.sftp_minimized, |this| this.hidden())
                .child(header)
                .child(
                    h_flex()
                        .h(px(32.))
                        .items_center()
                        .gap_1()
                        .px_2()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().background)
                        .child(
                            Button::new("sftp-up")
                                .ghost()
                                .small()
                                .icon(IconName::ChevronUp)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.navigate_sftp(parent_path.clone(), cx);
                                })),
                        )
                        .child(
                            Input::new(&self.sftp_workspace.path_input)
                                .flex_1()
                                .tab_index(0),
                        )
                        .child(div().flex_none()),
                )
                .child(
                    h_resizable("sftp-files-split")
                        .with_state(&self.sftp_workspace.file_panels)
                        .child(
                            resizable_panel()
                                .size(px(236.))
                                .size_range(px(120.)..Pixels::MAX)
                                .child(self.render_sftp_directory_tree(sftp, cx)),
                        )
                        .child(
                            resizable_panel().size_range(px(320.)..Pixels::MAX).child(
                                v_flex()
                                    .w_full()
                                    .h_full()
                                    .min_w(px(0.))
                                    .min_h(px(0.))
                                    .child(
                                        h_flex()
                                            .h(px(26.))
                                            .px_3()
                                            .items_center()
                                            .gap_2()
                                            .border_b_1()
                                            .border_color(cx.theme().border)
                                            .bg(cx.theme().muted.opacity(0.8))
                                            .child(
                                                h_flex()
                                                    .w(px(24.))
                                                    .flex_none()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(
                                                        Checkbox::new("sftp-select-all")
                                                            .checked(all_selected)
                                                            .on_click(cx.listener(
                                                                move |this, checked, _, cx| {
                                                                    this.toggle_all_sftp_entries(
                                                                        *checked, cx,
                                                                    );
                                                                },
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .flex_1()
                                                    .min_w(name_col_min_width)
                                                    .items_center()
                                                    .gap_2()
                                                    .child(div().w(icon_col_width).flex_none())
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .text_size(rems(0.917))
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(t!("name")),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .w(size_col_width)
                                                    .min_w(size_col_min_width)
                                                    .flex_shrink_1()
                                                    .text_size(rems(0.917))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t!("size")),
                                            )
                                            .child(
                                                div()
                                                    .w(modified_col_width)
                                                    .min_w(modified_col_min_width)
                                                    .flex_shrink_1()
                                                    .text_size(rems(0.917))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t!("modified")),
                                            )
                                            .child(div().w(px(12.)).flex_none()),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .flex_1()
                                            .relative()
                                            .min_h(px(0.))
                                            .on_mouse_down(
                                                MouseButton::Right,
                                                cx.listener(
                                                    |this, event: &MouseDownEvent, _, cx| {
                                                        let target_was_set_by_row = this
                                                            .sftp_workspace
                                                            .context_menu
                                                            .as_ref()
                                                            .is_some_and(|menu| {
                                                                menu.position == event.position
                                                            });
                                                        if !target_was_set_by_row {
                                                            this.open_sftp_context_menu(
                                                                None,
                                                                false,
                                                                None,
                                                                event.position,
                                                                cx,
                                                            );
                                                        }
                                                    },
                                                ),
                                            )
                                            .child(if entries.is_empty() {
                                                v_flex()
                                                    .size_full()
                                                    .items_center()
                                                    .justify_center()
                                                    .gap_3()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        Icon::new(IconName::FolderOpen)
                                                            .with_size(Size::Large),
                                                    )
                                                    .child(t!("sftp_directory_empty"))
                                                    .into_any_element()
                                            } else {
                                                let view = view.clone();
                                                let theme = cx.theme().clone();
                                                uniform_list(
                                                    "sftp-entries-list",
                                                    entries.len(),
                                                    move |range, window, _cx| {
                                                        range
                                                            .into_iter()
                                                            .filter_map(|ix| {
                                                                let entry = entries.get(ix)?;
                                                                let is_checked = selected_entries
                                                                    .contains(&entry.full_path);
                                                                let is_selected = selected_path
                                                                    .as_deref()
                                                                    == Some(
                                                                        entry.full_path.as_str(),
                                                                    );
                                                                let name_color = if entry.is_dir {
                                                                    theme.primary
                                                                } else {
                                                                    theme.foreground
                                                                };
                                                                let bg = if is_selected {
                                                                    theme.secondary
                                                                } else if ix % 2 == 0 {
                                                                    theme.background
                                                                } else {
                                                                    theme.muted.opacity(0.5)
                                                                };
                                                                Some(
                                            h_flex()
                                                .w_full()
                                                .h(px(28.))
                                                .items_center()
                                                .px_3()
                                                .gap_2()
                                                .bg(bg)
                                                .hover(|style| style.bg(theme.muted.opacity(0.8)))
                                                .border_b_1()
                                                .border_color(theme.border.opacity(0.35))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    window.listener_for(&view, {
                                                        let entries = Arc::clone(&entries);
                                                        move |this, event: &MouseDownEvent, _, cx| {
                                                            let Some(entry) = entries.get(ix) else {
                                                                return;
                                                            };
                                                            this.dismiss_sftp_context_menu(cx);
                                                            this.select_sftp_entry(
                                                                (*entry).clone(),
                                                                cx,
                                                            );
                                                            if event.click_count >= 2 {
                                                                if entry.is_dir {
                                                                    this.navigate_sftp(
                                                                        entry.full_path.clone(),
                                                                        cx,
                                                                    );
                                                                } else if is_editable_text_file(
                                                                    &entry.full_path,
                                                                ) {
                                                                    this.open_file_in_editor(
                                                                        entry.full_path.clone(),
                                                                        cx,
                                                                    );
                                                                } else if let Some(handle) =
                                                                    this.active_sftp_handle()
                                                                {
                                                                    handle.edit_file(
                                                                        entry.full_path.clone(),
                                                                    );
                                                                }
                                                            }
                                                        }
                                                    }),
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Right,
                                                    window.listener_for(&view, {
                                                        let entries = Arc::clone(&entries);
                                                        move |this, event: &MouseDownEvent, _, cx| {
                                                            let Some(entry) = entries.get(ix) else {
                                                                return;
                                                            };
                                                            this.mark_sftp_entry_selected(
                                                                &entry.full_path,
                                                                cx,
                                                            );
                                                            this.open_sftp_context_menu(
                                                                Some(entry.full_path.clone()),
                                                                entry.is_dir,
                                                                Some(entry.permissions),
                                                                event.position,
                                                                cx,
                                                            );
                                                        }
                                                    }),
                                                )
                                                .child(
                                                    h_flex()
                                                        .w(px(24.))
                                                        .flex_none()
                                                        .items_center()
                                                        .justify_center()
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            |_, _, cx| cx.stop_propagation(),
                                                        )
                                                        .child(
                                                            Checkbox::new(ElementId::Name(
                                                                format!(
                                                                    "check-{}",
                                                                    entry.full_path
                                                                )
                                                                .into(),
                                                            ))
                                                            .checked(is_checked)
                                                            .on_click(window.listener_for(&view, {
                                                                let entries = Arc::clone(&entries);
                                                                move |this, checked, _, cx| {
                                                                    if let Some(entry) =
                                                                        entries.get(ix)
                                                                    {
                                                                        this.toggle_sftp_entry(
                                                                            entry.full_path.clone(),
                                                                            *checked,
                                                                            cx,
                                                                        );
                                                                    }
                                                                }
                                                            })),
                                                        ),
                                                )
                                                .child(
                                                    h_flex()
                                                        .flex_1()
                                                        .min_w(name_col_min_width)
                                                        .items_center()
                                                        .gap_2()
                                                        .child(
                                                            div()
                                                                .w(icon_col_width)
                                                                .flex_none()
                                                                .text_size(rems(1.0))
                                                                .text_color(name_color)
                                                                .child(if entry.is_dir {
                                                                    "📁"
                                                                } else {
                                                                    "📄"
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_w(px(0.))
                                                                .truncate()
                                                                .text_size(rems(1.0))
                                                                .text_color(name_color)
                                                                .child(entry.name.clone()),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .w(size_col_width)
                                                        .min_w(size_col_min_width)
                                                        .flex_shrink_1()
                                                        .text_size(rems(0.917))
                                                        .text_color(theme.muted_foreground)
                                                        .child(if entry.is_dir {
                                                            "-".to_string()
                                                        } else {
                                                            format_bytes(entry.size)
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .w(modified_col_width)
                                                        .min_w(modified_col_min_width)
                                                        .flex_shrink_1()
                                                        .text_size(rems(0.917))
                                                        .text_color(theme.muted_foreground)
                                                        .child(format_mtime(entry.modified)),
                                                )
                                                .child(div().w(px(12.)).flex_none())
                                                .into_any_element(),
                                        )
                                                            })
                                                            .collect::<Vec<_>>()
                                                    },
                                                )
                                                .size_full()
                                                .track_scroll(
                                                    &self.sftp_workspace.remote_files_scroll_handle,
                                                )
                                                .into_any_element()
                                            })
                                            .child(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .right_0()
                                                    .bottom_0()
                                                    .w(px(16.))
                                                    .child(
                                                        Scrollbar::vertical(
                                                            &self
                                                                .sftp_workspace
                                                                .remote_files_scroll_handle,
                                                        )
                                                        .scrollbar_show(ScrollbarShow::Always),
                                                    ),
                                            )
                                            .context_menu({
                                                let view = view.clone();
                                                move |menu, window, cx| {
                                                    Self::build_sftp_context_menu(
                                                        menu,
                                                        view.clone(),
                                                        window,
                                                        cx,
                                                    )
                                                }
                                            }),
                                    ),
                            ),
                        ),
                ),
        );
        outer
            .with_animation(
                ElementId::NamedInteger("sftp-content-fade".into(), sftp_content_epoch),
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                |this, delta| this.opacity(delta * delta),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod sftp_tree_tests {
    use super::*;

    fn directory(name: &str, full_path: &str) -> crate::sftp::RemoteEntry {
        crate::sftp::RemoteEntry {
            name: name.into(),
            full_path: full_path.into(),
            is_dir: true,
            size: 0,
            modified: 0,
            permissions: 0o755,
        }
    }

    fn file(name: &str, full_path: &str) -> crate::sftp::RemoteEntry {
        crate::sftp::RemoteEntry {
            name: name.into(),
            full_path: full_path.into(),
            is_dir: false,
            size: 0,
            modified: 0,
            permissions: 0o644,
        }
    }

    fn tree_state() -> terminal::SftpUiState {
        terminal::SftpUiState {
            current_path: "/".into(),
            status: String::new(),
            entries: Vec::new(),
            directory_entries: HashMap::new(),
            expanded_directories: HashSet::new(),
            selected_path: None,
            selected_entries: HashSet::new(),
            home_dir: "/".into(),
            follow_terminal_cwd: false,
            initial_terminal_cwd_synced: false,
            latency_ms: None,
        }
    }

    fn row_metadata(rows: &[SftpTreeRenderRow<'_>]) -> Vec<(String, usize, u64, bool)> {
        rows.iter()
            .map(|row| {
                (
                    row.path.to_string(),
                    row.depth,
                    row.ancestor_continuation_mask,
                    row.is_last_sibling,
                )
            })
            .collect()
    }

    fn vertical_bounds(top: f32, height: f32) -> gpui::Bounds<Pixels> {
        gpui::Bounds::new(point(px(0.), px(top)), gpui::size(px(100.), px(height)))
    }

    #[test]
    fn visible_tree_target_keeps_the_current_scroll_position() {
        let viewport = vertical_bounds(100., 200.);
        let target = vertical_bounds(150., 30.);

        assert_eq!(
            crate::sftp::ops::minimal_sftp_tree_scroll_offset_y(
                px(-300.),
                viewport,
                target,
                px(0.),
            ),
            px(-300.)
        );
    }

    #[test]
    fn offscreen_tree_target_moves_only_to_the_nearest_viewport_edge() {
        let viewport = vertical_bounds(100., 200.);

        assert_eq!(
            crate::sftp::ops::minimal_sftp_tree_scroll_offset_y(
                px(-300.),
                viewport,
                vertical_bounds(70., 30.),
                px(0.),
            ),
            px(-270.)
        );
        assert_eq!(
            crate::sftp::ops::minimal_sftp_tree_scroll_offset_y(
                px(-300.),
                viewport,
                vertical_bounds(300., 30.),
                px(0.),
            ),
            px(-330.)
        );
    }

    #[test]
    fn partially_visible_tree_target_does_not_jump() {
        let viewport = vertical_bounds(100., 200.);

        assert_eq!(
            crate::sftp::ops::minimal_sftp_tree_scroll_offset_y(
                px(-300.),
                viewport,
                vertical_bounds(90., 30.),
                px(0.),
            ),
            px(-300.)
        );
        assert_eq!(
            crate::sftp::ops::minimal_sftp_tree_scroll_offset_y(
                px(-300.),
                viewport,
                vertical_bounds(290., 30.),
                px(0.),
            ),
            px(-300.)
        );
    }

    #[test]
    fn scrollbar_inset_only_affects_a_fully_offscreen_target() {
        let viewport = vertical_bounds(100., 200.);

        assert_eq!(
            crate::sftp::ops::minimal_sftp_tree_scroll_offset_y(
                px(-300.),
                viewport,
                vertical_bounds(295., 30.),
                px(16.),
            ),
            px(-300.)
        );
        assert_eq!(
            crate::sftp::ops::minimal_sftp_tree_scroll_offset_y(
                px(-300.),
                viewport,
                vertical_bounds(300., 30.),
                px(16.),
            ),
            px(-346.)
        );
    }

    #[test]
    fn tree_rows_mark_siblings_and_nested_branch_continuations() {
        let mut sftp = tree_state();
        sftp.directory_entries.insert(
            "/".into(),
            vec![directory("alpha", "/alpha"), directory("beta", "/beta")],
        );
        sftp.directory_entries.insert(
            "/alpha".into(),
            vec![
                directory("first", "/alpha/first"),
                directory("last", "/alpha/last"),
            ],
        );
        sftp.directory_entries.insert(
            "/alpha/first".into(),
            vec![directory("leaf", "/alpha/first/leaf")],
        );
        sftp.expanded_directories.insert("/alpha".into());
        sftp.expanded_directories.insert("/alpha/first".into());

        let rows = sftp_tree_render_rows(&sftp, false);

        assert_eq!(
            row_metadata(&rows),
            vec![
                ("/".into(), 0, 0, true),
                ("/alpha".into(), 1, 0, false),
                ("/alpha/first".into(), 2, 1, false),
                ("/alpha/first/leaf".into(), 3, 3, true),
                ("/alpha/last".into(), 2, 1, true),
                ("/beta".into(), 1, 0, true),
            ]
        );
    }

    #[test]
    fn tree_rows_respect_collapsed_nodes_and_hidden_directory_filtering() {
        let mut sftp = tree_state();
        sftp.directory_entries.insert(
            "/".into(),
            vec![
                directory("visible", "/visible"),
                directory(".hidden", "/.hidden"),
                file("plain.txt", "/plain.txt"),
            ],
        );
        sftp.directory_entries.insert(
            "/visible".into(),
            vec![directory("nested", "/visible/nested")],
        );

        let hidden_rows = sftp_tree_render_rows(&sftp, false);
        assert_eq!(
            row_metadata(&hidden_rows),
            vec![("/".into(), 0, 0, true), ("/visible".into(), 1, 0, true),]
        );

        let all_rows = sftp_tree_render_rows(&sftp, true);
        assert_eq!(
            row_metadata(&all_rows),
            vec![
                ("/".into(), 0, 0, true),
                ("/visible".into(), 1, 0, false),
                ("/.hidden".into(), 1, 0, true),
            ]
        );
    }

    #[test]
    fn tree_rows_keep_branch_mask_within_depth_guard() {
        let mut sftp = tree_state();
        sftp.directory_entries.insert(
            "/".into(),
            vec![directory("branch", "/branch"), directory("tail", "/tail")],
        );
        sftp.expanded_directories.insert("/branch".into());

        let mut parent = "/branch".to_string();
        let mut chain_at_depth_32 = String::new();
        let mut tail_at_depth_32 = String::new();
        for depth in 2..=33 {
            let child = format!("{parent}/level-{depth}");
            let tail = format!("{parent}/tail-{depth}");
            sftp.directory_entries.insert(
                parent.clone(),
                vec![
                    directory(&format!("level-{depth}"), &child),
                    directory(&format!("tail-{depth}"), &tail),
                ],
            );
            sftp.expanded_directories.insert(child.clone());
            if depth == 32 {
                chain_at_depth_32.clone_from(&child);
                tail_at_depth_32 = tail;
            }
            parent = child;
        }

        let rows = sftp_tree_render_rows(&sftp, false);
        let deepest_chain = rows
            .iter()
            .find(|row| row.path == chain_at_depth_32)
            .unwrap();
        let deepest_tail = rows
            .iter()
            .find(|row| row.path == tail_at_depth_32)
            .unwrap();

        assert!(rows.iter().all(|row| row.depth <= SFTP_TREE_MAX_DEPTH));
        assert_eq!(deepest_chain.depth, SFTP_TREE_MAX_DEPTH);
        assert_eq!(deepest_chain.ancestor_continuation_mask, 0x7fff_ffff);
        assert!(!deepest_chain.is_last_sibling);
        assert_eq!(deepest_tail.depth, SFTP_TREE_MAX_DEPTH);
        assert_eq!(deepest_tail.ancestor_continuation_mask, 0x7fff_ffff);
        assert!(deepest_tail.is_last_sibling);
        assert!(!rows.iter().any(|row| row.path.ends_with("level-33")));
        assert_eq!(rows.last().map(|row| row.path), Some("/tail"));
    }
}
