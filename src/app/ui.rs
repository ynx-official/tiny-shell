use crate::app::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui::{
    Context, ElementId, Focusable as _, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ParentElement as _, PathBuilder, Pixels, Render,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, hsla, point,
    prelude::FluentBuilder as _, px, relative, rems, uniform_list,
};
use gpui_component::{
    ActiveTheme, Disableable as _, ElementExt, Icon, IconName, InteractiveElementExt as _, Root,
    Selectable as _, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::Input,
    menu::{ContextMenuExt as _, PopupMenuItem},
    progress::Progress,
    scroll::{ScrollableElement as _, Scrollbar, ScrollbarShow},
    tab::{Tab, TabBar},
    v_flex,
};
use rust_i18n::t;
use std::collections::{HashMap, HashSet};

use crate::{
    Ashell, PaneLayout,
    app::constants::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH, TERMINAL_KEY_CONTEXT},
    app::{HomePage, TabContextMenuState},
    sftp::format_mtime,
    sftp::ops::is_editable_text_file,
    system::format_bytes,
    terminal::{self, TabKind},
};

impl Ashell {
    fn render_home_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sessions = self.config.sessions().to_vec();
        let total_connections = sessions.len();
        // All persisted profiles are SSH. SFTP is a runtime handle attached to
        // an active SSH workspace.
        let ssh_connections = sessions.len();
        let sftp_connections = self.sftp_handles.len();
        let mut recent_sessions: Vec<_> = sessions.iter().collect();
        recent_sessions.sort_by(|left, right| right.last_used.cmp(&left.last_used));
        recent_sessions.truncate(3);
        let has_recent_sessions = !recent_sessions.is_empty();

