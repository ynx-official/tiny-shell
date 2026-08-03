use super::*;

impl TinyShell {
    pub(super) fn render_system_info_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let source_tab_id = self.active_system_info_tab.as_ref().and_then(|info_id| {
            self.system_info_tabs
                .iter()
                .find(|tab| &tab.id == info_id)
                .map(|tab| tab.source_tab_id.clone())
        });
        let source_tab = source_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.id == id));
        let snapshot = source_tab_id
            .as_ref()
            .and_then(|id| self.remote_system_snapshots.get(id))
            .cloned()
            .unwrap_or_default();
        let connection = source_tab
            .and_then(|tab| tab.session.as_ref())
            .map(|session| format!("{}@{}:{}", session.user, session.host, session.port))
            .unwrap_or_default();
        let display = |value: String| {
            if value.trim().is_empty() {
                "-".to_string()
            } else {
                value
            }
        };
        let info_row = |label: String, value: String| {
            h_flex()
                .min_h(px(30.))
                .items_center()
                .gap_3()
                .child(
                    div()
                        .w(px(112.))
                        .flex_none()
                        .text_size(rems(0.76))
                        .text_color(cx.theme().muted_foreground)
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(rems(0.78))
                        .child(display(value)),
                )
                .into_any_element()
        };
        let card_title = |title: String| {
            div()
                .h(px(38.))
                .px_4()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(cx.theme().border)
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(rems(0.86))
                .child(title)
        };

        let mut processes = snapshot.processes.clone();
        processes.sort_by(|left, right| {
            right
                .cpu_percent
                .partial_cmp(&left.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        processes.truncate(12);

        div()
            .size_full()
            .overflow_y_scrollbar()
            .bg(cx.theme().muted.opacity(0.32))
            .child(
                v_flex()
                    .w_full()
                    .flex_none()
                    .p_5()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(rems(1.35))
                                    .child(t!("system_information")),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.78))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("system_information_desc")),
                            ),
                    )
                    .child(
                        v_flex()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .overflow_hidden()
                            .child(card_title(t!("system_overview").to_string()))
                            .child(
                                h_flex()
                                    .items_start()
                                    .gap_6()
                                    .p_4()
                                    .child(v_flex().flex_1().min_w(px(0.)).children(vec![
                                        info_row(
                                            t!("operating_system").to_string(),
                                            snapshot.os_name.clone(),
                                        ),
                                        info_row(
                                            t!("kernel_version").to_string(),
                                            snapshot.kernel_version.clone(),
                                        ),
                                        info_row(
                                            t!("host_name").to_string(),
                                            snapshot.hostname.clone(),
                                        ),
                                        info_row(
                                            t!("ip_address").to_string(),
                                            snapshot.ip_address.clone(),
                                        ),
                                        info_row(
                                            t!("system_load").to_string(),
                                            snapshot.load_average.clone(),
                                        ),
                                    ]))
                                    .child(v_flex().flex_1().min_w(px(0.)).children(vec![
                                        info_row(
                                            t!("kernel").to_string(),
                                            snapshot.kernel_name.clone(),
                                        ),
                                        info_row(
                                            t!("architecture").to_string(),
                                            snapshot.architecture.clone(),
                                        ),
                                        info_row(t!("connection_address").to_string(), connection),
                                        info_row(
                                            t!("uptime").to_string(),
                                            format_uptime(snapshot.uptime_seconds),
                                        ),
                                        info_row(
                                            t!("cpu_usage").to_string(),
                                            format!("{:.1}%", snapshot.cpu_percent * 100.0),
                                        ),
                                    ])),
                            ),
                    )
                    .child(
                        v_flex()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .overflow_hidden()
                            .child(card_title(t!("cpu").to_string()))
                            .child(
                                h_flex()
                                    .min_h(px(42.))
                                    .items_center()
                                    .px_4()
                                    .gap_4()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(rems(0.76))
                                            .child(display(snapshot.cpu_model.clone())),
                                    )
                                    .child(
                                        div()
                                            .w(px(120.))
                                            .text_center()
                                            .text_size(rems(0.76))
                                            .child(format!(
                                                "{} {}",
                                                snapshot.cpu_cores,
                                                t!("cpu_cores")
                                            )),
                                    )
                                    .child(
                                        div()
                                            .w(px(140.))
                                            .text_center()
                                            .text_size(rems(0.76))
                                            .child(if snapshot.cpu_frequency_mhz == 0 {
                                                "-".to_string()
                                            } else {
                                                format!("{} MHz", snapshot.cpu_frequency_mhz)
                                            }),
                                    )
                                    .child(
                                        div()
                                            .w(px(100.))
                                            .text_center()
                                            .text_size(rems(0.76))
                                            .child(format!("{:.1}%", snapshot.cpu_percent * 100.0)),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_start()
                            .gap_4()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().background)
                                    .overflow_hidden()
                                    .child(card_title(t!("memory").to_string()))
                                    .child(v_flex().p_4().children(vec![
                                        info_row(
                                            t!("total").to_string(),
                                            format_bytes(snapshot.mem_total),
                                        ),
                                        info_row(
                                            t!("used").to_string(),
                                            format_bytes(snapshot.mem_used),
                                        ),
                                        info_row(
                                            t!("available").to_string(),
                                            format_bytes(snapshot.mem_available),
                                        ),
                                        info_row(
                                            t!("usage").to_string(),
                                            format!("{:.1}%", snapshot.mem_percent * 100.0),
                                        ),
                                    ])),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().background)
                                    .overflow_hidden()
                                    .child(card_title(t!("swap").to_string()))
                                    .child(v_flex().p_4().children(vec![
                                info_row(
                                    t!("total").to_string(),
                                    format_bytes(snapshot.total_swap),
                                ),
                                info_row(t!("used").to_string(), format_bytes(snapshot.swap_used)),
                                info_row(
                                    t!("available").to_string(),
                                    format_bytes(
                                        snapshot.total_swap.saturating_sub(snapshot.swap_used),
                                    ),
                                ),
                                info_row(
                                    t!("usage").to_string(),
                                    format!("{:.1}%", snapshot.swap_percent * 100.0),
                                ),
                            ])),
                            ),
                    )
                    .child(
                        v_flex()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .overflow_hidden()
                            .child(card_title(t!("processes").to_string()))
                            .child(
                                h_flex()
                                    .h(px(32.))
                                    .items_center()
                                    .px_4()
                                    .bg(cx.theme().muted)
                                    .text_size(rems(0.72))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(div().flex_1().child(t!("process_command")))
                                    .child(
                                        div().w(px(120.)).text_center().child(t!("process_memory")),
                                    )
                                    .child(div().w(px(100.)).text_center().child(t!("cpu"))),
                            )
                            .children(processes.into_iter().enumerate().map(|(index, process)| {
                                h_flex()
                                    .min_h(px(32.))
                                    .items_center()
                                    .px_4()
                                    .when(index % 2 == 1, |this| {
                                        this.bg(cx.theme().muted.opacity(0.35))
                                    })
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .text_size(rems(0.74))
                                            .child(process.command),
                                    )
                                    .child(
                                        div()
                                            .w(px(120.))
                                            .text_center()
                                            .text_size(rems(0.74))
                                            .child(format_bytes(process.memory_bytes)),
                                    )
                                    .child(
                                        div()
                                            .w(px(100.))
                                            .text_center()
                                            .text_size(rems(0.74))
                                            .child(format!("{:.1}%", process.cpu_percent)),
                                    )
                            })),
                    )
                    .child(
                        v_flex()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .overflow_hidden()
                            .child(card_title(t!("network").to_string()))
                            .child(
                                h_flex()
                                    .min_h(px(46.))
                                    .items_center()
                                    .px_4()
                                    .child(info_row(
                                        t!("receive_rate").to_string(),
                                        snapshot.net_rx.clone(),
                                    ))
                                    .child(info_row(
                                        t!("send_rate").to_string(),
                                        snapshot.net_tx.clone(),
                                    )),
                            )
                            .child(
                                h_flex()
                                    .h(px(32.))
                                    .items_center()
                                    .px_4()
                                    .bg(cx.theme().muted)
                                    .text_size(rems(0.72))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(div().flex_1().child(t!("network_interface")))
                                    .child(div().w(px(120.)).text_center().child(t!("received")))
                                    .child(div().w(px(120.)).text_center().child(t!("sent")))
                                    .child(
                                        div().w(px(110.)).text_center().child(t!("receive_rate")),
                                    )
                                    .child(div().w(px(110.)).text_center().child(t!("send_rate"))),
                            )
                            .children(snapshot.network_interfaces.into_iter().enumerate().map(
                                |(index, interface)| {
                                    h_flex()
                                        .min_h(px(34.))
                                        .items_center()
                                        .px_4()
                                        .when(index % 2 == 1, |this| {
                                            this.bg(cx.theme().muted.opacity(0.35))
                                        })
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .text_size(rems(0.74))
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(interface.name),
                                        )
                                        .child(
                                            div()
                                                .w(px(120.))
                                                .text_center()
                                                .text_size(rems(0.74))
                                                .child(format_bytes(interface.received_bytes)),
                                        )
                                        .child(
                                            div()
                                                .w(px(120.))
                                                .text_center()
                                                .text_size(rems(0.74))
                                                .child(format_bytes(interface.transmitted_bytes)),
                                        )
                                        .child(
                                            div()
                                                .w(px(110.))
                                                .text_center()
                                                .text_size(rems(0.74))
                                                .child(format!(
                                                    "{}/s",
                                                    format_bytes(interface.receive_rate)
                                                )),
                                        )
                                        .child(
                                            div()
                                                .w(px(110.))
                                                .text_center()
                                                .text_size(rems(0.74))
                                                .child(format!(
                                                    "{}/s",
                                                    format_bytes(interface.transmit_rate)
                                                )),
                                        )
                                },
                            )),
                    )
                    .child(
                        v_flex()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .overflow_hidden()
                            .child(card_title(t!("file_system").to_string()))
                            .child(
                                h_flex()
                                    .h(px(32.))
                                    .items_center()
                                    .px_4()
                                    .bg(cx.theme().muted)
                                    .text_size(rems(0.72))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(div().flex_1().child(t!("mount_point")))
                                    .child(div().w(px(130.)).text_center().child(t!("total")))
                                    .child(div().w(px(130.)).text_center().child(t!("used")))
                                    .child(div().w(px(130.)).text_center().child(t!("available"))),
                            )
                            .children(snapshot.filesystems.into_iter().enumerate().map(
                                |(index, disk)| {
                                    let used =
                                        disk.total_bytes.saturating_sub(disk.available_bytes);
                                    h_flex()
                                        .min_h(px(32.))
                                        .items_center()
                                        .px_4()
                                        .when(index % 2 == 1, |this| {
                                            this.bg(cx.theme().muted.opacity(0.35))
                                        })
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .text_size(rems(0.74))
                                                .child(disk.mount),
                                        )
                                        .child(
                                            div()
                                                .w(px(130.))
                                                .text_center()
                                                .text_size(rems(0.74))
                                                .child(format_bytes(disk.total_bytes)),
                                        )
                                        .child(
                                            div()
                                                .w(px(130.))
                                                .text_center()
                                                .text_size(rems(0.74))
                                                .child(format_bytes(used)),
                                        )
                                        .child(
                                            div()
                                                .w(px(130.))
                                                .text_center()
                                                .text_size(rems(0.74))
                                                .child(format_bytes(disk.available_bytes)),
                                        )
                                },
                            )),
                    )
                    .child(div().h(px(24.)).flex_none()),
            )
    }

    pub(super) fn render_home_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                                        this.set_home_page(HomePage::Connections, cx);
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
                        h_flex()
                            .w_full()
                            .items_center()
                            .child(
                                div()
                                    .text_size(rems(1.25))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("overview_recent")),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("overview-edit-recent")
                                    .ghost()
                                    .small()
                                    .label(t!("edit").to_string())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.set_home_page(HomePage::Connections, cx);
                                    })),
                            ),
                    )
                    .child(
                        v_flex().w_full().gap_2().children(
                            recent_sessions
                                .into_iter()
                                .enumerate()
                                .map(|(ix, session)| {
                                    let connect_id = session.id.clone();
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
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.connect_saved_session(
                                                connect_id.clone(),
                                                window,
                                                cx,
                                            );
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
                                        this.show_settings_window(window, cx)
                                    })),
                            ),
                    ),
            )
    }

    pub(super) fn quick_command_parameter_indices(command: &str) -> Vec<usize> {
        (1..=5)
            .filter(|index| command.contains(&format!("[p{index}]")))
            .collect()
    }

    pub(super) fn select_quick_command(
        &mut self,
        category_id: String,
        command_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_quick_command = Some((category_id, command_id));
        for input in &self.quick_command_parameter_inputs {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.notify();
    }

    pub(super) fn execute_quick_command(
        &mut self,
        category_id: String,
        command_id: String,
        return_to_terminal: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_quick_command.as_ref() != Some(&(category_id.clone(), command_id.clone()))
        {
            self.select_quick_command(category_id.clone(), command_id.clone(), window, cx);
        }
        let command = self
            .config
            .quick_command_categories()
            .and_then(|categories| {
                categories
                    .iter()
                    .find(|category| category.id == category_id)
            })
            .and_then(|category| {
                category
                    .commands
                    .iter()
                    .find(|command| command.id == command_id)
            })
            .cloned();
        let Some(command) = command else {
            return;
        };
        let parameter_indices = Self::quick_command_parameter_indices(&command.command);
        let mut resolved = command.command.clone();
        for index in parameter_indices {
            let value = self.quick_command_parameter_inputs[index - 1]
                .read(cx)
                .value()
                .trim()
                .to_string();
            if value.is_empty() {
                self.status = t!("quick_command_parameter_required", index = index).into();
                self.quick_command_parameter_inputs[index - 1]
                    .read(cx)
                    .focus_handle(cx)
                    .focus(window, cx);
                cx.notify();
                return;
            }
            resolved = resolved.replace(&format!("[p{index}]"), &value);
        }
        self.send_terminal_input(format!("{resolved}\r").into_bytes(), window, cx);
        if return_to_terminal {
            self.home_page_open = false;
        }
        cx.notify();
    }

    pub(super) fn render_quick_command_detail(
        &self,
        return_to_terminal: bool,
        resizable_width: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let categories = self.config.quick_command_categories().unwrap_or_default();
        let selected =
            self.selected_quick_command
                .as_ref()
                .and_then(|(category_id, command_id)| {
                    categories
                        .iter()
                        .find(|category| category.id == *category_id)
                        .and_then(|category| {
                            category
                                .commands
                                .iter()
                                .find(|command| command.id == *command_id)
                                .map(|command| (category, command))
                        })
                });
        let Some((category, command)) = selected else {
            return v_flex()
                .self_stretch()
                .min_h(px(0.))
                .when(resizable_width, |this| this.w_full())
                .when(!resizable_width, |this| this.w(px(360.)).flex_none())
                .overflow_hidden()
                .items_center()
                .justify_center()
                .gap_2()
                .border_l_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().muted.opacity(0.12))
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(IconName::SquareTerminal).with_size(Size::Large))
                .child(t!("quick_command_select_hint"))
                .into_any_element();
        };
        let category_id = category.id.clone();
        let command_id = command.id.clone();
        let edit_category_id = category.id.clone();
        let edit_command_id = command.id.clone();
        let parameter_indices = Self::quick_command_parameter_indices(&command.command);

        v_flex()
            .self_stretch()
            .min_h(px(0.))
            .when(resizable_width, |this| this.w_full())
            .when(!resizable_width, |this| this.w(px(360.)).flex_none())
            .overflow_hidden()
            .border_l_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h(px(38.))
                    .flex_none()
                    .items_center()
                    .px_4()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().muted)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("quick_command_details"))
                    .child(div().flex_1())
                    .child(
                        Button::new("quick-command-detail-close")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected_quick_command = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        v_flex()
                            .size_full()
                            .overflow_y_scrollbar()
                            .p_4()
                            .gap_4()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .text_size(rems(1.1))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(command.name.clone()),
                                    )
                                    .child(
                                        h_flex()
                                            .flex_none()
                                            .items_center()
                                            .gap_1()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(cx.theme().muted)
                                            .text_size(rems(0.78))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                Icon::new(IconName::Folder).with_size(Size::XSmall),
                                            )
                                            .child(category.name.clone()),
                                    ),
                            )
                            .when(!command.remark.is_empty(), |this| {
                                this.child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(rems(0.78))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(t!("quick_command_remark")),
                                        )
                                        .child(
                                            div()
                                                .text_size(rems(0.85))
                                                .text_color(cx.theme().muted_foreground)
                                                .child(command.remark.clone()),
                                        ),
                                )
                            })
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(rems(0.78))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(t!("quick_command_content")),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .p_3()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(cx.theme().border)
                                            .bg(cx.theme().muted.opacity(0.22))
                                            .font_family("monospace")
                                            .text_size(rems(0.82))
                                            .child(command.command.clone()),
                                    ),
                            )
                            .when(!parameter_indices.is_empty(), |this| {
                                this.child(
                                    v_flex()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_size(rems(0.78))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(t!("quick_command_parameters")),
                                        )
                                        .children(parameter_indices.iter().map(|index| {
                                            v_flex()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_size(rems(0.75))
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(format!("[p{index}]")),
                                                )
                                                .child(Input::new(
                                                    &self.quick_command_parameter_inputs
                                                        [*index - 1],
                                                ))
                                        })),
                                )
                            }),
                    )
                    .overflow_hidden(),
            )
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .gap_2()
                    .p_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("quick-command-detail-edit")
                            .secondary()
                            .icon(IconName::Settings)
                            .label(t!("edit").to_string())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.show_quick_command_dialog(
                                    edit_category_id.clone(),
                                    Some(edit_command_id.clone()),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("quick-command-detail-send")
                            .primary()
                            .icon(IconName::SquareTerminal)
                            .label(t!("send").to_string())
                            .disabled(self.active_tab.is_none())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.execute_quick_command(
                                    category_id.clone(),
                                    command_id.clone(),
                                    return_to_terminal,
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_command_manager_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let categories = self
            .config
            .quick_command_categories()
            .unwrap_or_default()
            .to_vec();
        let selected_category = self
            .command_category_filter
            .clone()
            .filter(|selected| categories.iter().any(|category| category.id == *selected));
        let commands = categories
            .iter()
            .filter(|category| {
                selected_category
                    .as_deref()
                    .is_none_or(|selected| category.id == selected)
            })
            .flat_map(|category| {
                category
                    .commands
                    .iter()
                    .cloned()
                    .map(|command| (category.id.clone(), command))
            })
            .collect::<Vec<_>>();
        let command_count = categories
            .iter()
            .map(|category| category.commands.len())
            .sum::<usize>();
        let has_commands = !commands.is_empty();
        v_flex()
            .size_full()
            .p_6()
            .gap_5()
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
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
                    .child(div().flex_1())
                    .child(
                        Button::new("command-manager-new-category")
                            .secondary()
                            .icon(IconName::FolderOpen)
                            .label(t!("quick_command_new_category").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_quick_command_category_dialog(None, window, cx);
                            })),
                    )
                    .child(
                        Button::new("command-manager-new-command")
                            .primary()
                            .icon(IconName::Plus)
                            .label(t!("quick_command_new").to_string())
                            .disabled(selected_category.is_none())
                            .on_click(cx.listener({
                                let selected_category = selected_category.clone();
                                move |this, _, window, cx| {
                                    if let Some(category_id) = selected_category.clone() {
                                        this.show_quick_command_dialog(
                                            category_id,
                                            None,
                                            window,
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
                    .min_h(px(0.))
                    .h_full()
                    .items_stretch()
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
                                    .id("quick-command-category-all")
                                    .relative()
                                    .w_full()
                                    .cursor_pointer()
                                    .rounded_md()
                                    .bg(if selected_category.is_none() {
                                        cx.theme().tab_active
                                    } else {
                                        cx.theme().sidebar
                                    })
                                    .hover(|this| this.bg(cx.theme().secondary))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.command_category_filter = None;
                                        this.selected_quick_command = None;
                                        cx.notify();
                                    }))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .p_3()
                                            .pr(px(48.))
                                            .child(
                                                Icon::new(IconName::SquareTerminal)
                                                    .with_size(Size::Small),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .child(t!("quick_command_all")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .right(px(12.))
                                            .bottom_0()
                                            .w(px(36.))
                                            .flex()
                                            .items_center()
                                            .child(
                                                div()
                                                    .w_full()
                                                    .text_right()
                                                    .font_family("monospace")
                                                    .text_size(rems(0.8))
                                                    .text_color(
                                                        cx.theme().muted_foreground,
                                                    )
                                                    .child(command_count.to_string()),
                                            ),
                                    ),
                            )
                            .children(categories.iter().enumerate().map(|(index, category)| {
                                let category_id_for_select = category.id.clone();
                                let category_id_for_menu = category.id.clone();
                                let selected = selected_category.as_deref()
                                    == Some(category.id.as_str());
                                div()
                                    .id(("quick-command-category", index))
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
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.command_category_filter =
                                            Some(category_id_for_select.clone());
                                        this.selected_quick_command = None;
                                        cx.notify();
                                    }))
                                    .context_menu({
                                        let view = cx.entity();
                                        move |menu, window, _| {
                                            menu.item(
                                                PopupMenuItem::new(t!("rename").to_string())
                                                    .on_click(window.listener_for(&view, {
                                                        let category_id =
                                                            category_id_for_menu.clone();
                                                        move |this, _, window, cx| {
                                                            this.show_quick_command_category_dialog(
                                                                Some(category_id.clone()),
                                                                window,
                                                                cx,
                                                            );
                                                        }
                                                    })),
                                            )
                                            .item(
                                                PopupMenuItem::new(t!("delete").to_string())
                                                    .on_click(window.listener_for(&view, {
                                                        let category_id =
                                                            category_id_for_menu.clone();
                                                        move |this, _, _, cx| {
                                                            this.config
                                                                .remove_quick_command_category(
                                                                    &category_id,
                                                                );
                                                            if this.command_category_filter.as_deref()
                                                                == Some(category_id.as_str())
                                                            {
                                                                this.command_category_filter = None;
                                                            }
                                                            if this
                                                                .selected_quick_command
                                                                .as_ref()
                                                                .is_some_and(|(selected_category, _)| {
                                                                    selected_category == &category_id
                                                                })
                                                            {
                                                                this.selected_quick_command = None;
                                                            }
                                                            this.mark_config_preferences_dirty();
                                                            cx.notify();
                                                        }
                                                    })),
                                            )
                                        }
                                    })
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .p_3()
                                            .pr(px(48.))
                                            .child(
                                                Icon::new(IconName::Folder)
                                                    .with_size(Size::Small),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .text_ellipsis()
                                                    .child(category.name.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .right(px(12.))
                                            .bottom_0()
                                            .w(px(36.))
                                            .flex()
                                            .items_center()
                                            .child(
                                                div()
                                                    .w_full()
                                                    .text_right()
                                                    .font_family("monospace")
                                                    .text_size(rems(0.8))
                                                    .text_color(
                                                        cx.theme().muted_foreground,
                                                    )
                                                    .child(
                                                        category.commands.len().to_string(),
                                                    ),
                                            ),
                                    )
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
                                v_flex()
                                    .id("command-manager-list")
                                    .relative()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .track_scroll(&self.command_manager_scroll_handle)
                                    .overflow_y_scroll()
                                    .vertical_scrollbar(&self.command_manager_scroll_handle)
                                    .children(commands.into_iter().enumerate().map(
                                        |(index, (category_id, command))| {
                                            let run_command = command.command.clone();
                                            let select_category_id = category_id.clone();
                                            let select_command_id = command.id.clone();
                                            let selected = self.selected_quick_command.as_ref()
                                                == Some(&(category_id.clone(), command.id.clone()));
                                            let menu_category_id = category_id.clone();
                                            let menu_command = command.clone();
                                            let categories_for_menu = categories.clone();
                                            h_flex()
                                                .id(("quick-command-manager-row", index))
                                                .flex_none()
                                                .min_h(px(52.))
                                                .items_center()
                                                .gap_3()
                                                .px_4()
                                                .cursor_pointer()
                                                .border_b_1()
                                                .border_color(cx.theme().border.opacity(0.55))
                                                .when(selected, |this| {
                                                    this.bg(cx.theme().tab_active)
                                                })
                                                .when(!selected && index % 2 == 1, |this| {
                                                    this.bg(cx.theme().muted.opacity(0.2))
                                                })
                                                .hover(|this| this.bg(cx.theme().secondary))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                                        if event.click_count >= 2 {
                                                            this.execute_quick_command(
                                                                select_category_id.clone(),
                                                                select_command_id.clone(),
                                                                true,
                                                                window,
                                                                cx,
                                                            );
                                                        } else {
                                                            this.select_quick_command(
                                                                select_category_id.clone(),
                                                                select_command_id.clone(),
                                                                window,
                                                                cx,
                                                            );
                                                        }
                                                    }),
                                                )
                                                .context_menu({
                                                    let view = cx.entity();
                                                    move |mut menu, window, cx| {
                                                        menu = menu
                                                            .item(
                                                                PopupMenuItem::new(t!("edit").to_string())
                                                                    .on_click(window.listener_for(&view, {
                                                                        let category_id = menu_category_id.clone();
                                                                        let command_id = menu_command.id.clone();
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
                                                            .item(
                                                                PopupMenuItem::new(t!("clone").to_string())
                                                                    .on_click(window.listener_for(&view, {
                                                                        let category_id = menu_category_id.clone();
                                                                        let command = menu_command.clone();
                                                                        move |this, _, _, cx| {
                                                                            let mut duplicate = command.clone();
                                                                            duplicate.id = uuid::Uuid::new_v4().to_string();
                                                                            duplicate.name = t!("quick_command_copy_name", name = duplicate.name).to_string();
                                                                            this.config.upsert_quick_command(&category_id, duplicate);
                                                                            this.mark_config_preferences_dirty();
                                                                            cx.notify();
                                                                        }
                                                                    })),
                                                            );
                                                        menu = menu.submenu(
                                                            t!("quick_command_move_to").to_string(),
                                                            window,
                                                            cx,
                                                            {
                                                                let view = view.clone();
                                                                let source_category_id = menu_category_id.clone();
                                                                let command_id = menu_command.id.clone();
                                                                let categories = categories_for_menu.clone();
                                                                move |mut submenu, window, _| {
                                                                    for category in &categories {
                                                                        if category.id == source_category_id {
                                                                            continue;
                                                                        }
                                                                        let target_category_id = category.id.clone();
                                                                        let source_category_id = source_category_id.clone();
                                                                        let command_id = command_id.clone();
                                                                        submenu = submenu.item(
                                                                            PopupMenuItem::new(category.name.clone())
                                                                                .on_click(window.listener_for(&view, move |this, _, _, cx| {
                                                                                    this.config.move_quick_command(
                                                                                        &source_category_id,
                                                                                        &target_category_id,
                                                                                        &command_id,
                                                                                    );
                                                                                    this.selected_quick_command = Some((target_category_id.clone(), command_id.clone()));
                                                                                    this.command_category_filter = Some(target_category_id.clone());
                                                                                    this.mark_config_preferences_dirty();
                                                                                    cx.notify();
                                                                                })),
                                                                        );
                                                                    }
                                                                    submenu
                                                                }
                                                            },
                                                        );
                                                        menu.separator().item(
                                                            PopupMenuItem::new(t!("delete").to_string())
                                                                .on_click(window.listener_for(&view, {
                                                                    let category_id = menu_category_id.clone();
                                                                    let command_id = menu_command.id.clone();
                                                                    move |this, _, _, cx| {
                                                                        this.config.remove_quick_command(&category_id, &command_id);
                                                                        if this.selected_quick_command.as_ref()
                                                                            == Some(&(category_id.clone(), command_id.clone()))
                                                                        {
                                                                            this.selected_quick_command = None;
                                                                        }
                                                                        this.mark_config_preferences_dirty();
                                                                        cx.notify();
                                                                    }
                                                                })),
                                                        )
                                                    }
                                                })
                                                .child(
                                                    v_flex()
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .gap_1()
                                                        .child(
                                                            div()
                                                                .font_weight(FontWeight::MEDIUM)
                                                                .child(command.name),
                                                        )
                                                        .child(
                                                            div()
                                                                .id((
                                                                    "quick-command-content",
                                                                    index,
                                                                ))
                                                                .w_full()
                                                                .min_w(px(0.))
                                                                .overflow_hidden()
                                                                .whitespace_nowrap()
                                                                .text_ellipsis()
                                                                .text_size(rems(0.833))
                                                                .font_family("monospace")
                                                                .tooltip({
                                                                    let command =
                                                                        run_command.clone();
                                                                    move |window, cx| {
                                                                        gpui_component::tooltip::Tooltip::new(
                                                                            command.clone(),
                                                                        )
                                                                        .build(window, cx)
                                                                    }
                                                                })
                                                                .child(command.command),
                                                        ),
                                                )
                                        },
                                    ))
                                    .when(!has_commands, |this| {
                                        this.child(
                                            v_flex()
                                                .size_full()
                                                .items_center()
                                                .justify_center()
                                                .gap_3()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(
                                                    Icon::new(IconName::SquareTerminal)
                                                        .with_size(Size::Large),
                                                )
                                                .child(t!("command_manager_empty_title")),
                                        )
                                    }),
                            ),
                    )
                    .child(self.render_quick_command_detail(true, false, cx)),
            )
    }

    pub(super) fn render_settings_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let settings_inputs = crate::app::settings::form::SettingsInputs::from_main(self);
        div()
            .size_full()
            .p_6()
            .bg(cx.theme().muted.opacity(0.18))
            .child(
                div()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .child(self.render_settings_content(
                        &view,
                        "settings-page",
                        &self.focus_handle,
                        &settings_inputs,
                        cx,
                    )),
            )
    }

    pub(super) fn recent_usage_label(last_used: Option<&str>) -> String {
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

    pub(super) fn render_connection_manager_page(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
        let manager_state = self.connection_manager_state.read(cx).clone();

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
            .p_6()
            .gap_5()
            .child(
                h_flex()
                    .flex_none()
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
                                this.open_connection_operation_window(
                                    crate::app::connection_manager::operation_window::ConnectionOperation::EditGroup {
                                        original: None,
                                        parent: None,
                                    },
                                    window,
                                    cx,
                                );
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
                            .min_h(px(0.))
                            .id("connection-manager-groups")
                            .track_scroll(&self.connection_scroll_handle)
                            .p_2()
                            .gap_1()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().sidebar)
                            .overflow_y_scrollbar()
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
                            .children(
                                groups
                                    .into_iter()
                                    .enumerate()
                                    .filter(|(_, group)| {
                                        group
                                            .rsplit_once('/')
                                            .is_none_or(|(parent, _)| manager_state.expanded.contains(parent))
                                    })
                                    .map(|(ix, group)| {
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
                                let expanded = manager_state.expanded.contains(&group);
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
                                                        let parent = group_name
                                                            .rsplit_once('/')
                                                            .map(|(parent, _)| parent.to_string());
                                                        this.open_connection_operation_window(
                                                            crate::app::connection_manager::operation_window::ConnectionOperation::EditGroup {
                                                                original: Some(group_name.clone()),
                                                                parent,
                                                            },
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
                                                            this.open_connection_operation_window(
                                                                crate::app::connection_manager::operation_window::ConnectionOperation::EditGroup {
                                                                    original: None,
                                                                    parent: Some(group_name.clone()),
                                                                },
                                                                window,
                                                                cx,
                                                            );
                                                        }
                                                    })),
                                            )
                                            .item(
                                                PopupMenuItem::new(t!("connection_group_delete").to_string())
                                                    .on_click(window.listener_for(&view, {
                                                        let group_name = group_name.clone();
                                                        move |this, _, _, cx| {
                                                            this.config.remove_connection_group(&group_name);
                                                            if let Err(err) = crate::app::config_persistence::save_full(&this.config) {
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
                                                            this.open_connection_operation_window(
                                                                crate::app::connection_manager::operation_window::ConnectionOperation::MoveGroup {
                                                                    group: group_name.clone(),
                                                                },
                                                                window,
                                                                cx,
                                                            );
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
                                                div()
                                                    .id(("connection-group-toggle", ix))
                                                    .w(px(16.))
                                                    .on_click(cx.listener({
                                                        let group_name = group_name.clone();
                                                        move |this, _, _, cx| {
                                                            this.connection_manager_state.update(cx, |state, _| {
                                                                state.toggle_group(&group_name);
                                                            });
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .child(if expanded {
                                                        Icon::new(IconName::ChevronDown).with_size(Size::Small)
                                                    } else {
                                                        Icon::new(IconName::ChevronRight).with_size(Size::Small)
                                                    }),
                                            )
                                            .child(Icon::new(IconName::Folder).with_size(Size::Small))
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
                            }),
                            ),
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
                                            let connect_id = session.id.clone();
                                            let double_click_id = connect_id.clone();
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
                                                .cursor_pointer()
                                                .hover(|this| this.bg(cx.theme().muted))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener({
                                                        move |this, event: &MouseDownEvent, window, cx| {
                                                            if event.click_count >= 2 {
                                                                this.connect_saved_session(
                                                                    double_click_id.clone(),
                                                                    window,
                                                                    cx,
                                                                );
                                                            }
                                                        }
                                                    }),
                                                )
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
                                                                        if let Err(error) = crate::app::config_persistence::save_full(&this.config) {
                                                                            tracing::warn!("failed to move connection to ungrouped: {error:#}");
                                                                        }
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
                                                                            if let Err(error) = crate::app::config_persistence::save_full(&this.config) {
                                                                                tracing::warn!("failed to move connection to group: {error:#}");
                                                                            }
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
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            |_, _, cx| cx.stop_propagation(),
                                                        )
                                                        .child(
                                                            Button::new(format!(
                                                                "connection-manager-connect-{ix}"
                                                            ))
                                                            .small()
                                                            .primary()
                                                            .label(t!("connect").to_string())
                                                            .on_click(cx.listener(
                                                                move |this, _, window, cx| {
                                                                    this.connect_saved_session(
                                                                        connect_id.clone(),
                                                                        window,
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
                                                            .danger()
                                                            .label(t!("delete").to_string())
                                                            .on_click(cx.listener(
                                                                move |this, _, window, cx| {
                                                                    this.request_saved_session_deletion(
                                                                        delete_id.clone(),
                                                                        window,
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
    pub(crate) fn connection_group_tree_order(groups: Vec<String>) -> Vec<String> {
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

    pub(super) fn render_key_manager_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let keys = self.managed_keys.clone();
        let sessions = self.config.sessions().to_vec();
        let has_keys = !keys.is_empty();
        let rename_input = self.key_inline_input.clone();

        v_flex()
            .size_full()
            .p_6()
            .gap_5()
            .child(
                h_flex()
                    .flex_none()
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
                                        h_flex()
                                            .flex_1()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                Icon::new(IconName::Folder).with_size(Size::Small),
                                            )
                                            .child(Input::new(&rename_input).small().flex_1())
        .with_animation(
            ElementId::NamedInteger("settings-content-fade".into(), self.main_view_key()),
            Animation::new(Duration::from_millis(200)).with_easing(gpui::ease_out_quint()),
            |this, delta| this.opacity(delta * delta),
        )
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
                                                    .icon(IconName::Replace)
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
                                                            crate::app::input_focus::defer_focus_input_at_end(
                                                                this.key_inline_input.clone(),
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
                                                    .label(t!("key_delete").to_string())
                                                    .on_click({
                                                        let key_id = key_id.clone();
                                                        cx.listener(move |this, _, window, cx| {
                                                            this.request_managed_key_deletion(
                                                                key_id.clone(),
                                                                window,
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

    pub(super) fn render_overview_nav_item(
        &self,
        id: &'static str,
        page: HomePage,
        icon: IconName,
        label: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.home_page == page;
        let was_active = self.prev_home_page == page && !active;
        let epoch = self.home_page_epoch;
        let target_background = if active {
            cx.theme().tab_active
        } else {
            cx.theme().sidebar
        };
        let target_text = if active {
            cx.theme().primary
        } else {
            cx.theme().foreground
        };
        let start_background = if was_active {
            cx.theme().tab_active
        } else {
            target_background
        };
        let start_text = if was_active {
            cx.theme().primary
        } else {
            target_text
        };
        let hover_background = if active {
            cx.theme().tab_active
        } else {
            cx.theme().secondary
        };

        div()
            .id(id)
            .w_full()
            .h(px(42.))
            .flex_none()
            .cursor_pointer()
            .rounded_md()
            .bg(target_background)
            .text_color(target_text)
            .hover(move |this| this.bg(hover_background))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_home_page(page, cx);
            }))
            .child(
                h_flex()
                    .size_full()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .child(
                        h_flex()
                            .w(px(18.))
                            .flex_none()
                            .justify_center()
                            .child(Icon::new(icon).with_size(Size::Small)),
                    )
                    .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(label)),
            )
            .with_animation(
                ElementId::NamedInteger(format!("{id}-nav-transition").into(), epoch),
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                move |this, delta| {
                    this.bg(lerp_hsla(start_background, target_background, delta))
                        .text_color(lerp_hsla(start_text, target_text, delta))
                },
            )
    }

    pub(super) fn render_overview_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_update = matches!(
            self.update_runtime.status,
            Some(crate::app::updater::UpdateStatus::UpdateAvailable(_))
                | Some(crate::app::updater::UpdateStatus::DownloadCancelled(_))
                | Some(crate::app::updater::UpdateStatus::DownloadFailed(_, _))
        );
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
                    .id("overview-brand-version")
                    .w_full()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.show_update_dialog(window, cx)),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_size(rems(1.5))
                            .text_color(cx.theme().primary)
                            .child(t!("app_name")),
                    )
                    .child(
                        div()
                            .relative()
                            .px_2()
                            .py(px(2.))
                            .rounded_full()
                            .bg(cx.theme().muted)
                            .text_size(rems(0.65))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .when(has_update, |this| {
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
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(self.render_overview_nav_item(
                        "overview-sidebar-overview",
                        HomePage::Overview,
                        IconName::SquareTerminal,
                        t!("overview").to_string(),
                        cx,
                    ))
                    .child(self.render_overview_nav_item(
                        "overview-sidebar-connections",
                        HomePage::Connections,
                        IconName::Network,
                        t!("overview_connections").to_string(),
                        cx,
                    ))
                    .child(self.render_overview_nav_item(
                        "overview-sidebar-commands",
                        HomePage::Commands,
                        IconName::SquareTerminal,
                        t!("command_manager").to_string(),
                        cx,
                    ))
                    .child(self.render_overview_nav_item(
                        "overview-sidebar-key-management",
                        HomePage::KeyManager,
                        IconName::Folder,
                        t!("overview_key_manager").to_string(),
                        cx,
                    ))
                    .child(self.render_overview_nav_item(
                        "overview-sidebar-settings-link",
                        HomePage::Settings,
                        IconName::Settings,
                        t!("settings").to_string(),
                        cx,
                    )),
            )
            .child(div().flex_1())
    }
}
