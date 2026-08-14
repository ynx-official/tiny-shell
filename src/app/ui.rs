use crate::app::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui::{
    Anchor, Animation, AnimationExt as _, AnyElement, AppContext as _, Context, ElementId,
    Focusable as _, FontWeight, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, ParentElement as _, PathBuilder, Pixels, Point, Render,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, ease_out_quint, hsla, point,
    prelude::FluentBuilder as _, px, relative, rems, uniform_list,
};
use gpui_component::{
    ActiveTheme, Disableable as _, ElementExt, Icon, IconName, InteractiveElementExt as _, Root,
    Selectable as _, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    hover_card::HoverCard,
    input::Input,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    progress::Progress,
    scroll::{ScrollableElement as _, Scrollbar, ScrollbarShow},
    tab::{Tab, TabBar},
    v_flex,
};
use rust_i18n::t;
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    time::Duration,
};

use crate::{
    PaneLayout, TinyShell,
    app::constants::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH, TERMINAL_KEY_CONTEXT},
    app::{
        HomePage, IncomingPaneDrag, IncomingTabDrag, ProcessView, SftpPanelView,
        settings::MonitoringPosition,
    },
    sftp::format_mtime,
    sftp::ops::is_editable_text_file,
    system::format_bytes,
    terminal::{self, TabKind},
};

#[derive(Clone, Copy)]
enum SftpToolbarItem {
    SyncCwd,
    HiddenFiles,
    Refresh,
    NewFolder,
    Delete,
    UploadFile,
    UploadFolder,
    Download,
}

#[derive(Clone, Copy)]
enum SftpFooterItem {
    SyncStatus,
    Latency,
    Transfers,
}

struct TabDragPreview {
    label: String,
    offset: Point<Pixels>,
}

impl Render for TabDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().pl(self.offset.x).pt(self.offset.y).child(
            h_flex()
                .max_w(px(320.))
                .gap_2()
                .px_3()
                .py_2()
                .rounded(px(8.))
                .border_1()
                .border_color(hsla(199. / 360., 0.82, 0.68, 1.0))
                .bg(hsla(217. / 360., 0.74, 0.30, 0.96))
                .text_color(hsla(0., 0., 1., 1.))
                .shadow_lg()
                .child(Icon::new(IconName::SquareTerminal).with_size(Size::Small))
                .child(div().min_w_0().truncate().child(self.label.clone())),
        )
    }
}

fn format_uptime(seconds: u64) -> String {
    if seconds == 0 {
        return "-".to_string();
    }
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{}{} {}{}", days, t!("days"), hours, t!("hours"))
    } else if hours > 0 {
        format!("{}{} {}{}", hours, t!("hours"), minutes, t!("minutes"))
    } else {
        format!("{}{}", minutes, t!("minutes"))
    }
}

