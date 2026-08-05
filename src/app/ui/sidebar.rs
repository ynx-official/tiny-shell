use crate::app::settings::MonitoringPosition;

use super::*;

impl TinyShell {
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let show_update_pulse = matches!(
            self.update_runtime.status,
            Some(crate::app::updater::UpdateStatus::UpdateAvailable(_))
                | Some(crate::app::updater::UpdateStatus::ReadyToRestart(_, _))
        );
        let show_update_error_badge = matches!(
            self.update_runtime.status,
            Some(crate::app::updater::UpdateStatus::DownloadCancelled(_))
                | Some(crate::app::updater::UpdateStatus::DownloadFailed(_, _))
        );
        let active_tab = self
            .active_tab
            .as_ref()
            .and_then(|active_id| self.terminal_tab(active_id));
        let active_session = active_tab.and_then(|tab| tab.session.as_ref());
        let host_text = active_session
            .map(|session| session.host.clone())
            .unwrap_or_else(|| t!("local_host").to_string());
        let connection_text = active_session
            .map(|session| format!("{}@{}:{}", session.user, session.host, session.port))
            .unwrap_or_else(|| t!("local_terminal").to_string());
        let mut ip_address_entries = self.monitoring.system.ip_address_entries.clone();
        if ip_address_entries.is_empty() && !host_text.is_empty() {
            ip_address_entries.push(crate::system::IpAddressSample {
                interface: "-".to_string(),
                address: host_text.clone(),
            });
        }
        let primary_ip = ip_address_entries
            .first()
            .map(|entry| entry.address.clone())
            .unwrap_or_else(|| "-".to_string());
        let load_values = self
            .monitoring
            .system
            .load_average
            .split(|character: char| character == ',' || character.is_whitespace())
            .filter(|value| !value.is_empty())
            .take(3)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let load_value = |index: usize| {
            load_values
                .get(index)
                .cloned()
                .unwrap_or_else(|| "--".to_string())
        };
        let load_one = load_value(0);
        let load_five = load_value(1);
        let load_fifteen = load_value(2);

