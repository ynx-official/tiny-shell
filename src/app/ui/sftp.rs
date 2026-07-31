use super::*;

impl TinyShell {
    pub(crate) fn toggle_clean_mode(&mut self, cx: &mut Context<Self>) {
        self.workspace_mode.toggle_clean();
        cx.notify();
    }

    pub(crate) fn toggle_sftp_minimized(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.body_panels.clone();
        let presentation = self.workspace_mode.presentation(self.sftp_panel_minimized);
        let minimized = presentation.sftp_minimized;
        self.sftp_minimize_epoch = self.sftp_minimize_epoch.wrapping_add(1);

        if !minimized {
            let sizes = state.read(cx).sizes();
            if sizes.len() > 1 {
                self.prev_monitoring_size = Some(sizes[1]);
            }
        } else {
            let prev_size = self.prev_monitoring_size.unwrap_or(px(328.));

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
            self.sftp_panel_minimized = !minimized;
            self.config
                .set_sftp_panel_minimized(self.sftp_panel_minimized);
            self.mark_config_preferences_dirty();
        }
        cx.notify();
    }

    pub(super) fn render_sftp_tree_row(
        &self,
        row: crate::sftp::ops::SftpTreeRow,
        current_path: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = row.path.clone();
        let toggle_path = path.clone();
        let context_path = path.clone();
        let context_permissions = row.permissions;
        let view = cx.entity();
        let is_current = current_path == path;
        let theme = cx.theme().clone();
        let folder_icon = if row.expanded {
            IconName::FolderOpen
        } else {
            IconName::Folder
        };
        let tree_toggle = if path == "/" {
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
                .text_size(rems(0.78))
                .text_color(theme.muted_foreground)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.toggle_sftp_tree_directory(toggle_path.clone(), cx);
                        cx.stop_propagation();
                    }),
                )
                .child(if row.expanded { "▾" } else { "▸" })
                .into_any_element()
        };

        h_flex()
            .id(format!("sftp-tree-row-{context_path}"))
            .w_full()
            .h(px(30.))
            .pl(px(5. + row.depth as f32 * 15.))
            .pr_2()
            .items_center()
            .gap(px(5.))
            .rounded_sm()
            .cursor_pointer()
            .bg(if is_current {
                theme.secondary
            } else {
                theme.background.opacity(0.)
            })
            .hover(|style| style.bg(theme.muted.opacity(0.85)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.select_sftp_tree_directory(path.clone(), cx);
                }),
            )
            .child(tree_toggle)
            .child(
                Icon::new(folder_icon)
                    .with_size(Size::Small)
                    .text_color(if is_current {
                        theme.primary
                    } else {
                        theme.muted_foreground
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_size(rems(0.92))
                    .text_color(if is_current {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .when(is_current, |style| style.font_weight(FontWeight::MEDIUM))
                    .child(row.name),
            )
            .context_menu(move |menu, window, cx| {
                Self::build_sftp_tree_context_menu(
                    menu,
                    view.clone(),
                    context_path.clone(),
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
        let rows = crate::sftp::ops::sftp_tree_rows(sftp, self.show_hidden_files)
            .into_iter()
            .map(|row| self.render_sftp_tree_row(row, &sftp.current_path, cx))
            .collect::<Vec<_>>();
        let empty_context_path = sftp.current_path.clone();
        let view = cx.entity();

        v_flex()
            .w(px(236.))
            .h_full()
            .flex_none()
            .min_h(px(0.))
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h(px(28.))
                    .px_2()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .text_size(rems(0.85))
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::FolderOpen).with_size(Size::Small))
                    .child(t!("remote_files")),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        v_flex()
                            .id("sftp-directory-tree")
                            .size_full()
                            .track_scroll(&self.sftp_tree_scroll_handle)
                            .overflow_y_scroll()
                            .p_1()
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
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .w(px(8.))
                            .child(
                                Scrollbar::vertical(&self.sftp_tree_scroll_handle)
                                    .scrollbar_show(ScrollbarShow::Scrolling),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn set_sftp_panel_view(&mut self, view: SftpPanelView, cx: &mut Context<Self>) {
        if self.sftp_panel_view == view {
            return;
        }
        self.sftp_panel_view = view;
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
        let webdav_enabled = self.config.sync_enabled() && self.config.sync_backend() == "webdav";
        let sync_failed = self
            .sync_runtime
            .status
            .starts_with(t!("sync_failed").as_ref());
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
        // 目录内容更新不应触发整个面板淡入，否则每次进入子目录都会闪烁。
        // 动画只由面板视图和最小化状态变化触发。
        let sftp_content_epoch = (match self.sftp_panel_view {
            SftpPanelView::Files => 0,
            SftpPanelView::Commands => 1,
        } as u64)
            .wrapping_add(self.sftp_minimize_epoch);
        let toolbar_visibility = self.config.sftp_toolbar_visibility();
        let view = cx.entity();

        let header = h_flex()
            .flex_none()
            .h(px(34.))
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
                    .selected(self.sftp_panel_view == SftpPanelView::Files)
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
                    .selected(self.sftp_panel_view == SftpPanelView::Commands)
                    .icon(IconName::SquareTerminal)
                    .label(t!("quick_commands").to_string())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_sftp_panel_view(SftpPanelView::Commands, cx);
                    })),
            )
            .child(div().flex_1())
            .when_some(
                (self.sftp_panel_view == SftpPanelView::Files)
                    .then_some(active_sftp)
                    .flatten(),
                |this, sftp| {
                    let selected_entries = sftp.selected_entries.clone();
                    this.when(toolbar_visibility.sync_cwd, |this| {
                        this.child(
                            Checkbox::new("sftp-sync-cwd")
                                .small()
                                .label(t!("sync_cwd").to_string())
                                .checked(sftp.follow_terminal_cwd)
                                .tab_stop(false)
                                .on_click(cx.listener(|this, checked, window, cx| {
                                    if this
                                        .active_sftp()
                                        .is_some_and(|sftp| sftp.follow_terminal_cwd == *checked)
                                    {
                                        return;
                                    }
                                    this.toggle_follow_terminal_cwd(window, cx);
                                })),
                        )
                    })
                    .when(toolbar_visibility.hidden_files, |this| {
                        this.child(
                            Checkbox::new("sftp-show-hidden")
                                .small()
                                .label(t!("hidden").to_string())
                                .checked(self.show_hidden_files)
                                .tab_stop(false)
                                .on_click(cx.listener(|this, checked, _, cx| {
                                    if this.show_hidden_files == *checked {
                                        return;
                                    }
                                    this.show_hidden_files = *checked;
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
                                    this.sftp_creating_folder = true;
                                    this.sftp_new_folder_input.update(cx, |input, cx| {
                                        input.set_value("", window, cx);
                                    });
                                    crate::app::input_focus::defer_focus_input_at_end(
                                        this.sftp_new_folder_input.clone(),
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
                                .label(if selected_entries.is_empty() {
                                    t!("delete_selected").to_string()
                                } else {
                                    format!(
                                        "{} ({})",
                                        t!("delete_selected"),
                                        selected_entries.len()
                                    )
                                })
                                .disabled(selected_entries.is_empty())
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
                                .label(if selected_entries.is_empty() {
                                    t!("download").to_string()
                                } else {
                                    t!("download_count", count = selected_entries.len()).to_string()
                                })
                                .disabled(selected_entries.is_empty())
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
                        .when(self.sftp_panel_minimized, |this| this.hidden())
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

        if self.sftp_panel_view == SftpPanelView::Commands {
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
                        .when(self.sftp_panel_minimized, |this| this.hidden())
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

        let selected_path = sftp.selected_path.clone();
        let entries = sftp
            .entries
            .clone()
            .into_iter()
            .filter(|entry| self.show_hidden_files || !entry.name.starts_with('.'))
            .collect::<Vec<_>>();
        let selected_entries = sftp.selected_entries.clone();
        let all_selected = !entries.is_empty()
            && entries
                .iter()
                .all(|e| selected_entries.contains(&e.full_path));
        let parent_path = Self::sftp_parent_path(&sftp.current_path);
        let view = cx.entity();
        let icon_col_width = px(14.);
        let size_col_width = px(96.);
        let modified_col_width = px(152.);

        let mut outer = v_flex()
            .gap_0()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .flex_1()
            .min_h(px(0.))
            .relative()
            .overflow_hidden()
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, _window, cx| {
                    let paths_to_upload: Vec<String> = paths
                        .0
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect();
                    this.upload_sftp_files_batch(paths_to_upload, cx);
                }),
            );

        outer = outer.child(
            v_flex()
                .flex_1()
                .min_h(px(0.))
                .when(self.sftp_panel_minimized, |this| this.hidden())
                .child(header)
                .child(
                    h_flex()
                        .h(px(36.))
                        .items_center()
                        .gap_2()
                        .px_3()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().muted)
                        .child(
                            Button::new("sftp-up")
                                .ghost()
                                .small()
                                .icon(IconName::ChevronUp)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.navigate_sftp(parent_path.clone(), cx);
                                })),
                        )
                        .child(Input::new(&self.sftp_path_input).flex_1().tab_index(0))
                        .child(div().flex_none()),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_h(px(0.))
                        .child(self.render_sftp_directory_tree(sftp, cx))
                        .child(
                            v_flex()
                                .flex_1()
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
                                                .min_w(px(0.))
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
                                                .flex_none()
                                                .text_size(rems(0.917))
                                                .text_color(cx.theme().muted_foreground)
                                                .child(t!("size")),
                                        )
                                        .child(
                                            div()
                                                .w(modified_col_width)
                                                .flex_none()
                                                .text_size(rems(0.917))
                                                .text_color(cx.theme().muted_foreground)
                                                .child(t!("modified")),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .relative()
                                        .min_h(px(0.))
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                                let target_was_set_by_row =
                                                    this.sftp_context_menu.as_ref().is_some_and(
                                                        |menu| menu.position == event.position,
                                                    );
                                                if !target_was_set_by_row {
                                                    this.open_sftp_context_menu(
                                                        None,
                                                        false,
                                                        None,
                                                        event.position,
                                                        cx,
                                                    );
                                                }
                                            }),
                                        )
                                        .child({
                                            let entries = entries.clone();
                                            let selected_entries = selected_entries.clone();
                                            let selected_path = selected_path.clone();
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
                                                            let entry = entry.clone();
                                                            let is_checked = selected_entries
                                                                .contains(&entry.full_path);
                                                            let is_selected = selected_path
                                                                .as_deref()
                                                                == Some(entry.full_path.as_str());
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
                                                        let entry = entry.clone();
                                                        move |this, event: &MouseDownEvent, _, cx| {
                                                            this.dismiss_sftp_context_menu(cx);
                                                            this.select_sftp_entry(
                                                                entry.clone(),
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
                                                        let entry = entry.clone();
                                                        let remote_path = entry.full_path.clone();
                                                        move |this, event: &MouseDownEvent, _, cx| {
                                                            this.mark_sftp_entry_selected(
                                                                &entry.full_path,
                                                                cx,
                                                            );
                                                            this.open_sftp_context_menu(
                                                                Some(remote_path.clone()),
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
                                                                let path = entry.full_path.clone();
                                                                move |this, checked, _, cx| {
                                                                    this.toggle_sftp_entry(
                                                                        path.clone(),
                                                                        *checked,
                                                                        cx,
                                                                    );
                                                                }
                                                            })),
                                                        ),
                                                )
                                                .child(
                                                    h_flex()
                                                        .flex_1()
                                                        .min_w(px(0.))
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
                                                                .overflow_hidden()
                                                                .text_size(rems(1.0))
                                                                .text_color(name_color)
                                                                .child(entry.name),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .w(size_col_width)
                                                        .flex_none()
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
                                                        .flex_none()
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
                                            .track_scroll(&self.remote_files_scroll_handle)
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
                                                        &self.remote_files_scroll_handle,
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
        );
        outer
            .with_animation(
                ElementId::NamedInteger("sftp-content-fade".into(), sftp_content_epoch),
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                |this, delta| this.opacity(delta * delta),
            )
            .into_any_element()
    }
    pub(super) fn render_monitoring_panel(
        &mut self,
        viewport_width: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let cpu_color = cx.theme().chart_1;
        let mem_color = cx.theme().chart_2;
        let swap_color = cx.theme().chart_3;
        let net_color = cx.theme().chart_4;
        let disk_color = cx.theme().chart_5;
        let border_color = cx.theme().border;
        let muted_fg = cx.theme().muted_foreground;

        let cpu_pct = self.system.cpu_percent;
        // Dynamic CPU line color: green <30%, amber 30-80%, red >80%
        // NOTE: Hsla.h is normalized 0..=1 (not degrees)
        let cpu_path_color = {
            let pct = cpu_pct * 100.0;
            if pct < 30.0 {
                Hsla {
                    h: 120.0 / 360.0,
                    s: 0.65,
                    l: 0.45,
                    a: 1.0,
                }
            } else if pct < 80.0 {
                Hsla {
                    h: 45.0 / 360.0,
                    s: 0.8,
                    l: 0.55,
                    a: 1.0,
                }
            } else {
                Hsla {
                    h: 0.0,
                    s: 0.8,
                    l: 0.55,
                    a: 1.0,
                }
            }
        };
        // Network TX color: derived from net_color for visual distinction from RX
        let net_tx_color = if net_color.l > 0.5 {
            Hsla {
                l: net_color.l * 0.6,
                ..net_color
            }
        } else {
            Hsla {
                l: net_color.l * 1.5,
                ..net_color
            }
        };
        let mem_pct = self.system.mem_percent;
        let swap_pct = self.system.swap_percent;
        let mem_detail = self.system.mem_detail.clone();
        let swap_detail = self.system.swap_detail.clone();
        let net_rx = self.system.net_rx.clone();
        let net_tx = self.system.net_tx.clone();

        let (disk_used, disk_total) = self.system.disks.iter().fold((0u64, 0u64), |(u, t), d| {
            (u + (d.total_bytes - d.available_bytes), t + d.total_bytes)
        });
        let disk_pct = if disk_total > 0 {
            disk_used as f64 / disk_total as f64 * 100.0
        } else {
            0.0
        };

        let cpu_spark_data = self.cpu_history.clone();
        let net_rx_history = self.net_rx_history.clone();
        let net_tx_history = self.net_tx_history.clone();
        let disks = self.system.disks.clone();
        let card_min_w = px(110.);

        let show_net_card = viewport_width > px(750.);
        let show_disk_card = viewport_width > px(600.);

        // --- CPU card ---
        let cpu_card = v_flex()
            .min_w(card_min_w)
            .flex_1()
            .h_full()
            .px_1()
            .py_1()
            .gap_0p5()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .child(
                        div()
                            .text_size(rems(0.833))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cpu_color)
                            .child(t!("cpu").to_string()),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(rems(0.833))
                            .text_color(muted_fg)
                            .child(format!("{:.0}%", cpu_pct * 100.0)),
                    ),
            )
            .child(
                canvas(
                    move |bounds, _window, _cx| {
                        let n = cpu_spark_data.len();
                        if n < 2 {
                            return None;
                        }
                        let mut path = PathBuilder::stroke(px(1.5));
                        let w = bounds.size.width;
                        let h = bounds.size.height;
                        let max_val = cpu_spark_data
                            .iter()
                            .cloned()
                            .fold(0.0f32, f32::max)
                            .max(0.1);
                        for (i, &val) in cpu_spark_data.iter().enumerate() {
                            let x = bounds.origin.x + w * i as f32 / (n - 1).max(1) as f32;
                            let y = bounds.origin.y + h * (1.0 - val / max_val * 0.85);
                            let pt = point(x, y);
                            if i == 0 {
                                path.move_to(pt);
                            } else {
                                path.line_to(pt);
                            }
                        }
                        path.build().ok()
                    },
                    move |_bounds, path_opt, window, _cx| {
                        if let Some(path) = path_opt {
                            window.paint_path(path, cpu_path_color);
                        }
                    },
                )
                .flex_1()
                .w_full(),
            );

        // --- MEM card: mem + swap ---
        let mem_card = v_flex()
            .min_w(card_min_w)
            .flex_1()
            .h_full()
            .px_1()
            .py_1()
            .gap_0p5()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .child(
                        div()
                            .text_size(rems(0.833))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(mem_color)
                            .child(t!("mem").to_string()),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(rems(0.833))
                            .text_color(muted_fg)
                            .child(format!("{:.0}%", mem_pct * 100.0)),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_1()
                    .child(
                        Progress::new("mem-progress")
                            .value(mem_pct * 100.0)
                            .color(mem_color)
                            .with_size(px(5.))
                            .flex_1(),
                    )
                    .child(
                        div()
                            .text_size(rems(0.7))
                            .text_color(muted_fg)
                            .child(mem_detail),
                    ),
            )
            .when(self.system.total_swap > 0, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_1()
                        .child(
                            Progress::new("swap-progress")
                                .value(swap_pct * 100.0)
                                .color(swap_color)
                                .with_size(px(4.))
                                .flex_1(),
                        )
                        .child(
                            div()
                                .text_size(rems(0.7))
                                .text_color(muted_fg)
                                .child(swap_detail),
                        ),
                )
            });

        // --- NET card: rx/tx text + dual sparkline ---
        let net_card = if show_net_card {
            Some(
                v_flex()
                    .min_w(card_min_w)
                    .flex_1()
                    .h_full()
                    .px_1()
                    .py_1()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .child(
                                div()
                                    .text_size(rems(0.833))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(net_color)
                                    .child(t!("net").to_string()),
                            )
                            .child(div().flex_1())
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(rems(0.75))
                                            .text_color(net_color)
                                            .child(format!("↓{}", net_rx)),
                                    )
                                    .child(
                                        div()
                                            .text_size(rems(0.75))
                                            .text_color(net_tx_color)
                                            .child(format!("↑{}", net_tx)),
                                    ),
                            ),
                    )
                    .child(
                        canvas(
                            move |bounds, _window, _cx| {
                                let n_rx = net_rx_history.len();
                                let n_tx = net_tx_history.len();
                                if n_rx < 2 || n_tx < 2 {
                                    return None;
                                }
                                let all: Vec<f32> = net_rx_history
                                    .iter()
                                    .chain(net_tx_history.iter())
                                    .cloned()
                                    .collect();
                                let max_val = all.iter().cloned().fold(0.0f32, f32::max).max(1.0);
                                let w = bounds.size.width;
                                let h = bounds.size.height;
                                let mut paths = Vec::new();

                                let mut rx_path = PathBuilder::stroke(px(1.5));
                                for (i, &val) in net_rx_history.iter().enumerate() {
                                    let x =
                                        bounds.origin.x + w * i as f32 / (n_rx - 1).max(1) as f32;
                                    let y = bounds.origin.y + h * (1.0 - val / max_val * 0.85);
                                    let pt = point(x, y);
                                    if i == 0 {
                                        rx_path.move_to(pt);
                                    } else {
                                        rx_path.line_to(pt);
                                    }
                                }
                                if let Ok(path) = rx_path.build() {
                                    paths.push((path, net_color));
                                }

                                let mut tx_path = PathBuilder::stroke(px(1.0));
                                for (i, &val) in net_tx_history.iter().enumerate() {
                                    let x =
                                        bounds.origin.x + w * i as f32 / (n_tx - 1).max(1) as f32;
                                    let y = bounds.origin.y + h * (1.0 - val / max_val * 0.85);
                                    let pt = point(x, y);
                                    if i == 0 {
                                        tx_path.move_to(pt);
                                    } else {
                                        tx_path.line_to(pt);
                                    }
                                }
                                if let Ok(path) = tx_path.build() {
                                    paths.push((path, net_tx_color));
                                }

                                Some(paths)
                            },
                            move |_bounds, paths_opt, window, _cx| {
                                if let Some(paths) = paths_opt {
                                    for (path, color) in paths {
                                        window.paint_path(path, color);
                                    }
                                }
                            },
                        )
                        .flex_1()
                        .w_full(),
                    ),
            )
        } else {
            None
        };

        // --- DISK card ---
        let disk_card = if show_disk_card {
            Some(
                v_flex()
                    .min_w(card_min_w)
                    .flex_1()
                    .h_full()
                    .px_1()
                    .py_1()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .child(
                                div()
                                    .text_size(rems(0.833))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(disk_color)
                                    .child(t!("disk").to_string()),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(rems(0.833))
                                    .text_color(muted_fg)
                                    .child(format!("{:.0}%", disk_pct)),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h(px(0.))
                            .child(
                                v_flex()
                                    .id("disk-scroll")
                                    .track_scroll(&self.disk_scroll_handle)
                                    .overflow_y_scroll()
                                    .size_full()
                                    .children(disks.iter().map(|disk| {
                                        let pct = if disk.total_bytes > 0 {
                                            (disk.total_bytes - disk.available_bytes) as f64
                                                / disk.total_bytes as f64
                                                * 100.0
                                        } else {
                                            0.0
                                        };
                                        let mount_short = disk.mount.clone();
                                        let mount_id = format!("disk-{}", mount_short);
                                        h_flex()
                                            .w_full()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_size(rems(0.667))
                                                    .text_color(muted_fg)
                                                    .child(mount_short),
                                            )
                                            .child(
                                                Progress::new(mount_id)
                                                    .value(pct as f32)
                                                    .color(disk_color)
                                                    .with_size(px(4.))
                                                    .flex_1(),
                                            )
                                            .child(
                                                div()
                                                    .text_size(rems(0.667))
                                                    .text_color(muted_fg)
                                                    .child(format!("{:.0}%", pct)),
                                            )
                                    })),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right_0()
                                    .bottom_0()
                                    .w(px(8.))
                                    .child(
                                        Scrollbar::vertical(&self.disk_scroll_handle)
                                            .scrollbar_show(ScrollbarShow::Scrolling),
                                    ),
                            )
                            .into_any_element(),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        let mut panel = h_flex()
            .h(px(80.))
            .w_full()
            .flex_none()
            .px_3()
            .gap_3()
            .border_b_1()
            .border_color(border_color)
            .bg(cx.theme().muted);

        panel = panel.child(cpu_card);
        panel = panel.child(mem_card);
        if let Some(card) = net_card {
            panel = panel.child(card);
        }
        if let Some(card) = disk_card {
            panel = panel.child(card);
        }
        panel
    }
}