fn lerp_hsla(from: Hsla, to: Hsla, delta: f32) -> Hsla {
    let delta = delta.clamp(0.0, 1.0);
    let mut hue_delta = to.h - from.h;
    if hue_delta > 0.5 {
        hue_delta -= 1.0;
    } else if hue_delta < -0.5 {
        hue_delta += 1.0;
    }
    let hue = from.h + hue_delta * delta;
    Hsla {
        h: if hue < 0.0 {
            hue + 1.0
        } else if hue > 1.0 {
            hue - 1.0
        } else {
            hue
        },
        s: from.s + (to.s - from.s) * delta,
        l: from.l + (to.l - from.l) * delta,
        a: from.a + (to.a - from.a) * delta,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceShellLayout {
    Hidden,
    Collapsed,
    Resizable,
}

fn workspace_shell_layout(show_sidebar: bool, sidebar_collapsed: bool) -> WorkspaceShellLayout {
    if !show_sidebar {
        WorkspaceShellLayout::Hidden
    } else if sidebar_collapsed {
        WorkspaceShellLayout::Collapsed
    } else {
        WorkspaceShellLayout::Resizable
    }
}

impl TinyShell {
    /// Erases the deeply nested workspace element before it reaches the native
    /// window shell. On Windows, keeping the complete GPUI tree in one debug
    /// stack frame can overflow while resize or window-detach triggers a
    /// synchronous redraw.
    fn render_workspace_shell(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Rendering only derives elements; state synchronization runs in prepaint.

        // The file-transfer panel belongs to an active terminal session. Keeping it
        // out of the home workspace avoids showing an empty "remote files" area on
        // Overview and Key Manager pages.
        let main_view_key = self.main_view_key();
        let presentation = self.workspace_mode.presentation(self.sftp_panel.minimized);
        let main_content_raw = if self.workspace().active_system_info_tab_id().is_some() {
            self.render_system_info_page(cx).into_any_element()
        } else if self.workspace().active_tab_id().is_some() && !self.home_page_open {
            let monitoring_position =
                MonitoringPosition::from_config(self.config.monitoring_position());
            let monitoring_contents = v_flex()
                .size_full()
                .min_h(px(0.))
                .overflow_hidden()
                .when(monitoring_position == MonitoringPosition::Bottom, |this| {
                    this.child(self.render_monitoring_panel(window.viewport_size().width, cx))
                })
                .child(
                    v_flex()
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_hidden()
                        .child(self.render_sftp_panel(window, cx)),
                );

            let is_monitor_bottom = monitoring_position == MonitoringPosition::Bottom;
            let minimized_height = if presentation.clean {
                1.
            } else if is_monitor_bottom {
                81.
            } else {
                1.
            };
            let min_panel_height = if is_monitor_bottom { 260. } else { 180. };
            let default_panel_height = if is_monitor_bottom { 328. } else { 248. };

            let sftp_size = if presentation.sftp_minimized {
                px(minimized_height)
            } else {
                px(self
                    .config
                    .body_panels()
                    .and_then(|s| s.get(1).copied())
                    .unwrap_or(default_panel_height))
            };

            let body = v_resizable("tiny-shell-body")
                .lock(self.config.lock_layout())
                .with_state(&self.body_panels)
                .child(resizable_panel().child(self.render_terminal_panel(window, cx)))
                .child(
                    resizable_panel()
                        .size(sftp_size)
                        .size_range(if presentation.sftp_minimized {
                            px(minimized_height)..px(minimized_height)
                        } else {
                            px(min_panel_height)..px(1200.)
                        })
                        .child(monitoring_contents),
                );

            v_flex()
                .size_full()
                .min_h(px(0.))
                .overflow_hidden()
                .child(div().flex_1().min_h(px(0.)).overflow_hidden().child(body))
                .when(presentation.show_sftp_footer, |this| {
                    this.child(self.render_sftp_footer(cx))
                })
                .into_any_element()
        } else {
            match self.home_page {
                HomePage::Overview => self.render_home_page(cx).into_any_element(),
                HomePage::Connections => self.render_connection_manager_page(cx).into_any_element(),
                HomePage::Commands => self.render_command_manager_page(cx).into_any_element(),
                HomePage::KeyManager => self.render_key_manager_page(cx).into_any_element(),
                HomePage::Settings => self.render_settings_page(cx).into_any_element(),
            }
        };

        let main_content = div()
            .size_full()
            .overflow_hidden()
            .child(main_content_raw)
            .with_animation(
                ElementId::NamedInteger("main-content-fade".into(), main_view_key),
                Animation::new(Duration::from_millis(240)).with_easing(ease_out_quint()),
                |this, delta| this.opacity(delta * delta),
            );

        match workspace_shell_layout(presentation.show_sidebar, self.sidebar_collapsed) {
            WorkspaceShellLayout::Hidden => v_flex()
                .size_full()
                .relative()
                .overflow_hidden()
                .when(
                    self.active_title_bar_style == crate::session::config::TitleBarStyle::Native,
                    |this| {
                        this.child(
                            div()
                                .flex_none()
                                .h(px(32.))
                                .w_full()
                                .bg(cx.theme().tab_bar)
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .child(self.render_tab_bar(window.window_handle(), cx)),
                        )
                    },
                )
                .child(main_content)
                .into_any_element(),
            WorkspaceShellLayout::Collapsed => h_flex()
                .size_full()
                .child(
                    div()
                        .flex_none()
                        .w(px(COLLAPSED_SIDEBAR_WIDTH))
                        .h_full()
                        .child(self.render_collapsed_sidebar(cx)),
                )
                .child(
                    div().flex_1().h_full().min_w(px(0.)).child(
                        v_flex()
                            .size_full()
                            .relative()
                            .overflow_hidden()
                            .when(
                                self.active_title_bar_style
                                    == crate::session::config::TitleBarStyle::Native,
                                |this| {
                                    this.child(
                                        div()
                                            .flex_none()
                                            .h(px(32.))
                                            .w_full()
                                            .bg(cx.theme().tab_bar)
                                            .border_b_1()
                                            .border_color(cx.theme().border)
                                            .child(self.render_tab_bar(window.window_handle(), cx)),
                                    )
                                },
                            )
                            .child(main_content),
                    ),
                )
                .into_any_element(),
            WorkspaceShellLayout::Resizable => {
                let sidebar_content =
                    if self.workspace().active_tab_id().is_some() && !self.home_page_open {
                        self.sidebar(cx).into_any_element()
                    } else {
                        self.render_overview_sidebar(cx).into_any_element()
                    };

                let sidebar_area = resizable_panel()
                    .size(px(self
                        .config
                        .workspace_panels()
                        .and_then(|s| s.first().copied())
                        .unwrap_or(SIDEBAR_WIDTH)))
                    .size_range(px(190.)..px(360.))
                    .flex_none()
                    .child(sidebar_content);

                let main_area = resizable_panel().child(
                    v_flex()
                        .size_full()
                        .relative()
                        .overflow_hidden()
                        .when(
                            self.active_title_bar_style
                                == crate::session::config::TitleBarStyle::Native,
                            |this| {
                                this.child(
                                    div()
                                        .flex_none()
                                        .h(px(32.))
                                        .w_full()
                                        .bg(cx.theme().tab_bar)
                                        .border_b_1()
                                        .border_color(cx.theme().border)
                                        .child(self.render_tab_bar(window.window_handle(), cx)),
                                )
                            },
                        )
                        .child(main_content),
                );

                h_resizable("tiny-shell-workspace")
                    .lock(self.config.lock_layout())
                    .with_state(&self.workspace_panels)
                    .child(sidebar_area)
                    .child(main_area)
                    .into_any_element()
            }
        }
    }
}

impl TinyShell {
    /// Builds the native window shell behind a second type-erased boundary so
    /// resize and the first detached-window frame do not retain workspace
    /// construction locals on the same Windows UI-thread stack.
    fn render_root_shell(
        &mut self,
        workspace: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let drag_move_view = cx.entity();
        let pane_drag_move_view = drag_move_view.clone();
        let drop_view = drag_move_view.clone();
        let workspace_with_tool_panel = self.render_workspace_with_tool_panel(workspace, cx);

        v_flex()
            .id("tiny-shell-root")
            .size_full()
            .relative()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(self.ui_font_family.clone())
            .on_drag_move::<IncomingTabDrag>(move |event, window, cx| {
                let drag = event.drag(cx).clone();
                let position = event.event.position;
                drag_move_view.update(cx, |this, cx| {
                    this.on_native_tab_drag_move(drag, position, window, cx);
                });
            })
            .on_drop::<IncomingPaneDrag>({
                let pane_drop_view = pane_drag_move_view;
                move |drag, window, cx| {
                    let drag = drag.clone();
                    let position = window.mouse_position();
                    pane_drop_view.update(cx, |this, cx| {
                        let Some((target_tab_id, bounds)) = this
                            .terminal_bounds
                            .iter()
                            .find(|(_, bounds)| bounds.contains(&position))
                            .map(|(id, bounds)| (id.clone(), *bounds))
                        else {
                            return;
                        };
                        let zone = crate::app::tab_drag::dock_zone_at(position, bounds, None)
                            .unwrap_or_default();
                        this.dock_pane(&drag.group_id, &drag.tab_id, &target_tab_id, zone, cx);
                    });
                }
            })
            .on_drop::<IncomingTabDrag>(move |drag, window, cx| {
                let drag = drag.clone();
                let target_window = window.window_handle();
                let target = drop_view.clone();
                crate::app::clear_tab_drag_hover();
                if drag.source_window == target_window {
                    let drag_id = drag.drag_id;
                    let position = window.mouse_position();
                    target.update(cx, |target, cx| {
                        target.finish_native_local_tab_drop(drag.group_id, position, window, cx);
                    });
                    window.defer(cx, move |_window, cx| {
                        crate::app::clear_incoming_tab_drag_except(drag_id, None, cx);
                    });
                    return;
                }

                let position = window.mouse_position();
                if !TinyShell::promote_native_tab_drag_from_target(&drag, position, window, cx) {
                    drag.source
                        .update(cx, |source, cx| source.cancel_tab_drag(cx));
                    window.defer(cx, move |_window, cx| {
                        crate::app::clear_incoming_tab_drag_except(drag.drag_id, None, cx);
                    });
                    return;
                }
                let Some(zone) = target.read(cx).native_tab_drop_zone(position) else {
                    crate::app::clear_tab_drag_hover();
                    drag.source
                        .update(cx, |source, cx| source.cancel_tab_drag(cx));
                    window.defer(cx, move |_window, cx| {
                        crate::app::clear_incoming_tab_drag_except(drag.drag_id, None, cx);
                    });
                    return;
                };
                TinyShell::defer_native_cross_window_tab_drop(
                    drag,
                    target_window,
                    target,
                    zone,
                    window,
                    cx,
                );
            })
            // Keep tab-drag tracking on the root element. Registering a window
            // listener from Render is invalid during GPUI's layout phase.
            .on_mouse_move(cx.listener(Self::on_tab_drag_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_tab_drag_mouse_up))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(Self::on_tab_drag_mouse_up_out),
            )
            .on_action(cx.listener(|this, _: &crate::OpenSettings, window, cx| {
                this.show_settings_window(window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::OpenSession, window, cx| {
                this.show_selector_dialog(window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::OpenTransfers, window, cx| {
                this.show_transfers_dialog(window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::NewSsh, window, cx| {
                this.open_new_ssh_dialog(window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::NewWindow, _, cx| this.open_new_window(cx)))
            .on_action(
                cx.listener(|this, _: &crate::DetachTabToWindow, window, cx| {
                    this.detach_tab_to_new_window(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::MoveTabNextWindow, window, cx| {
                    this.move_active_group_to_adjacent_window(window.window_handle(), false, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::MoveTabPreviousWindow, window, cx| {
                    this.move_active_group_to_adjacent_window(window.window_handle(), true, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::OpenSearch, window, cx| {
                    this.toggle_search(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &crate::ToggleSidebar, _, cx| {
                this.sidebar_collapsed = !this.sidebar_collapsed;
                this.config.set_sidebar_collapsed(this.sidebar_collapsed);
                this.mark_config_preferences_dirty();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ToggleSftpZoom, window, cx| {
                this.toggle_sftp_minimized(window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::FocusPaneLeft, _, _| {
                this.focus_adjacent_pane(crate::app::PaneDirection::Left)
            }))
            .on_action(cx.listener(|this, _: &crate::FocusPaneRight, _, _| {
                this.focus_adjacent_pane(crate::app::PaneDirection::Right)
            }))
            .on_action(cx.listener(|this, _: &crate::FocusPaneUp, _, _| {
                this.focus_adjacent_pane(crate::app::PaneDirection::Up)
            }))
            .on_action(cx.listener(|this, _: &crate::FocusPaneDown, _, _| {
                this.focus_adjacent_pane(crate::app::PaneDirection::Down)
            }))
            .on_action(cx.listener(|this, _: &crate::SplitPaneLeft, _, cx| {
                this.split_current_pane(crate::app::PaneDirection::Left, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::SplitPaneRight, _, cx| {
                this.split_current_pane(crate::app::PaneDirection::Right, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::SplitPaneUp, _, cx| {
                this.split_current_pane(crate::app::PaneDirection::Up, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::SplitPaneDown, _, cx| {
                this.split_current_pane(crate::app::PaneDirection::Down, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::ClosePane, _, cx| {
                if let Some(active_id) = this.workspace().active_tab_id().map(str::to_owned) {
                    this.close_tab(active_id, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &crate::Copy, window, cx| {
                if window.focused(cx) == Some(this.focus_handle.clone()) {
                    if let Some(text) = this.active_terminal_selection_text() {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        let active_id = this.workspace().active_tab_id().map(str::to_owned);
                        if let Some(active_id) = active_id {
                            if let Some(tab) = this.terminal_tab_mut(&active_id) {
                                tab.clear_selection();
                            }
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &crate::Paste, window, cx| {
                if window.focused(cx) == Some(this.focus_handle.clone()) {
                    if let Some(clipboard) = cx.read_from_clipboard() {
                        if let Some(text) = clipboard.text() {
                            this.paste_into_terminal(&text, window, cx);
                        }
                    }
                } else {
                    cx.propagate();
                }
            }))
            .when(
                self.active_title_bar_style == crate::session::config::TitleBarStyle::Integrated,
                |this| {
                    this.child(
                        div()
                            .id("title-bar")
                            .flex()
                            .items_center()
                            .h(px(34.))
                            .w_full()
                            .bg(cx.theme().tab_bar)
                            .child(self.render_window_controls(window, cx))
                            .child(
                                div()
                                    .id("tab-bar-drag")
                                    .flex_1()
                                    .min_w(px(0.))
                                    .h_full()
                                    .on_double_click(|_, window, _| {
                                        #[cfg(target_os = "macos")]
                                        window.titlebar_double_click();
                                        #[cfg(not(target_os = "macos"))]
                                        window.zoom_window();
                                    })
                                    .when(cfg!(target_os = "linux"), |this| {
                                        this.on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, _| {
                                                // Don't start window move if the user
                                                // might be initiating a tab drag
                                                if !this.tab_drag.is_pending()
                                                    && !this.tab_drag.is_dragging()
                                                {
                                                    this.should_move_window = true;
                                                }
                                            }),
                                        )
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, _| {
                                                this.should_move_window = false;
                                            }),
                                        )
                                        .on_mouse_down_out(cx.listener(|this, _, _, _| {
                                            this.should_move_window = false;
                                        }))
                                        .on_mouse_move(
                                            cx.listener(|this, _, window, _| {
                                                if this.should_move_window {
                                                    this.should_move_window = false;
                                                    window.start_window_move();
                                                }
                                            }),
                                        )
                                    })
                                    .child(self.render_tab_bar(window.window_handle(), cx)),
                            ),
                    )
                },
            )
            .child(div().flex_1().min_h_0().child(workspace_with_tool_panel))
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .on_prepaint({
                let view = cx.entity().clone();
                move |_, window, cx| {
                    // Cross-window drag hit testing needs screen-space bounds
                    // after layout has been computed, not during render.
                    let handle = window.window_handle();
                    let screen_bounds = match window.window_bounds() {
                        gpui::WindowBounds::Fullscreen(b)
                        | gpui::WindowBounds::Maximized(b)
                        | gpui::WindowBounds::Windowed(b) => b,
                    };
                    let is_window_active = window.is_window_active();

                    view.update(cx, |this, cx| {
                        if this.last_registered_window_bounds != Some(screen_bounds) {
                            this.last_registered_window_bounds = Some(screen_bounds);
                            crate::app::update_window_bounds(handle, screen_bounds);
                        }
                        if is_window_active && !this.was_window_active {
                            crate::app::mark_window_active(handle);
                            this.reconcile_on_window_activation(cx);
                        }
                        this.was_window_active = is_window_active;

                        this.open_pending_dialog(window, cx);
                        this.sync_tool_panel_target(cx);
                        this.sync_sftp_path_input(window, cx);
                        this.sync_sftp_tree_scroll(window, cx);

                        if let Some(active_id) = this.workspace().active_tab_id().map(str::to_owned)
                        {
                            if let Some(scrollbar) = this.terminal_scrollbars.get(&active_id) {
                                if let Some(new_display_offset) =
                                    scrollbar.future_display_offset.take()
                                {
                                    if let Some(tab) = this.terminal_tab_mut(&active_id) {
                                        let current = tab.display_offset();
                                        match new_display_offset.cmp(&current) {
                                            std::cmp::Ordering::Greater => {
                                                tab.scroll_up_by(new_display_offset - current)
                                            }
                                            std::cmp::Ordering::Less => {
                                                tab.scroll_down_by(current - new_display_offset)
                                            }
                                            std::cmp::Ordering::Equal => {}
                                        }
                                    }
                                }
                            }
                            if let Some(snapshot) = this.active_snapshot().as_ref() {
                                if let Some(scrollbar) = this.terminal_scrollbars.get(&active_id) {
                                    scrollbar.update(snapshot, px(this.terminal_line_height()));
                                }
                            }
                        }

                        let current_win_size = window.viewport_size();
                        let size_changed = this.last_window_size != Some(current_win_size);
                        this.last_window_size = Some(current_win_size);

                        let current_sizes = this.workspace_panels.read(cx).sizes().clone();
                        if let Some(current_first_size) = current_sizes.first().copied() {
                            if size_changed {
                                if let Some(target_width) = this.last_sidebar_width {
                                    if current_first_size != target_width {
                                        this.workspace_panels.update(cx, |state, cx| {
                                            state.resize_panel(0, target_width, window, cx);
                                        });
                                    }
                                }
                            } else {
                                this.last_sidebar_width = Some(current_first_size);
                            }
                        }
                    });
                }
            })
            .into_any_element()
    }
}

impl Render for TinyShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.render_workspace_shell(window, cx);
        self.render_root_shell(workspace, window, cx)
    }
}
mod home;
mod monitoring;
mod sftp;
mod sidebar;
#[path = "ui/terminal.rs"]
mod terminal_ui;
mod tool_panel;

#[cfg(test)]
mod tests {
    use super::{WorkspaceShellLayout, workspace_shell_layout};

    #[test]
    fn workspace_shell_layout_follows_sidebar_visibility_and_collapse_state() {
        assert_eq!(
            workspace_shell_layout(false, false),
            WorkspaceShellLayout::Hidden
        );
        assert_eq!(
            workspace_shell_layout(false, true),
            WorkspaceShellLayout::Hidden
        );
        assert_eq!(
            workspace_shell_layout(true, true),
            WorkspaceShellLayout::Collapsed
        );
        assert_eq!(
            workspace_shell_layout(true, false),
            WorkspaceShellLayout::Resizable
        );
    }
}
