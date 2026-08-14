use super::*;

const NATIVE_TAB_MIN_WIDTH: f32 = 96.;
const NATIVE_TAB_TITLE_MAX_WIDTH: f32 = 192.;

struct TabBarGroupData {
    id: String,
    drag_id: u64,
    ordinal: u64,
    title: String,
    pane_ids: Vec<String>,
    connected: bool,
    disconnected: bool,
    status_epoch: u64,
}

impl TinyShell {
    pub(super) fn render_window_controls(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_macos = cfg!(target_os = "macos");
        let is_fullscreen = window.is_fullscreen();

        let is_active = cx.active_window() == Some(window.window_handle());

        h_flex()
            .group("window-controls")
            .flex_none()
            .items_center()
            .px_3()
            .gap_2()
            .when(!is_macos || is_fullscreen, |this| {
                this.child(
                    h_flex()
                        .id("window-close")
                        .size(px(12.))
                        .rounded_full()
                        .bg(if is_active {
                            hsla(3.0 / 360.0, 1.0, 0.67, 1.0)
                        } else {
                            hsla(0.0, 0.0, 0.8, 1.0)
                        }) // Red or Inactive Grey
                        .group_hover("window-controls", |s| {
                            s.bg(hsla(3.0 / 360.0, 1.0, 0.67, 1.0))
                        })
                        .when(!is_macos, |this| {
                            this.window_control_area(gpui::WindowControlArea::Close)
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.request_main_window_close(window, cx);
                        }))
                        .hover(|s| s.bg(hsla(3.0 / 360.0, 1.0, 0.55, 1.0)))
                        .active(|s| s.bg(hsla(3.0 / 360.0, 1.0, 0.45, 1.0)))
                        .items_center()
                        .justify_center()
                        .child(
                            h_flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(7.))
                                .font_weight(FontWeight::BOLD)
                                .line_height(relative(1.0))
                                .text_color(hsla(3.0 / 360.0, 1.0, 0.15, 0.7))
                                .opacity(0.0)
                                .group_hover("window-controls", |s| s.opacity(1.0))
                                .child("✕"),
                        ),
                )
                .child(
                    h_flex()
                        .id("window-minimize")
                        .size(px(12.))
                        .rounded_full()
                        .bg(if is_active {
                            hsla(39.0 / 360.0, 1.0, 0.59, 1.0)
                        } else {
                            hsla(0.0, 0.0, 0.8, 1.0)
                        }) // Yellow or Inactive Grey
                        .group_hover("window-controls", |s| {
                            s.bg(hsla(39.0 / 360.0, 1.0, 0.59, 1.0))
                        })
                        .when(!is_macos, |this| {
                            this.window_control_area(gpui::WindowControlArea::Min)
                        })
                        .on_click(|_, window, _| window.minimize_window())
                        .hover(|s| s.bg(hsla(39.0 / 360.0, 1.0, 0.49, 1.0)))
                        .active(|s| s.bg(hsla(39.0 / 360.0, 1.0, 0.39, 1.0)))
                        .items_center()
                        .justify_center()
                        .child(
                            h_flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(7.))
                                .font_weight(FontWeight::BOLD)
                                .line_height(relative(1.0))
                                .text_color(hsla(39.0 / 360.0, 1.0, 0.15, 0.8))
                                .opacity(0.0)
                                .group_hover("window-controls", |s| s.opacity(1.0))
                                .child("−"),
                        ),
                )
                .child(
                    h_flex()
                        .id("window-maximize")
                        .size(px(12.))
                        .rounded_full()
                        .bg(if is_active {
                            hsla(127.0 / 360.0, 0.68, 0.47, 1.0)
                        } else {
                            hsla(0.0, 0.0, 0.8, 1.0)
                        }) // Green or Inactive Grey
                        .group_hover("window-controls", |s| {
                            s.bg(hsla(127.0 / 360.0, 0.68, 0.47, 1.0))
                        })
                        .when(!is_macos, |this| {
                            this.window_control_area(gpui::WindowControlArea::Max)
                        })
                        .on_click(|_, window, _| {
                            if window.is_fullscreen() {
                                window.toggle_fullscreen();
                            } else {
                                #[cfg(target_os = "macos")]
                                window.titlebar_double_click();
                                #[cfg(not(target_os = "macos"))]
                                window.zoom_window();
                            }
                        })
                        .hover(|s| s.bg(hsla(127.0 / 360.0, 0.68, 0.37, 1.0)))
                        .active(|s| s.bg(hsla(127.0 / 360.0, 0.68, 0.27, 1.0)))
                        .items_center()
                        .justify_center()
                        .child(
                            h_flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(7.))
                                .font_weight(FontWeight::BOLD)
                                .line_height(relative(1.0))
                                .text_color(hsla(127.0 / 360.0, 1.0, 0.15, 0.8))
                                .opacity(0.0)
                                .group_hover("window-controls", |s| s.opacity(1.0))
                                .child("+"),
                        ),
                )
            })
            .when(is_macos, |this| {
                this.when(!is_fullscreen, |this| this.w(px(80.)))
            })
    }

    pub(super) fn render_tab_bar(
        &self,
        source_window: gpui::AnyWindowHandle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = cx.entity();
        let active_tab_index = self.workspace().active_tab_id().and_then(|active_id| {
            self.workspace()
                .tabs()
                .iter()
                .position(|tab| tab.id == active_id)
        });
        let active_group_index = self.workspace().active_group_id().and_then(|gid| {
            self.workspace()
                .tab_groups()
                .iter()
                .position(|g| g.id == gid)
        });
        // Home is the default tab, but it is not kept open after the user
        // enters a terminal workspace. The trailing plus creates it again.
        let show_home_tab = self.home_page_open || self.workspace().active_tab_id().is_none();
        let home_page_selected = self.workspace().active_system_info_tab_id().is_none()
            && ((show_home_tab && self.home_page_open)
                || self.workspace().active_tab_id().is_none());
        let selected =
            if home_page_selected || self.workspace().active_system_info_tab_id().is_some() {
                usize::MAX
            } else {
                active_group_index.or(active_tab_index).unwrap_or(0)
            };
        let groups_data: Vec<TabBarGroupData> = self
            .workspace()
            .tab_groups()
            .iter()
            .map(|g| {
                let pane_ids: Vec<String> = g
                    .pane_root
                    .tab_ids()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                let first_tab = pane_ids
                    .first()
                    .and_then(|tab_id| self.workspace().terminal_tab(tab_id));
                let connected = first_tab.is_some_and(|tab| tab.connected);
                let disconnected = first_tab.is_some_and(|tab| tab.disconnected_reason.is_some());
                let status_epoch = first_tab
                    .map(|tab| {
                        tab.backend_generation.wrapping_mul(3)
                            + if disconnected {
                                2
                            } else if connected {
                                1
                            } else {
                                0
                            }
                    })
                    .unwrap_or(0);
                TabBarGroupData {
                    id: g.id.clone(),
                    drag_id: g.drag_id,
                    ordinal: g.ordinal,
                    title: g.title.clone(),
                    pane_ids,
                    connected,
                    disconnected,
                    status_epoch,
                }
            })
            .collect();
        let system_info_tabs_data: Vec<(String, String, String, Option<String>)> = self
            .workspace()
            .system_info_tabs()
            .iter()
            .map(|tab| {
                let group_id = self.workspace().group_id_for_tab(&tab.source_tab_id);
                (
                    tab.id.clone(),
                    tab.source_tab_id.clone(),
                    tab.title.clone(),
                    group_id,
                )
            })
            .collect();
        let is_integrated =
            self.active_title_bar_style == crate::session::config::TitleBarStyle::Integrated;
        let native_tab_title_max_width = px(NATIVE_TAB_TITLE_MAX_WIDTH);
        let selected_tab_color = Hsla::from(gpui::rgb(0x1586F5));
        let tab_selection_epoch = self.main_view_key();

        h_flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .items_center()
            .gap_2()
            .child(
                div()
                    .h_full()
                    .flex_none()
                    .px_1()
                    .flex()
                    .items_center()
                    .child(
                        Button::new("tab-quick-connections")
                            .ghost()
                            .small()
                            .rounded(px(6.))
                            .icon(IconName::FolderOpen)
                            .tooltip(t!("overview_connections").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_quick_connection_manager_dialog(window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .on_prepaint({
                        let view = view.clone();
                        move |bounds, _window, cx| {
                            view.update(cx, |this, _| {
                                this.tab_bar_bounds = Some(bounds);
                            });
                        }
                    })
                    .when(is_integrated, |this| {
                        this.window_control_area(gpui::WindowControlArea::Drag)
                    })
                    .overflow_x_hidden()
                    .child({
                        let home_tab = Tab::new()
                            .min_w(px(92.))
                            .when(home_page_selected, |this| {
                                this.prefix(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .right_0()
                                        .bottom_0()
                                        .rounded_tl(px(8.))
                                        .rounded_tr(px(8.))
                                        .bg(cx.theme().background)
                                        .child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .right_0()
                                                .h(px(3.))
                                                .bg(selected_tab_color)
                                                .with_animation(
                                                    ElementId::NamedInteger(
                                                        "tab-selection-indicator".into(),
                                                        tab_selection_epoch,
                                                    ),
                                                    Animation::new(Duration::from_millis(180))
                                                        .with_easing(ease_out_quint()),
                                                    |this, delta| this.opacity(delta * delta),
                                                ),
                                        ),
                                )
                            })
                            .child(
                                h_flex()
                                    .relative()
                                    .h_full()
                                    .items_center()
                                    .px_3()
                                    .when(home_page_selected, |this| {
                                        this.font_weight(FontWeight::BOLD)
                                    })
                                    .child(t!("new_tab")),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_active_system_info_tab(None);
                                this.home_page_open = true;
                                this.set_home_page(HomePage::Overview, cx);
                            }));
                        let plus_tab = Tab::new()
                            .min_w(px(40.))
                            .prefix(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top(px(8.))
                                    .bottom(px(8.))
                                    .w(px(1.))
                                    .bg(cx.theme().border.opacity(0.8)),
                            )
                            .child(
                                h_flex()
                                    .h_full()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::new(IconName::Plus).with_size(Size::Small)),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_active_system_info_tab(None);
                                this.home_page_open = true;
                                this.set_home_page(HomePage::Overview, cx);
                            }));
                        TabBar::new("tiny-shell-tab-bar")
                            .track_scroll(&self.tabs_scroll_handle)
                            .children(groups_data.iter().enumerate().map(
                                |(ix, group)| {
                                    let gid = group.id.clone();
                                    let pane_ids = &group.pane_ids;
                                    let ordinal = group.ordinal;
                                    let title = &group.title;
                                    let label = if pane_ids.len() > 1 {
                                        format!("{} {} ({})", ordinal, title, pane_ids.len())
                                    } else {
                                        format!("{} {}", ordinal, title)
                                    };
                                    let close_id = if self.workspace().active_group_id() == Some(gid.as_str()) {
                                        self.workspace()
                                            .active_tab_id()
                                            .map(str::to_owned)
                                            .unwrap_or_else(|| {
                                                pane_ids.first().cloned().unwrap_or_default()
                                            })
                                    } else {
                                        pane_ids.first().cloned().unwrap_or_default()
                                    };
                                    let tab_selected = ix == selected;
                                    let tab_multi_selected = self.tab_drag.is_selected(&gid);

                                    // Status is independent of selection: grey means the
                                    // backend is still connecting, green is ready, and red
                                    // means the connection has failed or disconnected.
                                    let dot_color = if group.disconnected {
                                        cx.theme().danger
                                    } else if group.connected {
                                        cx.theme().success
                                    } else {
                                        cx.theme().muted_foreground
                                    };
                                    let dot_epoch = group.status_epoch;
                                    let drag_gid = gid.clone();
                                    let drag_payload = IncomingTabDrag {
                                        drag_id: group.drag_id,
                                        source_window,
                                        source: view.clone(),
                                        group_id: gid.clone(),
                                    };
                                    let drag_preview_label = label.clone();
                                    let tooltip_label = label.clone();
                                    let context_gid = gid.clone();
                                    let bounds_gid = gid.clone();
                                    let bounds_view = view.clone();
                                    Tab::new()
                                        .on_prepaint(move |bounds, _window, cx| {
                                            bounds_view.update(cx, |this, _| {
                                                this.tab_group_bounds
                                                    .insert(bounds_gid.clone(), bounds);
                                            });
                                        })
                                        .min_w(px(112.))
                                        .when(!is_integrated, |this| {
                                            this.min_w(px(NATIVE_TAB_MIN_WIDTH))
                                        })
                                        .when(tab_selected, |this| {
                                            this.prefix(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .left_0()
                                                    .right_0()
                                                    .bottom_0()
                                                    .rounded_tl(px(8.))
                                                    .rounded_tr(px(8.))
                                                    .bg(cx.theme().background)
                                                    .child(
                                                        div()
                                                            .absolute()
                                                            .top_0()
                                                            .left_0()
                                                            .right_0()
                                                            .h(px(3.))
                                                            .bg(selected_tab_color)
                                                            .with_animation(
                                                                ElementId::NamedInteger(
                                                                    "tab-selection-indicator".into(),
                                                                    tab_selection_epoch,
                                                                ),
                                                                Animation::new(Duration::from_millis(180))
                                                                    .with_easing(ease_out_quint()),
                                                                |this, delta| this.opacity(delta * delta),
                                                            ),
                                                    ),
                                            )
                                        })
                                        .when(!tab_selected && ix > 0, |this| {
                                            this.prefix(
                                                div()
                                                    .absolute()
                                                    .left_0()
                                                    .top(px(8.))
                                                    .bottom(px(8.))
                                                    .w(px(1.))
                                                    .bg(cx.theme().border.opacity(0.8)),
                                            )
                                        })
                                        .child(
                                            h_flex()
                                                .id(("tab-native-drag", ix))
                                                .relative()
                                                .h_full()
                                                .items_center()
                                                .gap_2()
                                                .px_2()
                                                .when(!is_integrated, |this| {
                                                    this.gap_1().px_1()
                                                })
                                                .rounded_tl(px(8.))
                                                .rounded_tr(px(8.))
                                                .when(!tab_selected, |this| {
                                                    this.hover(|this| {
                                                        this.bg(cx.theme().secondary.opacity(0.55))
                                                    })
                                                })
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, event: &MouseDownEvent, _, _| {
                                                        let additive = event.modifiers.platform
                                                            || event.modifiers.control;
                                                        this.tab_drag.begin_with_selection(
                                                            drag_gid.clone(),
                                                            event.position,
                                                            additive,
                                                        );
                                                    }),
                                                )
                                                .when(tab_multi_selected && !tab_selected, |this| {
                                                    this.bg(cx.theme().primary.opacity(0.10))
                                                })
                                                .on_drag(
                                                    drag_payload,
                                                    move |_, offset, _, cx| {
                                                        crate::app::clear_tab_drag_hover();
                                                        cx.new(|_| TabDragPreview {
                                                            label: drag_preview_label.clone(),
                                                            offset,
                                                        })
                                                    },
                                                )
                                                .when(tab_selected, |this| {
                                                    this.font_weight(FontWeight::BOLD)
                                                })
                                                .child(
                                                    div()
                                                        .size(px(8.))
                                                        .flex_none()
                                                        .rounded_full()
                                                        .bg(dot_color)
                                                        .with_animation(
                                                                ElementId::NamedInteger(
                                                                    format!("tab-status-dot-{gid}").into(),
                                                                    dot_epoch,
                                                                ),
                                                            Animation::new(Duration::from_millis(160))
                                                                .with_easing(ease_out_quint()),
                                                            |this, delta| this.opacity(delta * delta),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .id(("native-tab-title", ix))
                                                        .min_w(px(0.))
                                                        .when(!is_integrated, move |this| {
                                                            this.flex_none()
                                                                .max_w(native_tab_title_max_width)
                                                                .truncate()
                                                                .tooltip(move |window, cx| {
                                                                    gpui_component::tooltip::Tooltip::new(
                                                                        tooltip_label.clone(),
                                                                    )
                                                                    .build(window, cx)
                                                                })
                                                        })
                                                        .child(label),
                                                )
                                                .context_menu({
                                                    let view = view.clone();
                                                    move |menu, window, cx| {
                                                        Self::build_tab_context_menu(
                                                            menu,
                                                            view.clone(),
                                                            context_gid.clone(),
                                                            window,
                                                            cx,
                                                        )
                                                    }
                                                }),
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.activate_group(gid.clone(), window, cx)
                                        }))
                                        .suffix(
                                            Button::new(("tab-close", ix))
                                                .ghost()
                                                .xsmall()
                                                .icon(IconName::Close)
                                                .mr(px(4.))
                                                .when(!is_integrated, |this| this.mr(px(0.)))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    |_, window, cx| {
                                                        window.prevent_default();
                                                        cx.stop_propagation();
                                                    },
                                                )
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        window.prevent_default();
                                                        cx.stop_propagation();
                                                        if !close_id.is_empty() {
                                                            this.close_tab(close_id.clone(), cx)
                                                        }
                                                    },
                                                )),
                                        )
                                },
                            )
                            .chain(system_info_tabs_data.iter().enumerate().map(
                                |(ix, (info_id, source_tab_id, title, group_id))| {
                                    let selected_info = self.workspace().active_system_info_tab_id() == Some(info_id.as_str());
                                    let click_info_id = info_id.clone();
                                    let click_source_id = source_tab_id.clone();
                                    let click_group_id = group_id.clone();
                                    let close_info_id = info_id.clone();
                                    Tab::new()
                                        .min_w(px(150.))
                                        .when(selected_info, |this| {
                                            this.prefix(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .left_0()
                                                    .right_0()
                                                    .bottom_0()
                                                    .rounded_tl(px(8.))
                                                    .rounded_tr(px(8.))
                                                    .bg(cx.theme().background)
                                                    .child(
                                                        div()
                                                            .absolute()
                                                            .top_0()
                                                            .left_0()
                                                            .right_0()
                                                            .h(px(3.))
                                                            .bg(selected_tab_color)
                                                            .with_animation(
                                                                ElementId::NamedInteger(
                                                                    "tab-selection-indicator".into(),
                                                                    tab_selection_epoch,
                                                                ),
                                                                Animation::new(Duration::from_millis(180))
                                                                    .with_easing(ease_out_quint()),
                                                                |this, delta| this.opacity(delta * delta),
                                                            ),
                                                    ),
                                            )
                                        })
                                        .prefix(
                                            div()
                                                .absolute()
                                                .left_0()
                                                .top(px(8.))
                                                .bottom(px(8.))
                                                .w(px(1.))
                                                .bg(cx.theme().border.opacity(0.8)),
                                        )
                                        .child(
                                            h_flex()
                                                .relative()
                                                .h_full()
                                                .items_center()
                                                .gap_2()
                                                .px_2()
                                                .when(!selected_info, |this| {
                                                    this.hover(|this| {
                                                        this.bg(cx.theme().secondary.opacity(0.55))
                                                    })
                                                })
                                                .when(selected_info, |this| {
                                                    this.font_weight(FontWeight::BOLD)
                                                })
                                                .child(Icon::new(IconName::Info).with_size(Size::Small))
                                                .child(
                                                    div()
                                                        .min_w(px(0.))
                                                        .overflow_hidden()
                                                        .whitespace_nowrap()
                                                        .text_ellipsis()
                                                        .child(title.clone()),
                                                ),
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            if let Some(group_id) = click_group_id.clone() {
                                                this.activate_group(group_id, window, cx);
                                            }
                                            this.window_state
                                                .workspace_state_mut()
                                                .activate_terminal_tab(&click_source_id);
                                            this.system_tab_id = Some(click_source_id.clone());
                                            this.set_active_system_info_tab(Some(click_info_id.clone()));
                                            this.home_page_open = false;
                                            this.request_active_system_snapshot();
                                            cx.notify();
                                        }))
                                        .suffix(
                                            Button::new(("system-info-tab-close", ix))
                                                .ghost()
                                                .xsmall()
                                                .icon(IconName::Close)
                                                .mr(px(4.))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    |_, window, cx| {
                                                        window.prevent_default();
                                                        cx.stop_propagation();
                                                    },
                                                )
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    window.prevent_default();
                                                    cx.stop_propagation();
                                                    this.close_system_info_tab(close_info_id.clone(), cx);
                                                })),
                                        )
                                },
                            ))
                            .chain(show_home_tab.then_some(home_tab))
                            .chain(std::iter::once(plus_tab)))
                            .last_empty_space(div().flex_1())
                            .w_full()
                            .h_full()
                    }),
            )
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .pr(px(6.))
                    .child(
                        Button::new("tab-bar-clean-mode")
                            .secondary()
                            .when(self.workspace_mode.presentation(self.sftp_panel.minimized).clean, |button| {
                                button.primary()
                            })
                            .small()
                            .rounded(px(999.))
                            .icon(IconName::SquareTerminal)
                            .tooltip(if self.workspace_mode.presentation(self.sftp_panel.minimized).clean {
                                t!("workspace_exit_clean_mode").to_string()
                            } else {
                                t!("workspace_enter_clean_mode").to_string()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.toggle_clean_mode(cx);
                            })),
                    )
                    .child(
                        Button::new("tab-bar-more")
                            .secondary()
                            .small()
                            .rounded(px(999.))
                            .icon(IconName::Ellipsis)
                            .tooltip(t!("more").to_string())
                            .dropdown_menu({
                                let view = view.clone();
                                move |menu, window, cx| {
                                    menu.min_w(150.)
                                        .item(
                                            PopupMenuItem::new(
                                                t!("settings_open_settings").to_string(),
                                            )
                                            .on_click(window.listener_for(
                                                &view,
                                                |this, _, window, cx| {
                                                    this.show_settings_window(window, cx)
                                                },
                                            )),
                                        )
                                        .item(
                                            PopupMenuItem::new(t!("tool_panel").to_string())
                                                .checked(view.read(cx).tool_panel.open)
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.toggle_tool_panel(window, cx)
                                                    },
                                                )),
                                        )
                                }
                            }),
                    ),
            )
    }

    pub(super) fn render_terminal_floating_toolbar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = cx.entity();
        let presentation = self.workspace_mode.presentation(self.sftp_panel.minimized);
        let has_selection = self.active_terminal_selection_text().is_some();
        let has_clipboard_text = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .is_some_and(|text| !text.is_empty());
        let can_use_sftp = self.active_kind() == Some(TabKind::Ssh);

        h_flex()
            .absolute()
            .right(px(12.))
            .bottom(px(12.))
            .items_center()
            .gap_1()
            .px_1()
            .py_1()
            .rounded(px(999.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background.opacity(0.96))
            .shadow_lg()
            .opacity(0.56)
            .hover(|this| this.opacity(1.0))
            .child(
                Button::new("terminal-toolbar-sftp")
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
                    .disabled(!can_use_sftp)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_sftp_minimized(window, cx);
                    })),
            )
            .child(
                Button::new("terminal-toolbar-search")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Search)
                    .tooltip(t!("terminal_find").to_string())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_search(window, cx);
                    })),
            )
            .child(
                Button::new("terminal-toolbar-split")
                    .ghost()
                    .xsmall()
                    .icon(IconName::PanelBottom)
                    .tooltip(t!("workspace_split").to_string())
                    .dropdown_menu_with_anchor(Anchor::TopRight, {
                        let view = view.clone();
                        move |menu, window, _cx| {
                            menu.min_w(160.)
                                .item(
                                    PopupMenuItem::new(t!("workspace_split_right").to_string())
                                        .on_click(window.listener_for(&view, |this, _, _, cx| {
                                            this.split_current_pane(
                                                crate::app::PaneDirection::Right,
                                                cx,
                                            );
                                        })),
                                )
                                .item(
                                    PopupMenuItem::new(t!("workspace_split_down").to_string())
                                        .on_click(window.listener_for(&view, |this, _, _, cx| {
                                            this.split_current_pane(
                                                crate::app::PaneDirection::Down,
                                                cx,
                                            );
                                        })),
                                )
                        }
                    }),
            )
            .child(
                Button::new("terminal-toolbar-copy")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Copy)
                    .tooltip(t!("terminal_copy").to_string())
                    .disabled(!has_selection)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(text) = this.active_terminal_selection_text() {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                            let active_id = this.workspace().active_tab_id().map(str::to_owned);
                            if let Some(active_id) = active_id
                                && let Some(tab) = this.terminal_tab_mut(&active_id)
                            {
                                tab.clear_selection();
                            }
                            this.focus_handle.focus(window, cx);
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new("terminal-toolbar-paste")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Inbox)
                    .tooltip(t!("terminal_paste").to_string())
                    .disabled(!has_clipboard_text)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                            this.paste_into_terminal(&text, window, cx);
                        }
                    })),
            )
            .child(
                Button::new("terminal-toolbar-more")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Ellipsis)
                    .tooltip(t!("more").to_string())
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, _cx| {
                        menu.min_w(140.).item(
                            PopupMenuItem::new(t!("terminal_clear").to_string()).on_click(
                                window.listener_for(&view, |this, _, _, cx| {
                                    this.clear_active_terminal(cx);
                                }),
                            ),
                        )
                    }),
            )
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
    }

    pub(super) fn render_terminal_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let has_active = self.workspace().active_tab_id().is_some();
        let pane_tree = self.workspace().pane_root().clone();
        let view = cx.entity();
        let bounds_view = view.clone();
        let menu_view = view.clone();

        let presentation = self.workspace_mode.presentation(self.sftp_panel.minimized);

        v_flex()
            .size_full()
            .relative()
            .when(presentation.clean, |this| this.p_1().gap_1())
            .when(!presentation.clean, |this| this.p_2().gap_2())
            .bg(cx.theme().muted.opacity(0.18))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .min_h(px(0.))
                    .rounded_lg()
                    .bg(cx.theme().background)
                    .on_prepaint(move |bounds, _window, cx| {
                        bounds_view.update(cx, |this, cx| {
                            if this.terminal_panel_bounds != Some(bounds) {
                                this.terminal_panel_bounds = Some(bounds);
                                cx.notify();
                            }
                        });
                    })
                    .overflow_hidden()
                    .track_focus(&self.focus_handle)
                    .key_context(TERMINAL_KEY_CONTEXT)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::focus_terminal))
                    .on_mouse_move(cx.listener(Self::on_terminal_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_terminal_mouse_up))
                    .on_key_down(cx.listener(Self::on_terminal_key_down))
                    .on_action(cx.listener(Self::on_terminal_tab_action))
                    .on_action(cx.listener(Self::on_terminal_backtab_action))
                    .on_scroll_wheel(cx.listener(Self::on_terminal_scroll))
                    .child(if has_active {
                        Self::render_pane_tree(self, &pane_tree, &[], cx).into_any_element()
                    } else {
                        self.render_home_page(cx).into_any_element()
                    })
                    .context_menu(move |menu, window, cx| {
                        Self::build_terminal_context_menu(menu, menu_view.clone(), window, cx)
                    }),
            )
            .when(has_active, |this| {
                this.child(self.render_terminal_floating_toolbar(window, cx))
            })
            // Search bar overlay — only when search is active.
            .when(self.window_state.search_active, |el| {
                el.child(self.render_search_bar(window, cx))
            })
            .when(
                self.tab_drag.is_dragging() || self.incoming_tab_drag.is_some(),
                |el| el.child(self.render_tab_drag_overlay(cx)),
            )
    }

    pub(super) fn render_tab_drag_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scrim = hsla(220. / 360., 0.25, 0.08, 0.22);
        let active = self.incoming_tab_drop_zone;
        let card = |zone: crate::app::tab_drag::DockZone, label: String| {
            let selected = active == Some(zone);
            div()
                .w(px(118.))
                .h(px(70.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(10.))
                .border_2()
                .border_color(if selected {
                    cx.theme().primary
                } else {
                    cx.theme().border
                })
                .bg(if selected {
                    cx.theme().primary.opacity(0.28)
                } else {
                    cx.theme().background.opacity(0.94)
                })
                .shadow_lg()
                .text_sm()
                .font_weight(if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                })
                .child(label)
        };

        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .bg(scrim)
            .when(self.incoming_tab_drag.is_some(), |this| {
                this.child(
                    v_flex()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(card(
                            crate::app::tab_drag::DockZone::Up,
                            t!("drag_dock_up").to_string(),
                        ))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(card(
                                    crate::app::tab_drag::DockZone::Left,
                                    t!("drag_dock_left").to_string(),
                                ))
                                .child(card(
                                    crate::app::tab_drag::DockZone::Center,
                                    t!("drag_merge_title").to_string(),
                                ))
                                .child(card(
                                    crate::app::tab_drag::DockZone::Right,
                                    t!("drag_dock_right").to_string(),
                                )),
                        )
                        .child(card(
                            crate::app::tab_drag::DockZone::Down,
                            t!("drag_dock_down").to_string(),
                        )),
                )
            })
            .when(
                self.incoming_tab_drag.is_none() && self.tab_drag.reorder_index().is_some(),
                |this| {
                    this.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .px_5()
                                    .py_3()
                                    .rounded(px(9.))
                                    .bg(cx.theme().success.opacity(0.92))
                                    .text_color(hsla(0., 0., 1., 1.))
                                    .child(t!("drag_reorder_title").to_string()),
                            ),
                    )
                },
            )
            .when(
                self.incoming_tab_drag.is_none() && self.tab_drag.outside(),
                |this| {
                    this.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .px_5()
                                    .py_3()
                                    .rounded(px(9.))
                                    .bg(cx.theme().primary.opacity(0.92))
                                    .text_color(hsla(0., 0., 1., 1.))
                                    .child(t!("drag_detach_title").to_string()),
                            ),
                    )
                },
            )
    }

    pub(super) fn render_terminal_completion_popup(
        this: &TinyShell,
        tab_id: &str,
        cursor: Option<terminal::CursorState>,
        cx: &mut Context<TinyShell>,
    ) -> Option<AnyElement> {
        let cursor = cursor?;
        let tab = this.terminal_tab(tab_id)?;
        if tab.kind != terminal::TabKind::Ssh || tab.is_alternate_screen() {
            return None;
        }
        let state = this.terminal_completions.get(tab_id)?;
        if !state.is_visible() || this.window_state.search_active || this.tab_drag.is_dragging() {
            return None;
        }
        let pane_bounds = this.terminal_bounds.get(tab_id)?;
        let candidates = state.candidates().to_vec();
        let selected = state.selected_index();
        let cell_width = this.terminal_cell_width();
        let line_height = this.terminal_line_height();
        let pane_width = pane_bounds.size.width.as_f32();
        let popup_width = (pane_width - 8.0).clamp(1.0, 380.0);
        let popup_height = candidates.len() as f32 * 32.0 + 30.0;
        let max_left = (pane_width - popup_width - 4.0).max(4.0);
        let left = (cursor.col as f32 * cell_width).clamp(4.0, max_left);
        let below = (cursor.row as f32 + 1.0) * line_height + 4.0;
        let top = if below + popup_height <= pane_bounds.size.height.as_f32() {
            below
        } else {
            (cursor.row as f32 * line_height - popup_height - 4.0).max(4.0)
        };

        let list = candidates.into_iter().enumerate().fold(
            v_flex().w_full(),
            |list, (index, candidate)| {
                let prefix = candidate.command[..candidate.matched_prefix_bytes].to_string();
                let suffix = candidate.command[candidate.matched_prefix_bytes..].to_string();
                let tab_id = tab_id.to_string();
                let focus_handle = this.focus_handle.clone();
                let is_selected = selected == Some(index);

                list.child(
                    h_flex()
                        .id(("terminal-completion", index))
                        .h(px(32.))
                        .w_full()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .when(is_selected, |row| row.bg(cx.theme().primary.opacity(0.16)))
                        .hover(|row| row.bg(cx.theme().muted.opacity(0.45)))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.focus_pane_with_id(tab_id.clone());
                                this.accept_terminal_completion_at(&tab_id, index, window, cx);
                                focus_handle.focus(window, cx);
                            }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .size(px(18.))
                                .rounded(px(4.))
                                .items_center()
                                .justify_center()
                                .bg(cx.theme().muted.opacity(0.55))
                                .child(Icon::new(IconName::SquareTerminal).with_size(Size::XSmall)),
                        )
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .font_family(this.terminal_font_family.clone())
                                .text_size(rems(0.82))
                                .child(
                                    div()
                                        .flex_none()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().primary)
                                        .child(prefix),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(suffix),
                                ),
                        )
                        .child(
                            div()
                                .max_w(px(popup_width * 0.42))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(rems(0.75))
                                .text_color(cx.theme().muted_foreground)
                                .child(candidate.label),
                        ),
                )
            },
        );

        Some(
            v_flex()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(popup_width))
                .overflow_hidden()
                .rounded(px(7.))
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .shadow_lg()
                .child(list)
                .child(
                    h_flex()
                        .h(px(30.))
                        .w_full()
                        .items_center()
                        .gap_3()
                        .px_2()
                        .border_t_1()
                        .border_color(cx.theme().border.opacity(0.7))
                        .bg(cx.theme().muted.opacity(0.28))
                        .text_size(rems(0.68))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("↑↓ {}", t!("terminal_completion_select")))
                        .child(format!("Tab {}", t!("terminal_completion_complete")))
                        .child(format!("Esc {}", t!("terminal_completion_close"))),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_pane_tree(
        this: &mut TinyShell,
        layout: &PaneLayout,
        path: &[usize],
        cx: &mut Context<TinyShell>,
    ) -> impl IntoElement {
        match layout {
            PaneLayout::Empty => this.render_home_page(cx).into_any_element(),
            PaneLayout::Single(tab_id) => {
                let is_focused = path == this.workspace().focused_pane_path();
                let keyword_highlight = this.config.keyword_highlight();
                let snapshot = this
                    .workspace()
                    .terminal_tab(tab_id)
                    .map(|t| t.render_snapshot(keyword_highlight));
                let Some(snapshot) = snapshot else {
                    return div().into_any_element();
                };
                let completion_cursor = snapshot.cursor;
                let tab_id_clone2 = tab_id.clone();
                let focus_handle = this.focus_handle.clone();
                let marked_text = if is_focused {
                    this.terminal_marked_text.clone()
                } else {
                    None
                };
                let font_family = this.terminal_font_family.clone();
                let font_size = px(this.terminal_font_size);
                let line_height = px(this.terminal_line_height());
                let cell_width = px(this.terminal_cell_width());
                let is_url_hovered = this
                    .hovered_url
                    .as_ref()
                    .is_some_and(|hu| hu.tab_id == *tab_id);
                let mut el = div()
                    .size_full()
                    .overflow_hidden()
                    .when(is_url_hovered, |d| d.cursor_pointer())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.focus_pane_with_id(tab_id_clone2.clone());
                            cx.notify();
                        }),
                    )
                    .child(terminal::element::TerminalElement::new(
                        terminal::element::TerminalElementProps {
                            view: cx.entity(),
                            focus_handle,
                            snapshot,
                            marked_text,
                            font_family,
                            font_size,
                            line_height,
                            cell_width,
                            tab_id: tab_id.to_string(),
                            search_highlights: this.search_highlight_map(
                                tab_id,
                                cx.theme().danger.opacity(0.35),
                                cx.theme().danger.opacity(0.70),
                            ),
                        },
                    ));
                let scrollbar = this.terminal_scrollbars.entry(tab_id.clone()).or_default();
                el = el.vertical_scrollbar(scrollbar);
                if is_focused
                    && let Some(popup) =
                        Self::render_terminal_completion_popup(this, tab_id, completion_cursor, cx)
                {
                    el = div().size_full().relative().child(el).child(popup);
                }

                // When disconnected, overlay a reconnect bar at the bottom of the terminal.
                // Uses absolute positioning so the terminal element itself is unchanged,
                // keeping panel size stable in multi-panel layouts.
                let disconnected_reason = this
                    .workspace()
                    .terminal_tab(tab_id)
                    .and_then(|tab| tab.disconnected_reason.clone());
                if let Some(reason) = disconnected_reason {
                    let tab_id_for_reconnect = tab_id.clone();
                    el = div().size_full().relative().child(el).child(
                        div().absolute().bottom_0().left_0().right_0().child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .py_1()
                                .bg(cx.theme().danger.opacity(0.15))
                                .child(
                                    div()
                                        .text_size(rems(0.85))
                                        .text_color(cx.theme().danger)
                                        .child(
                                            t!("session_disconnected", "reason" = reason)
                                                .to_string(),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(rems(0.85))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("— {}", t!("press_enter_to_reconnect"))),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.retry_disconnected_tab(&tab_id_for_reconnect, cx);
                                    }),
                                ),
                        ),
                    );
                }

                let indicator_color = this
                    .workspace()
                    .terminal_tab(tab_id)
                    .map(|tab| {
                        if tab.connected {
                            cx.theme().success
                        } else {
                            cx.theme().danger
                        }
                    })
                    .unwrap_or(cx.theme().success);
                let has_multiple_panes = this.workspace().pane_root().tab_ids().len() > 1;

                if !is_focused {
                    el = el.opacity(0.85);
                }

                let mut wrapper = div().size_full();
                if has_multiple_panes {
                    if is_focused {
                        wrapper = wrapper
                            .relative()
                            .child(
                                div()
                                    .absolute()
                                    .top(px(1.))
                                    .left(px(1.))
                                    .right(px(1.))
                                    .h(px(1.))
                                    .bg(indicator_color),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .bottom(px(1.))
                                    .left(px(1.))
                                    .right(px(1.))
                                    .h(px(1.))
                                    .bg(indicator_color),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(px(1.))
                                    .top(px(1.))
                                    .bottom(px(1.))
                                    .w(px(1.))
                                    .bg(indicator_color),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .right(px(1.))
                                    .top(px(1.))
                                    .bottom(px(1.))
                                    .w(px(1.))
                                    .bg(indicator_color),
                            )
                            .p(px(4.))
                            .child(el);
                    } else {
                        wrapper = wrapper.p(px(4.)).child(el);
                    }
                } else {
                    wrapper = wrapper.child(el);
                }

                if has_multiple_panes {
                    let pane_drag = IncomingPaneDrag {
                        group_id: this
                            .workspace()
                            .active_group_id()
                            .unwrap_or_default()
                            .to_string(),
                        tab_id: tab_id.clone(),
                    };
                    let pane_label = this.tab_title(tab_id);
                    wrapper = wrapper.relative().child(
                        div()
                            .id((gpui::ElementId::from("pane-drag-handle"), tab_id.clone()))
                            .absolute()
                            .top(px(6.))
                            .right(px(8.))
                            .cursor_move()
                            .opacity(0.35)
                            .hover(|this| this.opacity(0.95))
                            .on_drag(pane_drag, move |_, offset, _, cx| {
                                cx.new(|_| TabDragPreview {
                                    label: pane_label.clone(),
                                    offset,
                                })
                            })
                            .child(
                                Button::new((gpui::ElementId::from("pane-drag"), tab_id.clone()))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::ArrowRight)
                                    .tooltip(t!("pane_drag_hint").to_string()),
                            ),
                    );
                }

                wrapper.into_any_element()
            }
            PaneLayout::Horizontal(children, ratio) => {
                v_flex()
                    .size_full()
                    .children(children.iter().enumerate().flat_map(|(i, child)| {
                        let mut items: Vec<gpui::AnyElement> = Vec::new();
                        if i > 0 {
                            let splitter_path = path.to_vec(); // path to the CONTAINER that has the ratio
                            items.push(
                                div()
                                    .h(px(4.))
                                    .w_full()
                                    .flex_none()
                                    .cursor_row_resize()
                                    .bg(cx.theme().border)
                                    .hover(|s| s.bg(cx.theme().accent))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, event, window, cx| {
                                            window.prevent_default();
                                            cx.stop_propagation();
                                            this.start_drag_split(
                                                splitter_path.clone(),
                                                i,
                                                event,
                                                window,
                                                cx,
                                            );
                                        }),
                                    )
                                    .into_any_element(),
                            );
                        }
                        let mut child_path = path.to_vec();
                        child_path.push(i);
                        items.push(
                            div()
                                .flex_grow(if children.len() == 2 {
                                    if i == 0 { *ratio } else { 1.0 - *ratio }
                                } else {
                                    1.0
                                })
                                .min_h(px(0.))
                                .overflow_hidden()
                                .child(Self::render_pane_tree(this, child, &child_path, cx))
                                .into_any_element(),
                        );
                        items
                    }))
                    .into_any_element()
            }
            PaneLayout::Vertical(children, ratio) => h_flex()
                .items_stretch()
                .size_full()
                .children(children.iter().enumerate().flat_map(|(i, child)| {
                    let mut items: Vec<gpui::AnyElement> = Vec::new();
                    if i > 0 {
                        let splitter_path = path.to_vec(); // path to the CONTAINER that has the ratio
                        items.push(
                            div()
                                .w(px(4.))
                                .h_full()
                                .flex_none()
                                .cursor_col_resize()
                                .bg(cx.theme().border)
                                .hover(|s| s.bg(cx.theme().accent))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, event, window, cx| {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        this.start_drag_split(
                                            splitter_path.clone(),
                                            i,
                                            event,
                                            window,
                                            cx,
                                        );
                                    }),
                                )
                                .into_any_element(),
                        );
                    }
                    let mut child_path = path.to_vec();
                    child_path.push(i);
                    items.push(
                        div()
                            .flex_grow(if children.len() == 2 {
                                if i == 0 { *ratio } else { 1.0 - *ratio }
                            } else {
                                1.0
                            })
                            .min_w(px(0.))
                            .overflow_hidden()
                            .child(Self::render_pane_tree(this, child, &child_path, cx))
                            .into_any_element(),
                    );
                    items
                }))
                .into_any_element(),
        }
    }

    pub(super) fn build_sftp_tree_context_menu(
        menu: PopupMenu,
        view: gpui::Entity<TinyShell>,
        remote_path: String,
        permissions: Option<u32>,
        window: &mut Window,
        _cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let can_mutate_target = remote_path != "/";

        menu.item(
            PopupMenuItem::new(t!("refresh").to_string()).on_click(window.listener_for(&view, {
                let remote_path = remote_path.clone();
                move |this, _, _, cx| {
                    if let Some(handle) = this.active_sftp_handle() {
                        handle.list_directory_tree(remote_path.clone());
                    }
                    if this
                        .active_sftp()
                        .is_some_and(|sftp| sftp.current_path == remote_path)
                    {
                        this.refresh_sftp(cx);
                    } else {
                        cx.notify();
                    }
                }
            })),
        )
        .separator()
        .item(
            PopupMenuItem::new(t!("sftp_new_folder").to_string()).on_click(window.listener_for(
                &view,
                {
                    let remote_path = remote_path.clone();
                    move |this, _, window, cx| {
                        this.navigate_sftp(remote_path.clone(), cx);
                        this.show_sftp_create_dialog(true, window, cx);
                    }
                },
            )),
        )
        .item(
            PopupMenuItem::new(t!("sftp_rename").to_string())
                .disabled(!can_mutate_target)
                .on_click(window.listener_for(&view, {
                    let remote_path = remote_path.clone();
                    move |this, _, window, cx| {
                        this.show_sftp_rename_dialog(remote_path.clone(), window, cx);
                    }
                })),
        )
        .item(
            PopupMenuItem::new(t!("delete").to_string())
                .disabled(!can_mutate_target)
                .on_click(window.listener_for(&view, {
                    let remote_path = remote_path.clone();
                    move |this, _, window, cx| {
                        this.show_sftp_delete_paths_confirm_dialog(
                            vec![remote_path.clone()],
                            false,
                            window,
                            cx,
                        );
                    }
                })),
        )
        .item(
            PopupMenuItem::new(t!("sftp_quick_delete").to_string())
                .disabled(!can_mutate_target)
                .on_click(window.listener_for(&view, {
                    let remote_path = remote_path.clone();
                    move |this, _, window, cx| {
                        this.show_sftp_delete_paths_confirm_dialog(
                            vec![remote_path.clone()],
                            true,
                            window,
                            cx,
                        );
                    }
                })),
        )
        .separator()
        .item(
            PopupMenuItem::new(t!("sftp_copy_path").to_string()).on_click(window.listener_for(
                &view,
                {
                    let remote_path = remote_path.clone();
                    move |_, _, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(remote_path.clone()));
                    }
                },
            )),
        )
        .separator()
        .item(
            PopupMenuItem::new(t!("download").to_string()).on_click(window.listener_for(&view, {
                let remote_path = remote_path.clone();
                move |this, _, window, cx| {
                    this.download_sftp_entry(remote_path.clone(), window, cx);
                }
            })),
        )
        .item(
            PopupMenuItem::new(t!("upload").to_string()).on_click(window.listener_for(&view, {
                let remote_path = remote_path.clone();
                move |this, _, window, cx| {
                    this.upload_sftp_files_to(remote_path.clone(), window, cx);
                }
            })),
        )
        .separator()
        .item(
            PopupMenuItem::new(t!("sftp_file_permissions").to_string()).on_click(
                window.listener_for(&view, move |this, _, window, cx| {
                    this.show_sftp_permissions_dialog(
                        remote_path.clone(),
                        true,
                        permissions,
                        window,
                        cx,
                    );
                }),
            ),
        )
    }

    pub(super) fn build_sftp_tree_empty_context_menu(
        menu: PopupMenu,
        view: gpui::Entity<TinyShell>,
        remote_path: String,
        window: &mut Window,
        _cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        menu.item(
            PopupMenuItem::new(t!("sftp_new_folder").to_string()).on_click(window.listener_for(
                &view,
                move |this, _, window, cx| {
                    this.navigate_sftp(remote_path.clone(), cx);
                    this.show_sftp_create_dialog(true, window, cx);
                },
            )),
        )
    }

    pub(super) fn build_sftp_context_menu(
        mut menu: PopupMenu,
        view: gpui::Entity<TinyShell>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let target = view.read(cx).sftp_workspace.context_menu.clone();
        let has_target = target
            .as_ref()
            .is_some_and(|target| target.remote_path.is_some());
        let is_file = target
            .as_ref()
            .is_some_and(|target| target.remote_path.is_some() && !target.is_dir);
        let editable = target
            .as_ref()
            .and_then(|target| target.remote_path.as_deref())
            .is_some_and(is_editable_text_file);
        let external_editor_set = !view.read(cx).config.sftp_external_editor().is_empty();

        menu = menu
            .item(
                PopupMenuItem::new(t!("refresh").to_string()).on_click(window.listener_for(
                    &view,
                    |this, _, _, cx| {
                        this.sftp_workspace.context_menu = None;
                        this.refresh_sftp(cx);
                    },
                )),
            )
            .separator()
            .item(
                PopupMenuItem::new(t!("sftp_open").to_string())
                    .disabled(!has_target)
                    .on_click(window.listener_for(&view, |this, _, _, cx| {
                        this.trigger_sftp_context_open(cx);
                    })),
            );

        menu = if is_file {
            menu.submenu(t!("sftp_open_with").to_string(), window, cx, {
                let view = view.clone();
                move |submenu, window, _| {
                    submenu
                        .item(
                            PopupMenuItem::new(t!("sftp_text_editor").to_string())
                                .disabled(!editable)
                                .on_click(window.listener_for(&view, |this, _, _, cx| {
                                    this.trigger_sftp_context_internal_editor(cx);
                                })),
                        )
                        .item(
                            PopupMenuItem::new(t!("sftp_system_association").to_string()).on_click(
                                window.listener_for(&view, |this, _, _, cx| {
                                    this.trigger_sftp_context_system_open(cx);
                                }),
                            ),
                        )
                }
            })
        } else {
            menu.item(PopupMenuItem::new(t!("sftp_open_with").to_string()).disabled(true))
        };

        menu = if is_file {
            menu.submenu(t!("sftp_select_text_editor").to_string(), window, cx, {
                let view = view.clone();
                move |submenu, window, _| {
                    submenu
                        .item(
                            PopupMenuItem::new(t!("sftp_internal_editor").to_string())
                                .disabled(!editable)
                                .on_click(window.listener_for(&view, |this, _, _, cx| {
                                    this.trigger_sftp_context_internal_editor(cx);
                                })),
                        )
                        .item(
                            PopupMenuItem::new(t!("sftp_external_editor").to_string())
                                .disabled(!external_editor_set)
                                .on_click(window.listener_for(&view, |this, _, _, cx| {
                                    this.trigger_sftp_context_external_editor(cx);
                                })),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new(t!("sftp_set_external_editor").to_string())
                                .on_click(window.listener_for(&view, |this, _, window, cx| {
                                    this.choose_sftp_external_editor(window, cx);
                                })),
                        )
                }
            })
        } else {
            menu.item(PopupMenuItem::new(t!("sftp_select_text_editor").to_string()).disabled(true))
        };

        menu.separator()
            .item(
                PopupMenuItem::new(t!("sftp_copy_path").to_string())
                    .disabled(!has_target)
                    .on_click(window.listener_for(&view, |this, _, _, cx| {
                        this.trigger_sftp_context_copy_path(cx);
                    })),
            )
            .separator()
            .item(
                PopupMenuItem::new(t!("download").to_string())
                    .disabled(!has_target)
                    .on_click(window.listener_for(&view, |this, _, window, cx| {
                        this.trigger_sftp_context_download(window, cx);
                    })),
            )
            .item(
                PopupMenuItem::new(t!("upload").to_string()).on_click(window.listener_for(
                    &view,
                    |this, _, window, cx| {
                        this.trigger_sftp_context_upload(window, cx);
                    },
                )),
            )
            .separator()
            .item(
                PopupMenuItem::new(t!("sftp_pack_transfer").to_string())
                    .disabled(!has_target)
                    .on_click(window.listener_for(&view, |this, _, window, cx| {
                        this.trigger_sftp_context_pack_download(window, cx);
                    })),
            )
            .separator()
            .submenu(t!("sftp_new").to_string(), window, cx, {
                let view = view.clone();
                move |submenu, window, _| {
                    submenu
                        .item(
                            PopupMenuItem::new(t!("sftp_new_file").to_string()).on_click(
                                window.listener_for(&view, |this, _, window, cx| {
                                    this.trigger_sftp_context_new_file(window, cx);
                                }),
                            ),
                        )
                        .item(
                            PopupMenuItem::new(t!("sftp_new_folder").to_string()).on_click(
                                window.listener_for(&view, |this, _, window, cx| {
                                    this.trigger_sftp_context_new_folder(window, cx);
                                }),
                            ),
                        )
                }
            })
            .separator()
            .item(
                PopupMenuItem::new(t!("sftp_rename").to_string())
                    .disabled(!has_target)
                    .on_click(window.listener_for(&view, |this, _, window, cx| {
                        this.trigger_sftp_context_rename(window, cx);
                    })),
            )
            .item(
                PopupMenuItem::new(t!("delete").to_string())
                    .disabled(!has_target)
                    .on_click(window.listener_for(&view, |this, _, window, cx| {
                        this.trigger_sftp_context_delete(false, window, cx);
                    })),
            )
            .item(
                PopupMenuItem::new(t!("sftp_quick_delete").to_string())
                    .disabled(!has_target)
                    .on_click(window.listener_for(&view, |this, _, window, cx| {
                        this.trigger_sftp_context_delete(true, window, cx);
                    })),
            )
            .item(
                PopupMenuItem::new(t!("sftp_file_permissions").to_string())
                    .disabled(!has_target)
                    .on_click(window.listener_for(&view, |this, _, window, cx| {
                        this.trigger_sftp_context_permissions(window, cx);
                    })),
            )
    }

    pub(super) fn build_terminal_context_menu(
        menu: PopupMenu,
        view: gpui::Entity<TinyShell>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let selection = view.read(cx).active_terminal_selection_text();
        let has_selection = selection.is_some();
        let has_clipboard_text = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .is_some_and(|text| !text.is_empty());

        menu.action_context(view.read(cx).focus_handle.clone())
            .item(
                PopupMenuItem::new(t!("terminal_copy").to_string())
                    .disabled(!has_selection)
                    .action(Box::new(crate::Copy)),
            )
            .item(
                PopupMenuItem::new(t!("terminal_paste").to_string())
                    .disabled(!has_clipboard_text)
                    .action(Box::new(crate::Paste)),
            )
            .item(
                PopupMenuItem::new(t!("terminal_paste_selection").to_string())
                    .disabled(!has_selection)
                    .on_click(window.listener_for(&view, move |this, _, window, cx| {
                        if let Some(text) = selection.as_deref() {
                            this.paste_into_terminal(text, window, cx);
                        }
                    })),
            )
            .separator()
            .item(
                PopupMenuItem::new(t!("terminal_find").to_string())
                    .action(Box::new(crate::OpenSearch)),
            )
            .separator()
            .item(
                PopupMenuItem::new(t!("terminal_clear").to_string()).on_click(window.listener_for(
                    &view,
                    |this, _, _, cx| {
                        this.clear_active_terminal(cx);
                    },
                )),
            )
    }

    pub(super) fn build_tab_context_menu(
        menu: PopupMenu,
        view: gpui::Entity<TinyShell>,
        group_id: String,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let (
            duplicate_session,
            reconnect_tab_ids,
            reconnect_all_tab_ids,
            is_connected_ssh,
            close_tab_id,
            close_other_tab_ids,
            close_all_tab_ids,
        ) = {
            let this = view.read(cx);
            let group_tab_ids: Vec<String> = this
                .workspace()
                .tab_groups()
                .iter()
                .find(|group| group.id == group_id)
                .map(|group| {
                    group
                        .pane_root
                        .tab_ids()
                        .iter()
                        .map(|tab_id| (*tab_id).to_string())
                        .collect()
                })
                .unwrap_or_default();
            let close_tab_id = group_tab_ids.first().cloned();
            let close_other_tab_ids: Vec<String> = this
                .workspace()
                .tab_groups()
                .iter()
                .filter(|group| group.id != group_id)
                .filter_map(|group| group.pane_root.tab_ids().first().copied())
                .map(String::from)
                .collect();
            let close_all_tab_ids: Vec<String> = this
                .workspace()
                .tab_groups()
                .iter()
                .filter_map(|group| group.pane_root.tab_ids().first().copied())
                .map(String::from)
                .collect();
            let duplicate_session = group_tab_ids.iter().find_map(|tab_id| {
                this.workspace()
                    .terminal_tab(tab_id)
                    .filter(|tab| tab.kind == TabKind::Ssh)
                    .and_then(|tab| tab.session.clone())
            });
            let reconnect_tab_ids: Vec<String> = group_tab_ids
                .iter()
                .filter(|tab_id| {
                    this.workspace().terminal_tab(tab_id).is_some_and(|tab| {
                        tab.kind == TabKind::Ssh
                            && !tab.connected
                            && tab.disconnected_reason.is_some()
                    })
                })
                .cloned()
                .collect();
            let reconnect_all_tab_ids: Vec<String> = this
                .workspace()
                .tabs()
                .iter()
                .filter(|tab| {
                    tab.kind == TabKind::Ssh && !tab.connected && tab.disconnected_reason.is_some()
                })
                .map(|tab| tab.id.clone())
                .collect();
            let is_connected_ssh = group_tab_ids.iter().any(|tab_id| {
                this.workspace()
                    .terminal_tab(tab_id)
                    .is_some_and(|tab| tab.kind == TabKind::Ssh && tab.connected)
            });
            (
                duplicate_session,
                reconnect_tab_ids,
                reconnect_all_tab_ids,
                is_connected_ssh,
                close_tab_id,
                close_other_tab_ids,
                close_all_tab_ids,
            )
        };

        let mut menu = menu
            .item(
                PopupMenuItem::new(t!("tab_copy_label").to_string())
                    .disabled(duplicate_session.is_none())
                    .on_click(window.listener_for(&view, move |this, _, _, cx| {
                        if let Some(session) = duplicate_session.clone() {
                            this.open_ssh_session(session, cx);
                        }
                    })),
            )
            .item(
                PopupMenuItem::new(t!("tab_connect").to_string())
                    .disabled(reconnect_tab_ids.is_empty())
                    .on_click(window.listener_for(&view, move |this, _, _, cx| {
                        for tab_id in &reconnect_tab_ids {
                            this.retry_disconnected_tab(tab_id, cx);
                        }
                        cx.notify();
                    })),
            )
            .item(
                PopupMenuItem::new(t!("tab_connect_all").to_string())
                    .disabled(reconnect_all_tab_ids.is_empty())
                    .on_click(window.listener_for(&view, move |this, _, _, cx| {
                        for tab_id in &reconnect_all_tab_ids {
                            this.retry_disconnected_tab(tab_id, cx);
                        }
                        cx.notify();
                    })),
            )
            .separator()
            .item(
                PopupMenuItem::new(t!("tab_disconnect").to_string())
                    .disabled(!is_connected_ssh)
                    .on_click(window.listener_for(&view, {
                        let group_id = group_id.clone();
                        move |this, _, _, cx| {
                            this.disconnect_tab_group(&group_id, cx);
                        }
                    })),
            )
            .separator()
            .item(
                PopupMenuItem::new(t!("tab_close").to_string())
                    .disabled(close_tab_id.is_none())
                    .on_click(window.listener_for(&view, move |this, _, _, cx| {
                        if let Some(tab_id) = close_tab_id.clone() {
                            this.close_tab(tab_id, cx);
                        }
                    })),
            )
            .item(
                PopupMenuItem::new(t!("tab_close_others").to_string())
                    .disabled(close_other_tab_ids.is_empty())
                    .on_click(window.listener_for(&view, move |this, _, _, cx| {
                        for tab_id in &close_other_tab_ids {
                            this.close_tab(tab_id.clone(), cx);
                        }
                        cx.notify();
                    })),
            )
            .item(
                PopupMenuItem::new(t!("tab_close_all").to_string())
                    .disabled(close_all_tab_ids.is_empty())
                    .on_click(window.listener_for(&view, move |this, _, _, cx| {
                        for tab_id in &close_all_tab_ids {
                            this.close_tab(tab_id.clone(), cx);
                        }
                        cx.notify();
                    })),
            )
            .separator();

        let source_window = window.window_handle();
        let targets = crate::app::other_main_windows(source_window);
        for (index, (target_window, target)) in targets.iter().cloned().enumerate() {
            let move_group_id = group_id.clone();
            menu = menu.item(
                PopupMenuItem::new(t!("tab_move_to_window", index = index + 1).to_string())
                    .on_click(window.listener_for(&view, move |this, _, _, cx| {
                        this.move_group_to_window(
                            move_group_id.clone(),
                            target_window,
                            target.clone(),
                            source_window,
                            cx,
                        );
                    })),
            );
        }
        if let Some((target_window, target)) = targets.into_iter().next() {
            menu = menu.item(
                PopupMenuItem::new(t!("tab_merge_whole_window").to_string()).on_click(
                    window.listener_for(&view, move |this, _, _, cx| {
                        this.merge_window_into(source_window, target_window, target.clone(), cx);
                    }),
                ),
            );
        }
        menu.item(
            PopupMenuItem::new(t!("settings_detach_tab").to_string()).on_click(
                window.listener_for(&view, move |this, _, window, cx| {
                    this.defer_group_detach(group_id.clone(), window, cx);
                }),
            ),
        )
    }
}

#[cfg(test)]
mod native_tab_width_tests {
    use super::*;

    #[test]
    fn native_title_can_grow_beyond_the_compact_tab_minimum() {
        assert_eq!(NATIVE_TAB_MIN_WIDTH, 96.);
        assert_eq!(NATIVE_TAB_TITLE_MAX_WIDTH, 192.);
    }
}