        let content = v_flex()
            .gap_2()
            .flex_1()
            .min_h(px(0.))
            .p_2()
            .overflow_hidden()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(56.)))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_center()
                            .child(
                                h_flex()
                                    .id("sidebar-brand-version")
                                    .items_center()
                                    .gap_1()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_update_dialog(window, cx)
                                    }))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::BOLD)
                                            .text_size(rems(1.25))
                                            .child(t!("app_name")),
                                    )
                                    .child(
                                        div()
                                            .relative()
                                            .px_1()
                                            .py(px(1.))
                                            .rounded_full()
                                            .bg(cx.theme().muted)
                                            .text_size(rems(0.58))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "v{}",
                                                env!("CARGO_PKG_VERSION")
                                            ))
                                            .when(show_update_error_badge, |this| {
                                                this.child(
                                                    div()
                                                        .absolute()
                                                        .top(px(-2.))
                                                        .right(px(-2.))
                                                        .size(px(6.))
                                                        .rounded_full()
                                                        .bg(cx.theme().danger),
                                                )
                                            }),
                                    )
                                    .when(show_update_pulse, |this| {
                                        this.child(crate::app::updater::pulse_icon(
                                            "sidebar-update-pulse",
                                            cx.theme().primary,
                                        ))
                                    }),
                            ),
                    )
                    .child(
                        Button::new("sidebar-collapse-toggle")
                            .ghost()
                            .small()
                            .icon(IconName::PanelLeftClose)
                            .tooltip(t!("settings_toggle_sidebar").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_collapsed = true;
                                this.config.set_sidebar_collapsed(true);
                                this.mark_config_preferences_dirty();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .px_2()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(48.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(rems(0.8))
                                    .child(t!("server_ip")),
                            )
                            .child({
                                let trigger_ip = primary_ip.clone();
                                let trigger = h_flex()
                                    .id("sidebar-ip-list")
                                    .flex_1()
                                    .min_w(px(0.))
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .id("sidebar-primary-ip")
                                            .flex_1()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .cursor_pointer()
                                            .text_size(rems(0.8))
                                            .on_click(move |_, _, cx| {
                                                cx.write_to_clipboard(
                                                    gpui::ClipboardItem::new_string(
                                                        trigger_ip.clone(),
                                                    ),
                                                );
                                            })
                                            .child(primary_ip),
                                    );

                                if ip_address_entries.len() > 1 {
                                    let popover_entries = ip_address_entries.clone();
                                    HoverCard::new("sidebar-ip-hover-card")
                                        .anchor(Anchor::TopRight)
                                        .open_delay(Duration::ZERO)
                                        .close_delay(Duration::from_millis(350))
                                        .trigger(trigger)
                                        .content(move |_, _, cx| {
                                            v_flex()
                                                .w(px(300.))
                                                .gap_1()
                                                .child(
                                                    h_flex()
                                                        .px_1()
                                                        .pb_1()
                                                        .items_center()
                                                        .justify_between()
                                                        .child(
                                                            div()
                                                                .text_size(rems(0.72))
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .child(t!("ip_address")),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_size(rems(0.62))
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(t!("click_to_copy")),
                                                        ),
                                                )
                                                .child(
                                                    v_flex()
                                                        .rounded_md()
                                                        .border_1()
                                                        .border_color(cx.theme().border)
                                                        .overflow_hidden()
                                                        .child(
                                                            h_flex()
                                                                .h(px(26.))
                                                                .px_2()
                                                                .items_center()
                                                                .bg(cx.theme().muted)
                                                                .text_size(rems(0.64))
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(
                                                                    div()
                                                                        .w(px(68.))
                                                                        .flex_none()
                                                                        .child(t!(
                                                                            "network_interface"
                                                                        )),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .flex_1()
                                                                        .child(t!("ip_address")),
                                                                ),
                                                        )
                                                        .children(
                                                            popover_entries
                                                                .clone()
                                                                .into_iter()
                                                                .enumerate()
                                                                .map(|(index, entry)| {
                                                                    let copied_ip =
                                                                        entry.address.clone();
                                                                    let tooltip = format!(
                                                                        "{}\n{}",
                                                                        entry.interface,
                                                                        entry.address
                                                                    );
                                                                    h_flex()
                                                                        .id((
                                                                            "sidebar-copy-ip",
                                                                            index,
                                                                        ))
                                                                        .h(px(30.))
                                                                        .px_2()
                                                                        .items_center()
                                                                        .cursor_pointer()
                                                                        .text_size(rems(0.68))
                                                                        .border_t_1()
                                                                        .border_color(
                                                                            cx.theme()
                                                                                .border
                                                                                .opacity(0.5),
                                                                        )
                                                                        .hover(|this| {
                                                                            this.bg(
                                                                                cx.theme().muted,
                                                                            )
                                                                        })
                                                                        .tooltip(
                                                                            move |window, cx| {
                                                                                gpui_component::tooltip::Tooltip::new(
                                                                                    tooltip.clone(),
                                                                                )
                                                                                .build(window, cx)
                                                                            },
                                                                        )
                                                                        .on_click(
                                                                            move |_, _, cx| {
                                                                                cx.write_to_clipboard(
                                                                                    gpui::ClipboardItem::new_string(
                                                                                        copied_ip.clone(),
                                                                                    ),
                                                                                );
                                                                            },
                                                                        )
                                                                        .child(
                                                                            div()
                                                                                .w(px(68.))
                                                                                .flex_none()
                                                                                .min_w(px(0.))
                                                                                .overflow_hidden()
                                                                                .whitespace_nowrap()
                                                                                .text_ellipsis()
                                                                                .child(
                                                                                    entry.interface,
                                                                                ),
                                                                        )
                                                                        .child(
                                                                            div()
                                                                                .flex_1()
                                                                                .min_w(px(0.))
                                                                                .overflow_hidden()
                                                                                .whitespace_nowrap()
                                                                                .text_ellipsis()
                                                                                .child(
                                                                                    entry.address,
                                                                                ),
                                                                        )
                                                                }),
                                                        ),
                                                )
                                        })
                                        .into_any_element()
                                } else {
                                    trigger.into_any_element()
                                }
                            }),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(48.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(rems(0.8))
                                    .child(t!("connection_address")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(rems(0.75))
                                    .child(connection_text),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(48.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(rems(0.8))
                                    .child(t!("running")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(rems(0.75))
                                    .child(format_uptime(self.monitoring.system.uptime_seconds)),
                            ),
                    ),
            )
            .child(
                div().w_full().px_1().child(
                    h_flex()
                        .id("sidebar-system-information")
                        .relative()
                        .w_full()
                        .h(px(26.))
                        .px_2()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().muted)
                        .cursor_pointer()
                        .text_size(rems(0.85))
                        .font_weight(FontWeight::MEDIUM)
                        .hover(|this| this.bg(cx.theme().secondary.opacity(0.7)))
                        .child(t!("server_information"))
                        .child(
                            div()
                                .absolute()
                                .right(px(8.))
                                .flex()
                                .items_center()
                                .child(Icon::new(IconName::ExternalLink).with_size(Size::Small)),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_system_info_tab(cx);
                        })),
                ),
            )
            .when(
                self.active_kind() == Some(TabKind::Ssh)
                    || MonitoringPosition::from_config(self.config.monitoring_position())
                        == MonitoringPosition::Sidebar,
                |this| {
                    this.child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_hidden()
                            .child(self.render_sidebar_monitoring_panel(cx)),
                    )
                },
            );

        v_flex()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .bg(cx.theme().sidebar)
            .overflow_hidden()
            .child(content)
            .child(
                h_flex()
                    .w_full()
                    .h(px(24.))
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().tab_bar)
                    .text_size(rems(0.68))
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        Icon::new(IconName::Cpu)
                            .with_size(Size::Small)
                            .text_color(cx.theme().primary),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(t!("sidebar_system_load")),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(div().size(px(6.)).rounded_full().bg(gpui::rgb(0x36B37E)))
                            .child(load_one),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(div().size(px(6.)).rounded_full().bg(gpui::rgb(0x7C8494)))
                            .child(load_five),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(div().size(px(6.)).rounded_full().bg(gpui::rgb(0x8B5CF6)))
                            .child(load_fifteen),
                    ),
            )
    }

    pub(super) fn render_collapsed_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                                this.mark_config_preferences_dirty();
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
                                        cx.listener(move |this, _, window, cx| {
                                            this.connect_saved_session(
                                                connect_id.clone(),
                                                window,
                                                cx,
                                            )
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
                                                        move |this, _, window, cx| {
                                                            this.request_saved_session_deletion(
                                                                delete_value.clone(),
                                                                window,
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
}