        v_flex()
            .w_full()
            .h_full()
            .p_8()
            .gap_7()
            .child(
                v_flex()
                    .w_full()
                    .items_center()
                    .pt_6()
                    .gap_3()
                    .child(
                        div()
                            .text_size(rems(2.5))
                            .font_weight(FontWeight::BOLD)
                            .child(t!("overview_welcome")),
                    )
                    .child(
                        div()
                            .text_size(rems(1.0))
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("overview_subtitle")),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .pt_2()
                            .child(
                                Button::new("overview-new-connection")
                                    .primary()
                                    .icon(IconName::Plus)
                                    .label(t!("overview_new_connection").to_string())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_new_ssh_dialog(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("overview-open-sessions")
                                    .secondary()
                                    .icon(IconName::Network)
                                    .label(t!("overview_connections").to_string())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.home_page = HomePage::Connections;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_2()
                            .p_4()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().muted)
                            .child(
                                Icon::new(IconName::Network)
                                    .with_size(Size::Medium)
                                    .text_color(cx.theme().primary),
                            )
                            .child(
                                div()
                                    .text_size(rems(1.75))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(total_connections.to_string()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.875))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("overview_total_connections")),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_2()
                            .p_4()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().muted)
                            .child(
                                Icon::new(IconName::SquareTerminal)
                                    .with_size(Size::Medium)
                                    .text_color(cx.theme().chart_2),
                            )
                            .child(
                                div()
                                    .text_size(rems(1.75))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(ssh_connections.to_string()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.875))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("overview_ssh_connections")),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_2()
                            .p_4()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().muted)
                            .child(
                                Icon::new(IconName::Folder)
                                    .with_size(Size::Medium)
                                    .text_color(cx.theme().chart_3),
                            )
                            .child(
                                div()
                                    .text_size(rems(1.75))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(sftp_connections.to_string()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.875))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("overview_sftp_connections")),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        div()
                            .text_size(rems(1.25))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("overview_recent")),
                    )
                    .child(
                        v_flex().w_full().gap_2().children(
                            recent_sessions
                                .into_iter()
                                .enumerate()
                                .map(|(ix, session)| {
                                    let session_to_open = session.clone();
                                    let title = session.name.clone();
                                    let detail = format!(
                                        "{}@{}:{} · {}",
                                        session.user,
                                        session.host,
                                        session.port,
                                        Self::recent_usage_label(session.last_used.as_deref()),
                                    );
                                    div()
                                        .id(("overview-recent", ix))
                                        .w_full()
                                        .p_3()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .bg(cx.theme().muted)
                                        .cursor_pointer()
                                        .hover(|this| this.bg(cx.theme().secondary))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.open_ssh_session(session_to_open.clone(), cx);
                                        }))
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap_3()
                                                .child(
                                                    Icon::new(IconName::Network)
                                                        .with_size(Size::Medium)
                                                        .text_color(cx.theme().primary),
                                                )
                                                .child(
                                                    v_flex()
                                                        .gap_1()
                                                        .child(
                                                            div()
                                                                .font_weight(FontWeight::MEDIUM)
                                                                .child(title),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_size(rems(0.833))
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(detail),
                                                        ),
                                                )
                                                .child(div().flex_1())
                                                .child(
                                                    Icon::new(IconName::ArrowRight)
                                                        .with_size(Size::Small)
                                                        .text_color(cx.theme().muted_foreground),
                                                ),
                                        )
                                }),
                        ),
                    )
                    .when(!has_recent_sessions, |this| {
                        this.child(
                            div()
                                .p_4()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("overview_no_recent")),
                        )
                    }),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        div()
                            .text_size(rems(1.25))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("overview_quick_actions")),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                Button::new("overview-open-commands")
                                    .secondary()
                                    .icon(IconName::SquareTerminal)
                                    .label(t!("command_manager").to_string())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.home_page = HomePage::Commands;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("overview-open-key-manager")
                                    .secondary()
                                    .icon(IconName::Folder)
                                    .label(t!("overview_key_manager").to_string())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.home_page = HomePage::KeyManager;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("overview-open-documentation")
                                    .ghost()
                                    .icon(IconName::Folder)
                                    .label(t!("documentation").to_string())
                                    .on_click(|_, _, _| {
                                        #[cfg(target_os = "windows")]
                                        let _ = std::process::Command::new("explorer")
                                            .arg("README.md")
                                            .spawn();
                                        #[cfg(target_os = "macos")]
                                        let _ = std::process::Command::new("open")
                                            .arg("README.md")
                                            .spawn();
                                        #[cfg(target_os = "linux")]
                                        let _ = std::process::Command::new("xdg-open")
                                            .arg("README.md")
                                            .spawn();
                                    }),
                            )
                            .child(
                                Button::new("overview-settings")
                                    .ghost()
                                    .icon(IconName::Settings)
                                    .label(t!("settings").to_string())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_settings_dialog(window, cx)
                                    })),
                            ),
                    ),
            )
    }

    fn render_command_manager_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_6()
            .gap_5()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(rems(2.0))
                            .font_weight(FontWeight::BOLD)
                            .child(t!("command_manager")),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("command_manager_desc")),
                    ),
            )
            .child(
                v_flex()
                    .max_w(px(680.))
                    .gap_3()
                    .p_5()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().muted)
                    .child(Icon::new(IconName::SquareTerminal).with_size(Size::Large))
                    .child(
                        div()
                            .text_size(rems(1.167))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("command_manager_empty_title")),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("command_manager_empty_desc")),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .pt_2()
                            .child(
                                Button::new("commands-open-connections")
                                    .primary()
                                    .icon(IconName::Network)
                                    .label(t!("overview_connections").to_string())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.home_page = HomePage::Connections;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("commands-new-connection")
                                    .secondary()
                                    .icon(IconName::Plus)
                                    .label(t!("overview_new_connection").to_string())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_new_ssh_dialog(window, cx);
                                    })),
                            ),
                    ),
            )
    }

    fn render_settings_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_6()
            .gap_5()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(rems(2.0))
                            .font_weight(FontWeight::BOLD)
                            .child(t!("settings")),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("settings_page_desc")),
                    ),
            )
            .child(
                h_flex()
                    .max_w(px(760.))
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_2()
                            .p_4()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().muted)
                            .child(Icon::new(IconName::Settings).with_size(Size::Medium))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("settings_general")),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.875))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("settings_page_appearance")),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_2()
                            .p_4()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().muted)
                            .child(Icon::new(IconName::SquareTerminal).with_size(Size::Medium))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("settings_terminal")),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.875))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("settings_page_terminal")),
                            ),
                    ),
            )
            .child(
                Button::new("settings-page-open-editor")
                    .primary()
                    .icon(IconName::Settings)
                    .label(t!("settings_open_editor").to_string())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_settings_dialog(window, cx);
                    })),
            )
    }

    fn recent_usage_label(last_used: Option<&str>) -> String {
        let Some(last_used) = last_used else {
            return t!("overview_not_used").to_string();
        };
        let Ok(used_at) = chrono::DateTime::parse_from_rfc3339(last_used) else {
            return t!("overview_recently_used").to_string();
        };
        let elapsed =
            chrono::Local::now().signed_duration_since(used_at.with_timezone(&chrono::Local));
        if elapsed.num_minutes() < 1 {
            t!("overview_just_now").to_string()
        } else if elapsed.num_hours() < 1 {
            t!("overview_minutes_ago", count = elapsed.num_minutes()).to_string()
        } else if elapsed.num_days() < 1 {
            t!("overview_hours_ago", count = elapsed.num_hours()).to_string()
        } else {
            t!("overview_days_ago", count = elapsed.num_days()).to_string()
        }
    }

    fn render_connection_manager_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let all_sessions = self.config.sessions().to_vec();
        let selected_group = self.connection_group_filter.clone();
        let mut groups = self.config.connection_groups().to_vec();
        for session in &all_sessions {
            if let Some(group) = &session.group
                && !groups.iter().any(|candidate| candidate == group)
            {
                groups.push(group.clone());
            }
        }
        let mut seen = HashSet::new();
        groups.retain(|group| seen.insert(group.clone()));
        groups = Self::connection_group_tree_order(groups);
        let groups_for_rows = groups.clone();

        let mut sessions: Vec<_> = all_sessions
            .iter()
            .filter(|session| {
                selected_group.as_deref().is_none_or(|group| {
                    let prefix = format!("{group}/");
                    session.group.as_deref().is_some_and(|session_group| {
                        session_group == group || session_group.starts_with(&prefix)
                    })
                })
            })
            .cloned()
            .collect();
        sessions.sort_by(|left, right| right.last_used.cmp(&left.last_used));
        let has_sessions = !sessions.is_empty();

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .flex_none()
                    .h(px(60.))
                    .items_center()
                    .gap_2()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(rems(2.0))
                                    .font_weight(FontWeight::BOLD)
                                    .child(t!("overview_connections")),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.917))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("overview_connections_desc")),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("connection-manager-new-group")
                            .secondary()
                            .icon(IconName::FolderOpen)
                            .label(t!("connection_group_new").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_connection_group_dialog(None, None, window, cx);
                            })),
                    )
                    .child(
                        Button::new("connection-manager-new")
                            .primary()
                            .icon(IconName::Plus)
                            .label(t!("overview_new_connection").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_new_ssh_dialog(window, cx)
                            })),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .h_full()
                    .items_stretch()
                    .gap_0()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .overflow_hidden()
                    .child(
                        v_flex()
                            .h_full()
                            .w(px(210.))
                            .flex_none()
                            .p_2()
                            .gap_1()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().sidebar)
                            .child(
                                div()
                                    .id("connection-group-all")
                                    .w_full()
                                    .cursor_pointer()
                                    .rounded_md()
                                    .bg(if selected_group.is_none() {
                                        cx.theme().tab_active
                                    } else {
                                        cx.theme().sidebar
                                    })
                                    .hover(|this| this.bg(cx.theme().secondary))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.connection_group_filter = None;
                                        cx.notify();
                                    }))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .p_3()
                                            .child(
                                                Icon::new(IconName::Network).with_size(Size::Small),
                                            )
                                            .child(div().flex_1().child(t!("connection_group_all")))
                                            .child(
                                                div()
                                                    .text_size(rems(0.8))
                                                    .text_color(cx.theme().muted_foreground)
                                        .child(all_sessions.len().to_string()),
                                            ),
                                    ),
                            )
                            .children(groups.into_iter().enumerate().map(|(ix, group)| {
                                let group_name = group.clone();
                                let group_prefix = format!("{group}/");
                                let count = all_sessions
                                    .iter()
                                    .filter(|session| {
                                        session.group.as_deref().is_some_and(|session_group| {
                                            session_group == group || session_group.starts_with(&group_prefix)
                                        })
                                    })
                                    .count();
                                let selected = selected_group.as_deref() == Some(group.as_str());
                                let dragging = self.dragging_connection_group.as_deref() == Some(group.as_str());
                                let show_drop_before = self
                                    .connection_group_drop_before
                                    .as_deref()
                                    == Some(group.as_str());
                                let depth = group.matches('/').count();
                                let group_label = group.rsplit('/').next().unwrap_or(&group).to_string();
                                div()
                                    .id(("connection-group", ix))
                                    .relative()
                                    .w_full()
                                    .cursor_pointer()
                                    .rounded_md()
                                    .bg(if selected {
                                        cx.theme().tab_active
                                    } else {
                                        cx.theme().sidebar
                                    })
                                    .hover(|this| this.bg(cx.theme().secondary))
                                    .on_prepaint({
                                        let group_name = group_name.clone();
                                        let view = cx.entity();
                                        move |bounds, _window, cx| {
                                            view.update(cx, |this, _| {
                                                this.connection_group_bounds
                                                    .insert(group_name.clone(), bounds);
                                            });
                                        }
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener({
                                            let group_name = group_name.clone();
                                            move |this, event: &MouseDownEvent, _, _| {
                                                this.pending_connection_group_drag =
                                                    Some((group_name.clone(), event.position));
                                            }
                                        }),
                                    )
                                    .on_click(cx.listener({
                                        let group_name = group_name.clone();
                                        move |this, _, _, cx| {
                                            this.connection_group_filter = Some(group_name.clone());
                                            cx.notify();
                                        }
                                    }))
                                    .context_menu({
                                        let view = cx.entity();
                                        let group_name = group_name.clone();
                                        move |mut menu, window, _| {
                                            menu = menu.item(
                                                PopupMenuItem::new(
                                                    t!("connection_group_rename").to_string(),
                                                )
                                                .on_click(window.listener_for(&view, {
                                                    let group_name = group_name.clone();
                                                    move |this, _, window, cx| {
                                                        this.show_connection_group_dialog(
                                                            Some(group_name.clone()),
                                                            None,
                                                            window,
                                                            cx,
                                                        );
                                                    }
                                                })),
                                            )
                                            .item(
                                                PopupMenuItem::new(t!("connection_group_new_child").to_string())
                                                    .on_click(window.listener_for(&view, {
                                                        let group_name = group_name.clone();
                                                        move |this, _, window, cx| {
                                                            this.show_connection_group_dialog(None, Some(group_name.clone()), window, cx);
                                                        }
                                                    })),
                                            )
                                            .item(
                                                PopupMenuItem::new(t!("connection_group_delete").to_string())
                                                    .on_click(window.listener_for(&view, {
                                                        let group_name = group_name.clone();
                                                        move |this, _, _, cx| {
                                                            this.config.remove_connection_group(&group_name);
                                                            if let Err(err) = this.config.save() {
                                                                tracing::warn!("failed to remove connection group: {err:#}");
                                                            }
                                                            if this.connection_group_filter.as_deref() == Some(group_name.as_str()) {
                                                                this.connection_group_filter = None;
                                                            }
                                                            cx.notify();
                                                        }
                                                    })),
                                            )
                                            .item(
                                                PopupMenuItem::new(t!("connection_group_move_to").to_string())
                                                    .on_click(window.listener_for(&view, {
                                                        let group_name = group_name.clone();
                                                        move |this, _, window, cx| {
                                                            this.show_move_connection_group_dialog(group_name.clone(), window, cx);
                                                        }
                                                    })),
                                            )
                                            ;
                                            menu
                                        }
                                    })
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .p_3()
                                            .pl(px(12. + depth as f32 * 14.))
                                            .child(
                                                Icon::new(IconName::Folder).with_size(Size::Small),
                                            )
                                            .child(div().flex_1().child(group_label))
                                            .child(
                                                div()
                                                    .text_size(rems(0.8))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(count.to_string()),
                                            ),
                                    )
                                    .when(show_drop_before, |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .right_0()
                                                .h(px(2.))
                                                .bg(cx.theme().primary),
                                        )
                                    })
                                    .when(dragging, |this| this.opacity(0.55))
                            })),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .min_w(px(0.))
                            .bg(cx.theme().background)
                            .overflow_hidden()
                            .child(
                                h_flex()
                                    .flex_none()
                                    .items_center()
                                    .h(px(38.))
                                    .px_4()
                                    .gap_3()
                                    .bg(cx.theme().muted)
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div().flex_1().text_size(rems(0.833)).child(t!("session")),
                                    )
                                    .child(
                                        div().w(px(150.)).text_size(rems(0.833)).child(t!("host")),
                                    )
                                    .child(
                                        div()
                                            .w(px(130.))
                                            .text_size(rems(0.833))
                                            .child(t!("overview_recent")),
                                    )
                                    .child(
                                        div()
                                            .w(px(180.))
                                            .text_size(rems(0.833))
                                            .child(t!("overview_actions")),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .overflow_y_scrollbar()
                                    .children(sessions.into_iter().enumerate().map(
                                        |(ix, session)| {
                                            let connect_session = session.clone();
                                            let edit_id = session.id.clone();
                                            let delete_id = session.id.clone();
                                            let session_id = session.id.clone();
                                            let groups_for_menu = groups_for_rows.clone();
                                            let host = format!("{}:{}", session.host, session.port);
                                            let recent = Self::recent_usage_label(
                                                session.last_used.as_deref(),
                                            );
                                            h_flex()
                                                .id(("connection-manager-row", ix))
                                                .flex_none()
                                                .items_center()
                                                .min_h(px(58.))
                                                .px_4()
                                                .gap_3()
                                                .border_b_1()
                                                .border_color(cx.theme().border)
                                                .hover(|this| this.bg(cx.theme().muted))
                                                .context_menu({
                                                    let view = cx.entity();
                                                    move |mut menu, window, _| {
                                                        let session_id = session_id.clone();
                                                        menu = menu.item(
                                                            PopupMenuItem::new(
                                                                t!("connection_group_ungrouped")
                                                                    .to_string(),
                                                            )
                                                            .on_click(window.listener_for(&view, {
                                                                let session_id = session_id.clone();
                                                                move |this, _, _, cx| {
                                                                    if let Some(mut session) = this
                                                                        .config
                                                                        .get(&session_id)
                                                                        .cloned()
                                                                    {
                                                                        session.group = None;
                                                                        this.config.upsert(session);
                                                                        let _ = this.config.save();
                                                                        cx.notify();
                                                                    }
                                                                }
                                                            })),
                                                        );
                                                        for group in &groups_for_menu {
                                                            let group_name = group.clone();
                                                            let session_id = session_id.clone();
                                                            menu = menu.item(
                                                                PopupMenuItem::new(format!(
                                                                    "{} {}",
                                                                    t!("connection_group_move_to"),
                                                                    group_name
                                                                ))
                                                                .on_click(window.listener_for(
                                                                    &view,
                                                                    move |this, _, _, cx| {
                                                                        if let Some(mut session) =
                                                                            this.config
                                                                                .get(&session_id)
                                                                                .cloned()
                                                                        {
                                                                            session.group = Some(
                                                                                group_name.clone(),
                                                                            );
                                                                            this.config
                                                                                .upsert(session);
                                                                            let _ =
                                                                                this.config.save();
                                                                            cx.notify();
                                                                        }
                                                                    },
                                                                )),
                                                            );
                                                        }
                                                        menu
                                                    }
                                                })
                                                .child(
                                                    h_flex()
                                                        .flex_1()
                                                        .items_center()
                                                        .gap_2()
                                                        .child(
                                                            Icon::new(IconName::Network)
                                                                .with_size(Size::Small),
                                                        )
                                                        .child(
                                                            v_flex()
                                                                .gap_1()
                                                                .child(
                                                                    div()
                                                                        .font_weight(
                                                                            FontWeight::MEDIUM,
                                                                        )
                                                                        .child(session.name),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(rems(0.8))
                                                                        .text_color(
                                                                            cx.theme()
                                                                                .muted_foreground,
                                                                        )
                                                                        .child(session.user),
                                                                ),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(150.))
                                                        .text_size(rems(0.833))
                                                        .child(host),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(130.))
                                                        .text_size(rems(0.8))
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(recent),
                                                )
                                                .child(
                                                    h_flex()
                                                        .w(px(180.))
                                                        .justify_end()
                                                        .gap_1()
                                                        .child(
                                                            Button::new(format!(
                                                                "connection-manager-connect-{ix}"
                                                            ))
                                                            .small()
                                                            .primary()
                                                            .label(t!("connect").to_string())
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    this.open_ssh_session(
                                                                        connect_session.clone(),
                                                                        cx,
                                                                    )
                                                                },
                                                            )),
                                                        )
                                                        .child(
                                                            Button::new(format!(
                                                                "connection-manager-edit-{ix}"
                                                            ))
                                                            .ghost()
                                                            .small()
                                                            .label(t!("edit").to_string())
                                                            .on_click(cx.listener(
                                                                move |this, _, window, cx| {
                                                                    this.edit_saved_session(
                                                                        edit_id.clone(),
                                                                        window,
                                                                        cx,
                                                                    )
                                                                },
                                                            )),
                                                        )
                                                        .child(
                                                            Button::new(format!(
                                                                "connection-manager-delete-{ix}"
                                                            ))
                                                            .ghost()
                                                            .small()
                                                            .icon(IconName::Delete)
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    this.remove_saved_session(
                                                                        delete_id.clone(),
                                                                        cx,
                                                                    )
                                                                },
                                                            )),
                                                        ),
                                                )
                                        },
                                    ))
                                    .when(!has_sessions, |this| {
                                        this.child(
                                            v_flex()
                                                .size_full()
                                                .items_center()
                                                .justify_center()
                                                .gap_3()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(
                                                    Icon::new(IconName::Network)
                                                        .with_size(Size::Large),
                                                )
                                                .child(t!("overview_no_connections")),
                                        )
                                    }),
                            ),
                    ),
            )
    }

    /// Render a stable tree: group creation order defines sibling order while
    /// descendants remain directly below their parent.
    fn connection_group_tree_order(groups: Vec<String>) -> Vec<String> {
        let present = groups.iter().cloned().collect::<HashSet<_>>();
        let mut children: HashMap<Option<String>, Vec<String>> = HashMap::new();
        for group in groups {
            let parent = group
                .rsplit_once('/')
                .map(|(parent, _)| parent.to_string())
                .filter(|parent| present.contains(parent));
            children.entry(parent).or_default().push(group);
        }

        let mut ordered = Vec::new();
        let mut stack = children
            .remove(&None)
            .unwrap_or_default()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();
        while let Some(group) = stack.pop() {
            if let Some(child_groups) = children.remove(&Some(group.clone())) {
                stack.extend(child_groups.into_iter().rev());
            }
            ordered.push(group);
        }
        ordered
    }

    fn render_key_manager_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let keys = self.managed_keys.clone();
        let sessions = self.config.sessions().to_vec();
        let has_keys = !keys.is_empty();
        let rename_input = self.key_inline_input.clone();

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .flex_none()
                    .h(px(60.))
                    .items_center()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(rems(2.0))
                                    .font_weight(FontWeight::BOLD)
                                    .child(t!("overview_key_manager")),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.917))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("key_management_desc")),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("key-manager-import")
                            .primary()
                            .icon(IconName::Plus)
                            .label(t!("import_key").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.import_managed_key(window, cx)
                            })),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.))
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .flex_none()
                            .items_center()
                            .h(px(38.))
                            .px_4()
                            .gap_3()
                            .bg(cx.theme().muted)
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(div().flex_1().text_size(rems(0.833)).child(t!("key_name")))
                            .child(
                                div()
                                    .w(px(110.))
                                    .text_size(rems(0.833))
                                    .child(t!("key_type")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(rems(0.833))
                                    .child(t!("key_fingerprint")),
                            )
                            .child(
                                div()
                                    .w(px(140.))
                                    .text_size(rems(0.833))
                                    .child(t!("overview_actions")),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scrollbar()
                            .children(keys.into_iter().enumerate().map(|(ix, key)| {
                                let key_id = key.id.clone();
                                let key_name = key.name.clone();
                                let key_type = if key.key_type.is_empty() {
                                    "unknown".to_string()
                                } else {
                                    key.key_type.clone()
                                };
                                let fingerprint = if key.fingerprint.len() > 34 {
                                    format!("{}…", &key.fingerprint[..34])
                                } else {
                                    key.fingerprint.clone()
                                };
                                let is_editing =
                                    self.editing_managed_key_id.as_deref() == Some(key_id.as_str());
                                let rename_input = rename_input.clone();
                                let reference_count = sessions
                                    .iter()
                                    .filter(|session| {
                                        session.managed_key_id.as_deref() == Some(key_id.as_str())
                                    })
                                    .count();
                                let imported_at =
                                    chrono::DateTime::from_timestamp(key.created_at, 0)
                                        .map(|date| date.format("%Y-%m-%d").to_string())
                                        .unwrap_or_else(|| "-".to_string());

                                h_flex()
                                    .id(("key-manager-row", ix))
                                    .flex_none()
                                    .items_center()
                                    .min_h(px(58.))
                                    .px_4()
                                    .gap_3()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .hover(|this| this.bg(cx.theme().muted))
                                    .child(if is_editing {
                                        div()
                                            .flex_1()
                                            .child(Input::new(&rename_input).small())
                                            .into_any_element()
                                    } else {
                                        h_flex()
                                            .flex_1()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                Icon::new(IconName::Folder).with_size(Size::Small),
                                            )
                                            .child(
                                                v_flex()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .child(key_name.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(rems(0.72))
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(format!(
                                                                "{} · {} {}",
                                                                imported_at,
                                                                reference_count,
                                                                t!("overview_key_references")
                                                            )),
                                                    ),
                                            )
                                            .into_any_element()
                                    })
                                    .child(div().w(px(110.)).text_size(rems(0.833)).child(key_type))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(rems(0.833))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(fingerprint),
                                    )
                                    .child(
                                        h_flex()
                                            .w(px(140.))
                                            .justify_end()
                                            .gap_1()
                                            .child(if is_editing {
                                                Button::new(format!("key-manager-save-{key_id}"))
                                                    .ghost()
                                                    .small()
                                                    .icon(IconName::Check)
                                                    .on_click({
                                                        let key_id = key_id.clone();
                                                        let rename_input = rename_input.clone();
                                                        cx.listener(move |this, _, _, cx| {
                                                            let new_name = rename_input
                                                                .read(cx)
                                                                .value()
                                                                .trim()
                                                                .to_string();
                                                            if !new_name.is_empty() {
                                                                this.rename_managed_key(
                                                                    key_id.clone(),
                                                                    new_name,
                                                                    cx,
                                                                );
                                                            }
                                                        })
                                                    })
                                            } else {
                                                Button::new(format!("key-manager-rename-{key_id}"))
                                                    .ghost()
                                                    .small()
                                                    .label(t!("key_rename").to_string())
                                                    .on_click({
                                                        let key_id = key_id.clone();
                                                        let key_name = key_name.clone();
                                                        cx.listener(move |this, _, window, cx| {
                                                            this.editing_managed_key_id =
                                                                Some(key_id.clone());
                                                            Self::set_input_value(
                                                                &this.key_inline_input,
                                                                key_name.clone(),
                                                                window,
                                                                cx,
                                                            );
                                                            cx.notify();
                                                        })
                                                    })
                                            })
                                            .child(
                                                Button::new(format!("key-manager-delete-{key_id}"))
                                                    .ghost()
                                                    .small()
                                                    .icon(IconName::Delete)
                                                    .on_click({
                                                        let key_id = key_id.clone();
                                                        cx.listener(move |this, _, _, cx| {
                                                            this.delete_managed_key(
                                                                key_id.clone(),
                                                                cx,
                                                            )
                                                        })
                                                    }),
                                            ),
                                    )
                            }))
                            .when(!has_keys, |this| {
                                this.child(
                                    v_flex()
                                        .size_full()
                                        .items_center()
                                        .justify_center()
                                        .gap_3()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(Icon::new(IconName::Folder).with_size(Size::Large))
                                        .child(t!("no_managed_keys")),
                                )
                            }),
                    ),
            )
    }

    fn render_overview_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_overview = self.home_page == HomePage::Overview;
        let is_connections = self.home_page == HomePage::Connections;
        let is_commands = self.home_page == HomePage::Commands;
        let is_key_manager = self.home_page == HomePage::KeyManager;
        let is_settings = self.home_page == HomePage::Settings;
        v_flex()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .p_4()
            .gap_5()
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_size(rems(1.5))
                            .text_color(cx.theme().primary)
                            .child("Ashell"),
                    )
                    .child(
                        div()
                            .text_size(rems(0.75))
                            .text_color(cx.theme().muted_foreground)
                            .child("WORKSPACE"),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("overview-sidebar-settings")
                            .ghost()
                            .icon(IconName::Settings)
                            .tooltip(t!("settings_open_settings").to_string())
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.home_page = HomePage::Settings;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .id("overview-sidebar-overview")
                            .w_full()
                            .cursor_pointer()
                            .hover(|this| this.bg(cx.theme().secondary))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.home_page = HomePage::Overview;
                                cx.notify();
                            }))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_3()
                                    .p_3()
                                    .rounded_md()
                                    .bg(if is_overview {
                                        cx.theme().tab_active
                                    } else {
                                        cx.theme().sidebar
                                    })
                                    .text_color(if is_overview {
                                        cx.theme().primary
                                    } else {
                                        cx.theme().foreground
                                    })
                                    .child(
                                        Icon::new(IconName::SquareTerminal).with_size(Size::Small),
                                    )
                                    .child(
                                        div().font_weight(FontWeight::MEDIUM).child(t!("overview")),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("overview-sidebar-connections")
                            .w_full()
                            .cursor_pointer()
                            .rounded_md()
                            .hover(|this| this.bg(cx.theme().secondary))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.home_page = HomePage::Connections;
                                cx.notify();
                            }))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_3()
                                    .p_3()
                                    .bg(if is_connections {
                                        cx.theme().tab_active
                                    } else {
                                        cx.theme().sidebar
                                    })
                                    .text_color(if is_connections {
                                        cx.theme().primary
                                    } else {
                                        cx.theme().foreground
                                    })
                                    .child(Icon::new(IconName::Network).with_size(Size::Small))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(t!("overview_connections")),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("overview-sidebar-commands")
                            .w_full()
                            .cursor_pointer()
                            .rounded_md()
                            .hover(|this| this.bg(cx.theme().secondary))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.home_page = HomePage::Commands;
                                cx.notify();
                            }))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_3()
                                    .p_3()
                                    .bg(if is_commands {
                                        cx.theme().tab_active
                                    } else {
                                        cx.theme().sidebar
                                    })
                                    .text_color(if is_commands {
                                        cx.theme().primary
                                    } else {
                                        cx.theme().foreground
                                    })
                                    .child(
                                        Icon::new(IconName::SquareTerminal).with_size(Size::Small),
                                    )
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(t!("command_manager")),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("overview-sidebar-key-management")
                            .w_full()
                            .cursor_pointer()
                            .rounded_md()
                            .hover(|this| this.bg(cx.theme().secondary))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.home_page = HomePage::KeyManager;
                                cx.notify();
                            }))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_3()
                                    .p_3()
                                    .bg(if is_key_manager {
                                        cx.theme().tab_active
                                    } else {
                                        cx.theme().sidebar
                                    })
                                    .text_color(if is_key_manager {
                                        cx.theme().primary
                                    } else {
                                        cx.theme().foreground
                                    })
                                    .child(Icon::new(IconName::Folder).with_size(Size::Small))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(t!("overview_key_manager")),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("overview-sidebar-settings-link")
                            .w_full()
                            .cursor_pointer()
                            .rounded_md()
                            .hover(|this| this.bg(cx.theme().secondary))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.home_page = HomePage::Settings;
                                cx.notify();
                            }))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_3()
                                    .p_3()
                                    .text_color(if is_settings {
                                        cx.theme().primary
                                    } else {
                                        cx.theme().foreground
                                    })
                                    .bg(if is_settings {
                                        cx.theme().tab_active
                                    } else {
                                        cx.theme().sidebar
                                    })
                                    .child(Icon::new(IconName::Settings).with_size(Size::Small))
                                    .child(
                                        div().font_weight(FontWeight::MEDIUM).child(t!("settings")),
                                    ),
                            ),
                    ),
            )
            .child(div().flex_1())
            .child(
                v_flex()
                    .gap_2()
                    .pt_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("overview-sidebar-new-ssh")
                            .primary()
                            .icon(IconName::Plus)
                            .label(t!("overview_new_connection").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_new_ssh_dialog(window, cx)
                            })),
                    )
                    .child(
                        div()
                            .text_size(rems(0.75))
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} {}",
                                self.config.sessions().len(),
                                t!("overview_saved_count")
                            )),
                    ),
            )
    }

    pub(crate) fn toggle_sftp_minimized(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.body_panels.clone();
        let minimized = self.sftp_panel_minimized;

        if !minimized {
            let sizes = state.read(cx).sizes();
            if sizes.len() > 1 {
                self.prev_monitoring_size = Some(sizes[1]);
            }
            self.sftp_panel_minimized = true;
        } else {
            self.sftp_panel_minimized = false;
            let prev_size = self.prev_monitoring_size.unwrap_or(px(328.));

            cx.on_next_frame(
                window,
                move |_this: &mut crate::app::Ashell,
                      window: &mut gpui::Window,
                      cx: &mut gpui::Context<crate::app::Ashell>| {
                    cx.on_next_frame(
                        window,
                        move |this: &mut crate::app::Ashell,
                              window: &mut gpui::Window,
                              cx: &mut gpui::Context<crate::app::Ashell>| {
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
        self.config
            .set_sftp_panel_minimized(self.sftp_panel_minimized);
        let _ = self.config.save();
        cx.notify();
    }

    fn render_sftp_panel(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active_sftp = self.active_sftp();

        // Compute active download progress for status bar and minimized header
        let build_summary = |kind: crate::terminal::TransferType| -> Option<(String, String, f32)> {
            let active: Vec<&crate::terminal::Transfer> = self
                .transfers
                .iter()
                .filter(|t| {
                    matches!(
                        t.state,
                        crate::terminal::TransferState::Running
                            | crate::terminal::TransferState::Paused
                    ) && t.info.kind == kind
                })
                .collect();
            if active.is_empty() {
                return None;
            }
            Some(if active.len() == 1 {
                let t = &active[0];
                let pct = t.total.and_then(|total| {
                    if total > 0 {
                        Some((t.transferred as f64 / total as f64 * 100.0) as f32)
                    } else {
                        None
                    }
                });
                match pct {
                    Some(pct) => (t.info.name.clone(), format!("{:.0}%", pct), pct),
                    None => (t.info.name.clone(), "-".to_string(), 0.0),
                }
            } else {
                let total_transferred: u64 = active.iter().map(|t| t.transferred).sum();
                let total_total: u64 = active.iter().filter_map(|t| t.total).sum();
                let pct = if total_total > 0 {
                    Some((total_transferred as f64 / total_total as f64 * 100.0) as f32)
                } else {
                    None
                };
                let label = match kind {
                    crate::terminal::TransferType::Download => {
                        t!("files_downloading", count = active.len()).to_string()
                    }
                    crate::terminal::TransferType::Upload => {
                        t!("files_uploading", count = active.len()).to_string()
                    }
                };
                match pct {
                    Some(pct) => (label, format!("{:.0}%", pct), pct),
                    None => (label, "-".to_string(), 0.0),
                }
            })
        };
        let dl_summary = build_summary(crate::terminal::TransferType::Download);
        let ul_summary = build_summary(crate::terminal::TransferType::Upload);
        let has_transfers = dl_summary.is_some() || ul_summary.is_some();

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
                div()
                    .text_size(rems(1.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().primary)
                    .child(t!("remote_files")),
            )
            .child(div().flex_1())
            .when_some(active_sftp, |this, sftp| {
                let selected_entries = sftp.selected_entries.clone();
                this.child(
                    Button::new("sftp-sync-cwd")
                        .ghost()
                        .small()
                        .selected(sftp.follow_terminal_cwd)
                        .icon(IconName::SquareTerminal)
                        .label(t!("sync_cwd").to_string())
                        .tooltip(if sftp.follow_terminal_cwd {
                            t!("sync_cwd_enabled_tooltip").to_string()
                        } else {
                            t!("sync_cwd_tooltip").to_string()
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_follow_terminal_cwd(window, cx);
                        })),
                )
                .child(
                    Button::new("sftp-refresh")
                        .ghost()
                        .small()
                        .icon(IconName::ArrowRight)
                        .label(t!("refresh").to_string())
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_sftp(cx))),
                )
                .child(
                    Button::new("sftp-new-folder")
                        .ghost()
                        .small()
                        .icon(IconName::Folder)
                        .label(t!("new_folder").to_string())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.sftp_creating_folder = true;
                            this.sftp_new_folder_input.update(cx, |input, cx| {
                                input.set_value("", window, cx);
                                input.focus_handle(cx).focus(window, cx);
                            });
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("sftp-delete-selected")
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .label(if selected_entries.is_empty() {
                            t!("delete_selected").to_string()
                        } else {
                            format!("{} ({})", t!("delete_selected"), selected_entries.len())
                        })
                        .disabled(selected_entries.is_empty())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.show_delete_confirm_dialog(window, cx);
                        })),
                )
                .child(
                    Button::new("sftp-upload-file")
                        .ghost()
                        .small()
                        .icon(IconName::Plus)
                        .label(t!("upload_file").to_string())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.upload_sftp_files(window, cx)),
                        ),
                )
                .child(
                    Button::new("sftp-upload-folder")
                        .ghost()
                        .small()
                        .icon(IconName::Folder)
                        .label(t!("upload_folder").to_string())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.upload_sftp_folder(window, cx)),
                        ),
                )
                .child(
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
                .child(
                    Checkbox::new("sftp-show-hidden")
                        .small()
                        .label(t!("hidden").to_string())
                        .checked(self.show_hidden_files)
                        .tab_stop(false)
                        .on_click(cx.listener(|this, checked, _, cx| {
                            this.show_hidden_files = *checked;
                            this.config.set_show_hidden_files(*checked);
                            let _ = this.config.save();
                            cx.notify();
                        })),
                )
            });
        let Some(sftp) = active_sftp else {
            let mut outer = v_flex()
                .gap_0()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .flex_1()
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
            outer = outer.child(
                h_flex()
                    .flex_none()
                    .h(px(24.))
                    .px_3()
                    .items_center()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().tab_bar)
                    .child(div().flex_1())
                    .child(
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
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_transfers_dialog(window, cx);
                                    }))
                            })
                            .when(!has_transfers, |this| {
                                this.icon(IconName::ArrowDown)
                                    .label(t!("transfers").to_string())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_transfers_dialog(window, cx);
                                    }))
                            }),
                    )
                    .child(
                        Button::new("sftp-minimize-toggle")
                            .ghost()
                            .small()
                            .icon(if self.sftp_panel_minimized {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            })
                            .label(if self.sftp_panel_minimized {
                                t!("panel_expand").to_string()
                            } else {
                                t!("panel_minimize").to_string()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_sftp_minimized(window, cx);
                            })),
                    ),
            );
            return outer.into_any_element();
        };

        let selected_path = sftp.selected_path.clone();
        let entries = sftp
            .entries
            .clone()
            .into_iter()
            .filter(|entry| self.show_hidden_files || !entry.name.starts_with('.'))
            .collect::<Vec<_>>();
        let status = sftp.status.clone();
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
                                        .on_click(cx.listener(move |this, checked, _, cx| {
                                            this.toggle_all_sftp_entries(*checked, cx);
                                        })),
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
                                            let is_checked =
                                                selected_entries.contains(&entry.full_path);
                                            let is_selected = selected_path.as_deref()
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
                                                            // 双击受支持的文本文件 → 内置编辑器
                                                            if event.click_count >= 2
                                                                && !entry.is_dir
                                                                && is_editable_text_file(
                                                                    &entry.full_path,
                                                                )
                                                            {
                                                                this.open_file_in_editor(
                                                                    entry.full_path.clone(),
                                                                    cx,
                                                                );
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
                                                                remote_path.clone(),
                                                                entry.is_dir,
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
                                                        .on_mouse_down(
                                                            MouseButton::Right,
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
                                    Scrollbar::vertical(&self.remote_files_scroll_handle)
                                        .scrollbar_show(ScrollbarShow::Always),
                                ),
                        ),
                ),
        );
        outer = outer.child(
            h_flex()
                .flex_none()
                .h(px(24.))
                .px_3()
                .items_center()
                .border_t_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().tab_bar)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .text_size(rems(0.833))
                        .text_color(cx.theme().primary)
                        .italic()
                        .child(status),
                )
                .child(
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
                .child(
                    Button::new("sftp-minimize-toggle")
                        .ghost()
                        .small()
                        .icon(if self.sftp_panel_minimized {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .label(if self.sftp_panel_minimized {
                            t!("panel_expand").to_string()
                        } else {
                            t!("panel_minimize").to_string()
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_sftp_minimized(window, cx);
                        })),
                ),
        );

        outer.into_any_element()
    }

    fn render_monitoring_panel(
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

    fn render_sidebar_monitoring_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cpu_pct = self.system.cpu_percent;
        let mem_pct = self.system.mem_percent;
        let swap_pct = self.system.swap_percent;

        let cpu_color = cx.theme().chart_1;
        let mem_color = cx.theme().chart_2;
        let swap_color = cx.theme().chart_3;
        let disk_color = cx.theme().chart_5;
        let net_color = cx.theme().chart_4;
        let muted_fg = cx.theme().muted_foreground;

        v_flex()
            .gap_4()
            .w_full()
            .p_2()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(rems(0.85))
                                    .text_color(cpu_color)
                                    .child(t!("cpu").to_string()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.85))
                                    .text_color(muted_fg)
                                    .child(format!("{:.1}%", cpu_pct * 100.0)),
                            ),
                    )
                    .child(
                        Progress::new("sidebar-cpu")
                            .value(cpu_pct * 100.0)
                            .color(cpu_color)
                            .with_size(px(4.))
                            .w_full(),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(rems(0.85))
                                    .text_color(mem_color)
                                    .child(t!("mem").to_string()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.85))
                                    .text_color(muted_fg)
                                    .child(self.system.mem_detail.clone()),
                            ),
                    )
                    .child(
                        Progress::new("sidebar-mem")
                            .value(mem_pct * 100.0)
                            .color(mem_color)
                            .with_size(px(4.))
                            .w_full(),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(rems(0.85))
                                    .text_color(swap_color)
                                    .child(t!("swap").to_string()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.85))
                                    .text_color(muted_fg)
                                    .child(self.system.swap_detail.clone()),
                            ),
                    )
                    .child(
                        Progress::new("sidebar-swap")
                            .value(swap_pct * 100.0)
                            .color(swap_color)
                            .with_size(px(4.))
                            .w_full(),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_size(rems(0.85))
                                    .text_color(disk_color)
                                    .child(t!("disk").to_string()),
                            )
                            .children(if self.system.disks.len() > 3 {
                                Some(
                                    div()
                                        .text_size(rems(0.65))
                                        .text_color(muted_fg)
                                        .child(t!("scroll").to_string()),
                                )
                            } else {
                                None
                            }),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .child(
                                v_flex()
                                    .id("sidebar-disk-scroll")
                                    .track_scroll(&self.disk_scroll_handle)
                                    .overflow_y_scroll()
                                    .max_h(px(90.))
                                    .gap_2()
                                    .children(self.system.disks.iter().map(|disk| {
                                        let pct = if disk.total_bytes > 0 {
                                            (disk.total_bytes - disk.available_bytes) as f64
                                                / disk.total_bytes as f64
                                                * 100.0
                                        } else {
                                            0.0
                                        };
                                        let mount_short = disk.mount.clone();
                                        let mount_id = format!("sidebar-disk-{}", mount_short);
                                        v_flex()
                                            .gap_0p5()
                                            .child(
                                                h_flex()
                                                    .justify_between()
                                                    .child(
                                                        div()
                                                            .text_size(rems(0.75))
                                                            .text_color(muted_fg)
                                                            .child(mount_short),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(rems(0.75))
                                                            .text_color(muted_fg)
                                                            .child(format!("{:.1}%", pct)),
                                                    ),
                                            )
                                            .child(
                                                Progress::new(mount_id)
                                                    .value(pct as f32)
                                                    .color(disk_color)
                                                    .with_size(px(4.))
                                                    .w_full(),
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
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(rems(0.85))
                                    .text_color(net_color)
                                    .child(t!("net").to_string()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.85))
                                    .text_color(muted_fg)
                                    .child(t!("live")),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(rems(0.75))
                                            .text_color(net_color)
                                            .child("↓"),
                                    )
                                    .child(
                                        div()
                                            .text_size(rems(0.75))
                                            .child(self.system.net_rx.clone()),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(rems(0.75))
                                            .text_color(cx.theme().chart_5)
                                            .child("↑"),
                                    )
                                    .child(
                                        div()
                                            .text_size(rems(0.75))
                                            .child(self.system.net_tx.clone()),
                                    ),
                            ),
                    ),
            )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sessions = self.config.sessions().to_vec();
        let active_session_id = self.active_session_id().map(ToOwned::to_owned);

        v_flex()
            .gap_4()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .p_4()
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .bg(cx.theme().sidebar)
            .overflow_hidden()
            .child(
                v_flex()
                    .gap_1()
                    .min_w(px(0.))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(rems(1.667))
                                    .text_color(cx.theme().primary)
                                    .child("Ashell"),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("sidebar-collapse-toggle")
                                    .ghost()
                                    .icon(IconName::PanelLeftClose)
                                    .tooltip(t!("settings_toggle_sidebar").to_string())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.sidebar_collapsed = true;
                                        this.config.set_sidebar_collapsed(true);
                                        let _ = this.config.save();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("sidebar-settings")
                                    .ghost()
                                    .icon(IconName::Settings)
                                    .tooltip(t!("settings_open_settings").to_string())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_settings_dialog(window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_size(rems(0.917))
                            .text_color(cx.theme().muted_foreground)
                            .child({
                                if let Some(kind) = self.active_kind() {
                                    match kind {
                                        TabKind::Local => t!("local_terminal").to_string(),
                                        TabKind::Ssh => {
                                            if let Some((_, session)) = self.active_ssh_session() {
                                                format!(
                                                    "{}@{}:{}",
                                                    session.user, session.host, session.port
                                                )
                                            } else {
                                                "ssh".to_string()
                                            }
                                        }
                                    }
                                } else {
                                    self.active_title()
                                }
                            }),
                    ),
            )
            .when(
                self.active_kind() == Some(TabKind::Ssh)
                    || self.config.monitoring_position() == "Sidebar",
                |this| this.child(self.render_sidebar_monitoring_panel(cx)),
            )
            .child(
                Button::new("open-ssh-panel")
                    .primary()
                    .label(t!("add_ssh").to_string())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_new_ssh_dialog(window, cx)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .gap_2()
                    .child(
                        div()
                            .text_size(rems(1.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().primary)
                            .child(t!("saved")),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h(px(0.))
                            .size_full()
                            .child(
                                v_flex()
                                    .size_full()
                                    .id("saved-sessions-scroll")
                                    .track_scroll(&self.saved_scroll_handle)
                                    .overflow_y_scroll()
                                    .gap_2()
                                    .children(sessions.into_iter().enumerate().map(
                                        |(ix, session)| {
                                            let connect_id = session.id.clone();
                                            let edit_id = session.id.clone();
                                            let delete_id = session.id.clone();
                                            let is_active = active_session_id.as_deref()
                                                == Some(session.id.as_str());
                                            let name = session.name.clone();
                                            let detail = self.session_detail(&session);
                                            div()
                                                .id(("saved-connect", ix))
                                                .w_full()
                                                .p_2()
                                                .rounded_md()
                                                .border_1()
                                                .border_color(if is_active {
                                                    cx.theme().primary
                                                } else {
                                                    cx.theme().border
                                                })
                                                .bg(if is_active {
                                                    cx.theme().tab_active
                                                } else {
                                                    cx.theme().muted
                                                })
                                                .cursor_pointer()
                                                .hover(|this| this.bg(cx.theme().secondary))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, _, _, cx| {
                                                        this.connect_saved_session(
                                                            connect_id.clone(),
                                                            cx,
                                                        )
                                                    }),
                                                )
                                                .context_menu({
                                                    let view = cx.entity();
                                                    move |menu, window, _| {
                                                        let edit_value = edit_id.clone();
                                                        let clone_value = edit_id.clone();
                                                        let delete_value = delete_id.clone();
                                                        menu.item(
                                                            PopupMenuItem::new(
                                                                t!("clone").to_string(),
                                                            )
                                                            .on_click(window.listener_for(
                                                                &view,
                                                                move |this, _, window, cx| {
                                                                    this.clone_saved_session(
                                                                        clone_value.clone(),
                                                                        window,
                                                                        cx,
                                                                    )
                                                                },
                                                            )),
                                                        )
                                                        .item(
                                                            PopupMenuItem::new(
                                                                t!("edit").to_string(),
                                                            )
                                                            .on_click(window.listener_for(
                                                                &view,
                                                                move |this, _, window, cx| {
                                                                    this.edit_saved_session(
                                                                        edit_value.clone(),
                                                                        window,
                                                                        cx,
                                                                    )
                                                                },
                                                            )),
                                                        )
                                                        .item(
                                                            PopupMenuItem::new(
                                                                t!("delete").to_string(),
                                                            )
                                                            .on_click(window.listener_for(
                                                                &view,
                                                                move |this, _, _, cx| {
                                                                    this.remove_saved_session(
                                                                        delete_value.clone(),
                                                                        cx,
                                                                    )
                                                                },
                                                            )),
                                                        )
                                                    }
                                                })
                                                .child(
                                                    v_flex()
                                                        .gap_1()
                                                        .child(
                                                            div()
                                                                .text_size(rems(1.0))
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .child(name),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_size(rems(0.917))
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(detail),
                                                        ),
                                                )
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left_0()
                                    .right_0()
                                    .child(
                                        gpui_component::scroll::Scrollbar::new(
                                            &self.saved_scroll_handle,
                                        )
                                        .id("saved-scrollbar")
                                        .axis(gpui_component::scroll::ScrollbarAxis::Vertical)
                                        .scrollbar_show(
                                            gpui_component::scroll::ScrollbarShow::Always,
                                        ),
                                    ),
                            ),
                    ),
            )
    }

    fn render_collapsed_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sessions = self.config.sessions().to_vec();
        let active_session_id = self.active_session_id().map(ToOwned::to_owned);

        v_flex()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .p_2()
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .bg(cx.theme().sidebar)
            .overflow_hidden()
            .items_center()
            // Top: expand button only
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .pb_2()
                    .child(
                        Button::new("sidebar-expand-toggle")
                            .ghost()
                            .icon(IconName::PanelLeftOpen)
                            .tooltip(t!("settings_toggle_sidebar").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_collapsed = false;
                                this.config.set_sidebar_collapsed(false);
                                let _ = this.config.save();
                                cx.notify();
                            })),
                    ),
            )
            // Saved sessions as compact cards
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .child(
                        v_flex()
                            .size_full()
                            .id("collapsed-saved-sessions-scroll")
                            .track_scroll(&self.collapsed_saved_scroll_handle)
                            .overflow_y_scroll()
                            .gap_2()
                            .items_center()
                            .children(sessions.into_iter().enumerate().map(|(ix, session)| {
                                let connect_id = session.id.clone();
                                let is_active =
                                    active_session_id.as_deref() == Some(session.id.as_str());
                                let name = session.name.clone();

                                // Abbreviate: first 1 char for CJK, first 2 chars for Latin
                                let abbrev = {
                                    let mut chars = name.chars();
                                    if let Some(first) = chars.next() {
                                        if first > '\u{2E7F}' {
                                            // CJK character range — show 1 char
                                            first.to_string()
                                        } else {
                                            // Latin / ASCII — show first 2 chars
                                            let mut s = first.to_string();
                                            if let Some(second) = chars.next() {
                                                s.push(second);
                                            }
                                            s
                                        }
                                    } else {
                                        "?".to_string()
                                    }
                                };

                                let edit_id = session.id.clone();
                                let delete_id = session.id.clone();
                                div()
                                    .id(("collapsed-saved", ix))
                                    .w(px(36.))
                                    .h(px(36.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(if is_active {
                                        cx.theme().primary
                                    } else {
                                        cx.theme().border
                                    })
                                    .bg(if is_active {
                                        cx.theme().tab_active
                                    } else {
                                        cx.theme().muted
                                    })
                                    .cursor_pointer()
                                    .hover(|this| this.bg(cx.theme().secondary))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.connect_saved_session(connect_id.clone(), cx)
                                        }),
                                    )
                                    .tooltip({
                                        let tooltip_text = format!("{} {}", name, session.user);
                                        move |window, cx| {
                                            gpui_component::tooltip::Tooltip::new(
                                                tooltip_text.clone(),
                                            )
                                            .build(window, cx)
                                        }
                                    })
                                    .context_menu({
                                        let view = cx.entity();
                                        move |menu, window, _| {
                                            let edit_value = edit_id.clone();
                                            let clone_value = edit_id.clone();
                                            let delete_value = delete_id.clone();
                                            menu.item(
                                                PopupMenuItem::new(t!("clone").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        move |this, _, window, cx| {
                                                            this.clone_saved_session(
                                                                clone_value.clone(),
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                            .item(
                                                PopupMenuItem::new(t!("edit").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        move |this, _, window, cx| {
                                                            this.edit_saved_session(
                                                                edit_value.clone(),
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                            .item(
                                                PopupMenuItem::new(t!("delete").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        move |this, _, _, cx| {
                                                            this.remove_saved_session(
                                                                delete_value.clone(),
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                        }
                                    })
                                    .child(
                                        div()
                                            .text_size(rems(0.833))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(if is_active {
                                                cx.theme().primary
                                            } else {
                                                cx.theme().foreground
                                            })
                                            .child(abbrev),
                                    )
                            })),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .child(
                                gpui_component::scroll::Scrollbar::new(
                                    &self.collapsed_saved_scroll_handle,
                                )
                                .id("collapsed-saved-scrollbar")
                                .axis(gpui_component::scroll::ScrollbarAxis::Vertical)
                                .scrollbar_show(gpui_component::scroll::ScrollbarShow::Scrolling),
                            ),
                    ),
            )
    }

    fn render_window_controls(
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
                            this.save_layout_state(window, cx);
                            window.remove_window();
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

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let active_tab_index = self
            .active_tab
            .as_ref()
            .and_then(|active_id| self.tabs.iter().position(|tab| &tab.id == active_id));
        let active_group_index = self
            .active_group
            .as_ref()
            .and_then(|gid| self.tab_groups.iter().position(|g| g.id == *gid));
        // Home is the default tab, but it is not kept open after the user
        // enters a terminal workspace. The trailing plus creates it again.
        let show_home_tab = self.home_page_open || self.active_tab.is_none();
        let home_page_selected =
            (show_home_tab && self.home_page_open) || self.active_tab.is_none();
        let home_tab_index = self.tab_groups.len();
        let selected = if home_page_selected {
            home_tab_index
        } else {
            active_group_index.or(active_tab_index).unwrap_or(0)
        };
        let groups_data: Vec<(String, u64, String, Vec<String>)> = self
            .tab_groups
            .iter()
            .map(|g| {
                let pane_ids: Vec<String> = g
                    .pane_root
                    .tab_ids()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                (g.id.clone(), g.ordinal, g.title.clone(), pane_ids)
            })
            .collect();
        let is_integrated =
            self.active_title_bar_style == crate::session::config::TitleBarStyle::Integrated;

        h_flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .items_center()
            .gap_2()
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
                            .child(
                                h_flex()
                                    .relative()
                                    .h_full()
                                    .items_center()
                                    .px_3()
                                    .when(home_page_selected, |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .right_0()
                                                .h(px(2.))
                                                .bg(cx.theme().primary),
                                        )
                                        .font_weight(FontWeight::BOLD)
                                    })
                                    .child(t!("new_tab")),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.home_page_open = true;
                                this.home_page = HomePage::Overview;
                                cx.notify();
                            }));
                        let plus_tab = Tab::new()
                            .min_w(px(40.))
                            .child(
                                h_flex()
                                    .h_full()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::new(IconName::Plus).with_size(Size::Small)),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.home_page_open = true;
                                this.home_page = HomePage::Overview;
                                cx.notify();
                            }));
                        TabBar::new("ashell-tab-bar")
                            .track_scroll(&self.tabs_scroll_handle)
                            .selected_index(selected)
                            .children(groups_data.iter().enumerate().map(
                                |(ix, (group_id, ordinal, title, pane_ids))| {
                                    let gid = group_id.clone();
                                    let label = if pane_ids.len() > 1 {
                                        format!("{} {} ({})", ordinal, title, pane_ids.len())
                                    } else {
                                        format!("{} {}", ordinal, title)
                                    };
                                    let close_id = if self.active_group.as_ref() == Some(&gid) {
                                        self.active_tab.clone().unwrap_or_else(|| {
                                            pane_ids.first().cloned().unwrap_or_default()
                                        })
                                    } else {
                                        pane_ids.first().cloned().unwrap_or_default()
                                    };

                                    // Status is independent of selection: grey means the
                                    // backend is still connecting, green is ready, and red
                                    // means the connection has failed or disconnected.
                                    let dot_color = pane_ids
                                        .first()
                                        .and_then(|id| self.tabs.iter().find(|t| t.id == *id))
                                        .map(|tab| {
                                            if tab.disconnected_reason.is_some() {
                                                cx.theme().danger
                                            } else if tab.connected {
                                                cx.theme().success
                                            } else {
                                                cx.theme().muted_foreground
                                            }
                                        })
                                        .unwrap_or(cx.theme().muted_foreground);
                                    let drag_gid = gid.clone();
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
                                        .child(
                                            h_flex()
                                                .relative()
                                                .h_full()
                                                .items_center()
                                                .gap_2()
                                                .px_2()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, event: &MouseDownEvent, _, _| {
                                                        this.tab_drag.begin(drag_gid.clone(), event.position);
                                                    }),
                                                )
                                                .when(ix == selected, |this| {
                                                    this.child(
                                                        div()
                                                            .absolute()
                                                            .top_0()
                                                            .left_0()
                                                            .right_0()
                                                            .h(px(2.))
                                                            .bg(cx.theme().primary),
                                                    )
                                                    .font_weight(FontWeight::BOLD)
                                                })
                                                .child(
                                                    div()
                                                        .size(px(8.))
                                                        .flex_none()
                                                        .rounded_full()
                                                        .bg(dot_color),
                                                )
                                                .child(div().min_w(px(0.)).child(label)),
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
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                                this.tab_context_menu = Some(
                                                    TabContextMenuState {
                                                        group_id: context_gid.clone(),
                                                        position: event.position,
                                                    },
                                                );
                                                cx.notify();
                                            }),
                                        )
                                },
                            ).chain(show_home_tab.then_some(home_tab)).chain(std::iter::once(plus_tab)))
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
                        Button::new("split-horizontal")
                            .secondary()
                            .small()
                            .rounded(px(999.))
                            .icon(IconName::PanelBottom)
                            .tooltip(t!("settings_split_pane_down").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.split_current_pane("down", cx);
                            })),
                    )
                    .child(
                        Button::new("split-vertical")
                            .secondary()
                            .small()
                            .rounded(px(999.))
                            .icon(IconName::PanelRight)
                            .tooltip(t!("settings_split_pane_right").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.split_current_pane("right", cx);
                            })),
                    )
                    .child(
                        Button::new("new-window")
                            .secondary()
                            .small()
                            .rounded(px(999.))
                            .icon(IconName::ExternalLink)
                            .tooltip(t!("settings_new_window").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_new_window(cx);
                            })),
                    )
                    .child(self.render_search_button(cx)),
            )
    }

    fn render_terminal_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let has_active = self.active_tab.is_some();
        let pane_tree = self.pane_root.clone();
        let view = cx.entity();

        v_flex()
            .size_full()
            .relative()
            .child(
                div()
                    .size_full()
                    .on_prepaint(move |bounds, _window, cx| {
                        view.update(cx, |this, cx| {
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
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(Self::on_terminal_right_click),
                    )
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
                    }),
            )
            // Keep terminal input in the terminal itself, while making the
            // high-frequency workspace actions consistently available at its
            // lower edge without changing terminal or split-pane behavior.
            .when(has_active, |this| {
                this.child(
                    h_flex()
                        .flex_none()
                        .h(px(32.))
                        .px_2()
                        .items_center()
                        .gap_1()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().tab_bar)
                        .child(
                            div()
                                .flex_1()
                                .text_size(rems(0.75))
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("terminal_quick_actions")),
                        )
                        .child(
                            Button::new("workspace-quick-search")
                                .ghost()
                                .small()
                                .icon(IconName::Search)
                                .label(t!("search").to_string())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_search(window, cx);
                                })),
                        )
                        .child(
                            Button::new("workspace-quick-split")
                                .ghost()
                                .small()
                                .icon(IconName::PanelBottom)
                                .label(t!("workspace_split").to_string())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.split_current_pane("down", cx);
                                })),
                        ),
                )
            })
            // Search bar overlay — only when search is active.
            .when(self.search_active, |el| {
                el.child(self.render_search_bar(window, cx))
            })
            // Every non-reorder drag has visible feedback. A neutral destination
            // explicitly states that releasing will cancel instead of moving data.
            .when(
                (self.tab_drag.is_dragging() && self.tab_drag.reorder_index().is_none())
                    || self.incoming_tab_drag.is_some(),
                |el| el.child(self.render_tab_drag_overlay(cx)),
            )
    }

    fn render_tab_drag_overlay(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let scrim = hsla(220. / 360., 0.25, 0.08, 0.28);
        let card_bg = hsla(217. / 360., 0.88, 0.40, 0.98);
        let card_border = hsla(199. / 360., 0.95, 0.72, 1.0);
        let card_text = hsla(0., 0., 1.0, 1.0);
        let neutral_bg = hsla(32. / 360., 0.82, 0.34, 0.98);
        let neutral_border = hsla(42. / 360., 0.95, 0.68, 1.0);

        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .bg(scrim)
            .when(
                self.tab_drag.outside() && self.tab_drag.merge_target().is_none(),
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
                                h_flex()
                                    .gap_2()
                                    .px(px(20.))
                                    .py(px(12.))
                                    .rounded_lg()
                                    .border_2()
                                    .border_color(card_border)
                                    .bg(card_bg)
                                    .shadow_lg()
                                    .text_color(card_text)
                                    .child(
                                        Icon::new(IconName::ExternalLink)
                                            .with_size(Size::Small)
                                            .text_color(card_text),
                                    )
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(FontWeight::BOLD)
                                            .child(t!("drag_detach_hint").to_string()),
                                    ),
                            ),
                    )
                },
            )
            .when(
                self.tab_drag.is_dragging()
                    && !self.tab_drag.outside()
                    && self.tab_drag.merge_target().is_none()
                    && self.tab_drag.reorder_index().is_none()
                    && self.incoming_tab_drag.is_none(),
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
                                h_flex()
                                    .gap_2()
                                    .px(px(20.))
                                    .py(px(12.))
                                    .rounded_lg()
                                    .border_2()
                                    .border_color(neutral_border)
                                    .bg(neutral_bg)
                                    .shadow_lg()
                                    .text_color(card_text)
                                    .child(
                                        Icon::new(IconName::Close)
                                            .with_size(Size::Small)
                                            .text_color(card_text),
                                    )
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(FontWeight::BOLD)
                                            .child(t!("drag_cancel_hint").to_string()),
                                    ),
                            ),
                    )
                },
            )
            .when(
                self.incoming_tab_drag.is_some() || self.tab_drag.merge_target().is_some(),
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
                                h_flex()
                                    .gap_2()
                                    .px(px(20.))
                                    .py(px(12.))
                                    .rounded_lg()
                                    .border_2()
                                    .border_color(card_border)
                                    .bg(card_bg)
                                    .shadow_lg()
                                    .text_color(card_text)
                                    .child(
                                        Icon::new(IconName::ArrowDown)
                                            .with_size(Size::Small)
                                            .text_color(card_text),
                                    )
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(FontWeight::BOLD)
                                            .child(t!("drag_merge_hint").to_string()),
                                    ),
                            ),
                    )
                },
            )
    }

    fn render_pane_tree(
        this: &mut Ashell,
        layout: &PaneLayout,
        path: &[usize],
        cx: &mut Context<Ashell>,
    ) -> impl IntoElement {
        match layout {
            PaneLayout::Single(tab_id) => {
                if tab_id.is_empty() {
                    return this.render_home_page(cx).into_any_element();
                }
                let is_focused = path == this.focused_pane_path.as_slice();
                let keyword_highlight = this.config.keyword_highlight();
                let snapshot = this
                    .tabs
                    .iter()
                    .find(|t| &t.id == tab_id)
                    .map(|t| t.render_snapshot(keyword_highlight));
                let Some(snapshot) = snapshot else {
                    return div().into_any_element();
                };
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
                        cx.entity(),
                        focus_handle,
                        snapshot,
                        marked_text,
                        font_family,
                        font_size,
                        line_height,
                        cell_width,
                        tab_id.to_string(),
                        this.search_highlight_map(
                            tab_id,
                            cx.theme().danger.opacity(0.35),
                            cx.theme().danger.opacity(0.70),
                        ),
                    ));
                let scrollbar = this.terminal_scrollbars.entry(tab_id.clone()).or_default();
                el = el.vertical_scrollbar(scrollbar);

                // When disconnected, overlay a reconnect bar at the bottom of the terminal.
                // Uses absolute positioning so the terminal element itself is unchanged,
                // keeping panel size stable in multi-panel layouts.
                let disconnected_reason = this
                    .tabs
                    .iter()
                    .find(|t| t.id == *tab_id)
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

                // A failed SSH attempt belongs to this terminal pane, not to
                // the entire application window. Keeping the recovery card
                // here leaves other tabs, the sidebar, and SFTP usable.
                if let Some(progress) = this
                    .connection_progress
                    .clone()
                    .filter(|progress| progress.tab_id == *tab_id && progress.failed)
                {
                    el = div().size_full().relative().child(el).child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .bg(gpui::Hsla {
                                h: 0.0,
                                s: 0.0,
                                l: 0.0,
                                a: 0.28,
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .p_4()
                            .child(
                                v_flex()
                                    .w(px(420.))
                                    .max_w_full()
                                    .gap_3()
                                    .p_4()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().popover)
                                    .shadow_lg()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                Icon::new(IconName::Close)
                                                    .with_size(Size::Small)
                                                    .text_color(cx.theme().danger),
                                            )
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(progress.title.clone()),
                                            ),
                                    )
                                    .child(div().max_h(px(180.)).overflow_y_scrollbar().children(
                                        progress.lines.iter().cloned().map(|line| {
                                            div()
                                                .text_size(rems(0.875))
                                                .text_color(cx.theme().danger)
                                                .child(line)
                                        }),
                                    ))
                                    .child(
                                        h_flex()
                                            .justify_end()
                                            .gap_2()
                                            .child(
                                                Button::new(format!("pane-connect-retry-{tab_id}"))
                                                    .primary()
                                                    .label(t!("retry").to_string())
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.retry_connection_progress(cx);
                                                    })),
                                            )
                                            .child(
                                                Button::new(format!(
                                                    "pane-connect-cancel-{tab_id}"
                                                ))
                                                .secondary()
                                                .label(t!("cancel").to_string())
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.cancel_connection_progress(cx);
                                                })),
                                            ),
                                    ),
                            ),
                    );
                }
                let indicator_color = this
                    .tabs
                    .iter()
                    .find(|t| t.id == *tab_id)
                    .map(|tab| {
                        if tab.connected {
                            cx.theme().success
                        } else {
                            cx.theme().danger
                        }
                    })
                    .unwrap_or(cx.theme().success);
                let has_multiple_panes = this.pane_root.tab_ids().len() > 1;

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
}

impl Render for Ashell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self
            .active_tab
            .as_ref()
            .is_some_and(|active_id| !self.tabs.iter().any(|tab| &tab.id == active_id))
        {
            self.active_tab = self.tabs.first().map(|tab| tab.id.clone());
        }
        self.sync_sftp_path_input(window, cx);

        // Refresh this window's screen-space bounds in the cross-window
        // registry so other windows can hit-test against it during a
        // cross-window tab drag.
        {
            let handle = window.window_handle();
            let screen_bounds = match window.window_bounds() {
                gpui::WindowBounds::Fullscreen(b)
                | gpui::WindowBounds::Maximized(b)
                | gpui::WindowBounds::Windowed(b) => b,
            };
            crate::app::update_window_bounds(handle, screen_bounds);
            if window.is_window_active() {
                crate::app::mark_window_active(handle);
            }
        }

        if self.show_transfers_dialog {
            self.show_transfers_dialog = false;
            self.show_transfers_dialog(window, cx);
        }
        if let Some(active_id) = self.active_tab.clone() {
            if let Some(scrollbar) = self.terminal_scrollbars.get(&active_id) {
                if let Some(new_display_offset) = scrollbar.future_display_offset.take() {
                    if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == active_id) {
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
            if let Some(snapshot) = self.active_snapshot().as_ref() {
                if let Some(scrollbar) = self.terminal_scrollbars.get(&active_id) {
                    scrollbar.update(snapshot, px(self.terminal_line_height()));
                }
            }
        }

        // The file-transfer panel belongs to an active terminal session. Keeping it
        // out of the home workspace avoids showing an empty "remote files" area on
        // Overview and Key Manager pages.
        let main_content = if self.active_tab.is_some() && !self.home_page_open {
            let monitoring_contents = v_flex()
                .size_full()
                .when(self.config.monitoring_position() == "Bottom", |this| {
                    this.child(self.render_monitoring_panel(window.viewport_size().width, cx))
                })
                .child(self.render_sftp_panel(window, cx));

            let is_monitor_bottom = self.config.monitoring_position() == "Bottom";
            let minimized_height = if is_monitor_bottom { 104. } else { 24. };
            let min_panel_height = if is_monitor_bottom { 260. } else { 180. };
            let default_panel_height = if is_monitor_bottom { 328. } else { 248. };

            let sftp_size = if self.sftp_panel_minimized {
                px(minimized_height)
            } else {
                px(self
                    .config
                    .body_panels()
                    .and_then(|s| s.get(1).copied())
                    .unwrap_or(default_panel_height))
            };

            v_resizable("ashell-body")
                .lock(self.config.lock_layout())
                .with_state(&self.body_panels)
                .child(resizable_panel().child(self.render_terminal_panel(window, cx)))
                .child(
                    resizable_panel()
                        .size(sftp_size)
                        .size_range(if self.sftp_panel_minimized {
                            px(minimized_height)..px(minimized_height)
                        } else {
                            px(min_panel_height)..px(1200.)
                        })
                        .child(monitoring_contents),
                )
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

        let workspace = if self.sidebar_collapsed {
            h_flex()
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
                                            .child(self.render_tab_bar(cx)),
                                    )
                                },
                            )
                            .child(main_content),
                    ),
                )
                .into_any_element()
        } else {
            let sidebar_content = if self.active_tab.is_some() && !self.home_page_open {
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
                                    .child(self.render_tab_bar(cx)),
                            )
                        },
                    )
                    .child(main_content),
            );

            h_resizable("ashell-workspace")
                .lock(self.config.lock_layout())
                .with_state(&self.workspace_panels)
                .child(sidebar_area)
                .child(main_area)
                .into_any_element()
        };

        v_flex()
            .id("ashell-root")
            .size_full()
            .relative()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(self.ui_font_family.clone())
            // Keep tab-drag tracking on the root element. Registering a window
            // listener from Render is invalid during GPUI's layout phase.
            .on_mouse_move(cx.listener(Self::on_tab_drag_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(Self::on_tab_drag_mouse_up),
            )
            .on_action(cx.listener(|this, _: &crate::OpenSettings, window, cx| this.show_settings_dialog(window, cx)))
            .on_action(cx.listener(|this, _: &crate::OpenSession, window, cx| this.show_selector_dialog(window, cx)))
            .on_action(cx.listener(|this, _: &crate::OpenTransfers, window, cx| this.show_transfers_dialog(window, cx)))
            .on_action(cx.listener(|this, _: &crate::NewSsh, window, cx| this.show_ssh_dialog(window, cx)))
            .on_action(cx.listener(|this, _: &crate::NewWindow, _, cx| this.open_new_window(cx)))
            .on_action(cx.listener(|this, _: &crate::DetachTabToWindow, _, cx| this.detach_tab_to_new_window(cx)))
            .on_action(cx.listener(|this, _: &crate::OpenSearch, window, cx| this.toggle_search(window, cx)))
            .on_action(cx.listener(|this, _: &crate::ToggleSidebar, _, cx| {
                this.sidebar_collapsed = !this.sidebar_collapsed;
                this.config.set_sidebar_collapsed(this.sidebar_collapsed);
                let _ = this.config.save();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ToggleSftpZoom, window, cx| {
                this.toggle_sftp_minimized(window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::FocusPaneLeft, _, _| this.focus_adjacent_pane("left")))
            .on_action(cx.listener(|this, _: &crate::FocusPaneRight, _, _| this.focus_adjacent_pane("right")))
            .on_action(cx.listener(|this, _: &crate::FocusPaneUp, _, _| this.focus_adjacent_pane("up")))
            .on_action(cx.listener(|this, _: &crate::FocusPaneDown, _, _| this.focus_adjacent_pane("down")))
            .on_action(cx.listener(|this, _: &crate::SplitPaneLeft, _, cx| this.split_current_pane("left", cx)))
            .on_action(cx.listener(|this, _: &crate::SplitPaneRight, _, cx| this.split_current_pane("right", cx)))
            .on_action(cx.listener(|this, _: &crate::SplitPaneUp, _, cx| this.split_current_pane("up", cx)))
            .on_action(cx.listener(|this, _: &crate::SplitPaneDown, _, cx| this.split_current_pane("down", cx)))
            .on_action(cx.listener(|this, _: &crate::ClosePane, _, cx| {
                if let Some(active_id) = this.active_tab.clone() {
                    this.close_tab(active_id, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &crate::Copy, window, cx| {
                if window.focused(cx) == Some(this.focus_handle.clone()) {
                    if let Some(text) = this.active_terminal_selection_text() {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        if let Some(active_id) = &this.active_tab {
                            if let Some(tab) = this.tabs.iter_mut().find(|tab| &tab.id == active_id) {
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
            .when(self.active_title_bar_style == crate::session::config::TitleBarStyle::Integrated, |this| {
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
                                    .on_mouse_move(cx.listener(|this, _, window, _| {
                                        if this.should_move_window {
                                            this.should_move_window = false;
                                            window.start_window_move();
                                        }
                                    }))
                                })
                                .child(self.render_tab_bar(cx)),
                        ),
                )
            })
            .child(
                div().flex_1().min_h_0().child(workspace),
            )
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .when_some(self.sftp_context_menu.clone(), |this, menu| {
                let label = if menu.is_dir {
                    t!("download_folder").to_string()
                } else {
                    t!("download").to_string()
                };
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.dismiss_sftp_context_menu(cx);
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, _, _, cx| {
                                this.dismiss_sftp_context_menu(cx);
                            }),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(menu.position.x)
                                .top(menu.position.y)
                                .w(px(172.))
                                .p_1()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().popover)
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                })
                                .on_mouse_down(MouseButton::Right, |_, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                })
                                .child(
                                    v_flex()
                                        .w_full()
                                        .child(
                                            Button::new("sftp-context-download")
                                                .ghost()
                                                .w_full()
                                                .justify_start()
                                                .label(label)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.trigger_sftp_context_download(window, cx);
                                                })),
                                        )
                                        .when(
                                            !menu.is_dir
                                                && is_editable_text_file(&menu.remote_path),
                                            |this| {
                                                this.child(
                                                    Button::new("sftp-context-edit")
                                                        .ghost()
                                                        .w_full()
                                                        .justify_start()
                                                        .label(t!("edit_file"))
                                                        .tooltip(
                                                            t!("edit_file_tooltip").to_string(),
                                                        )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.trigger_sftp_context_edit(cx);
                                                        })),
                                                )
                                            },
                                        ),
                                ),
                        ),
                )
            })
            .when_some(self.tab_context_menu.clone(), |this, menu| {
                let detach_gid = menu.group_id.clone();
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.tab_context_menu = None;
                                cx.notify();
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, _, _, cx| {
                                this.tab_context_menu = None;
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(menu.position.x)
                                .top(menu.position.y)
                                .w(px(200.))
                                .p_1()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().popover)
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                })
                                .on_mouse_down(MouseButton::Right, |_, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                })
                                .child(
                                    v_flex()
                                        .w_full()
                                        .child(
                                            Button::new("tab-context-detach")
                                                .ghost()
                                                .w_full()
                                                .justify_start()
                                                .label(t!("settings_detach_tab").to_string())
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    this.tab_context_menu = None;
                                                    this.activate_group(
                                                        detach_gid.clone(),
                                                        window,
                                                        cx,
                                                    );
                                                    this.detach_tab_to_new_window(cx);
                                                })),
                                        ),
                                ),
                        ),
                )
            })
            .when_some(
                self.connection_progress
                    .clone()
                    .filter(|progress| !progress.failed),
                |this, progress| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .bg(gpui::Hsla {
                            h: 0.0,
                            s: 0.0,
                            l: 0.0,
                            a: 0.48,
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(420.))
                                .p_5()
                                .rounded_lg()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().popover)
                                .shadow_lg()
                                .child(
                                    v_flex()
                                        .gap_4()
                                        .child(
                                            Button::new("ssh-connect-progress")
                                                .primary()
                                                .loading(!progress.failed)
                                                .label(progress.title.clone()),
                                        )
                                        .child(
                                            div()
                                                .relative()
                                                .min_h(px(0.))
                                                .max_h(px(220.))
                                                .child(
                                                    div()
                                                        .id("connection-progress-scroll")
                                                        .max_h(px(220.))
                                                        .overflow_hidden()
                                                        .overflow_y_scroll()
                                                        .track_scroll(&self.connection_scroll_handle)
                                                        .child(
                                                            v_flex().gap_2().children(
                                                                progress.lines.iter().cloned().map(|line| {
                                                                    div()
                                                                        .text_size(rems(1.0))
                                                                        .text_color(if progress.failed {
                                                                            cx.theme().danger
                                                                        } else {
                                                                            cx.theme().muted_foreground
                                                                        })
                                                                        .child(line)
                                                                }),
                                                            ),
                                                        )
                                                )
                                                .child(
                                                    div()
                                                        .absolute()
                                                        .top_0()
                                                        .right_0()
                                                        .bottom_0()
                                                        .w(px(16.))
                                                        .child(
                                                            Scrollbar::vertical(&self.connection_scroll_handle)
                                                                .scrollbar_show(ScrollbarShow::Scrolling)
                                                        )
                                                )
                                        )
                                        .when(progress.failed, |this| {
                                            this.child(
                                                h_flex()
                                                    .justify_end()
                                                    .gap_2()
                                                    .child(
                                                        Button::new("ssh-connect-progress-retry")
                                                            .primary()
                                                            .label(t!("retry").to_string())
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.retry_connection_progress(
                                                                        cx,
                                                                    )
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        Button::new("ssh-connect-progress-close")
                                                            .label(t!("cancel").to_string())
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.cancel_connection_progress(
                                                                        cx,
                                                                    )
                                                                },
                                                            )),
                                                    ),
                                            )
                                        }),
                                ),
                        ),
                )
            })
            .on_prepaint({
                let view = cx.entity().clone();
                move |_, window, cx| {
                    view.update(cx, |this, cx| {
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
    }
}
