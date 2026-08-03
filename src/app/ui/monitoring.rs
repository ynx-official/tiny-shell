use super::*;
use crate::app::monitoring::{format_network_axis, nice_network_scale, smooth_monitoring_series};

impl TinyShell {
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

        let cpu_pct = self.monitoring.system.cpu_percent;
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
        let mem_pct = self.monitoring.system.mem_percent;
        let swap_pct = self.monitoring.system.swap_percent;
        let mem_detail = self.monitoring.system.mem_detail.clone();
        let swap_detail = self.monitoring.system.swap_detail.clone();
        let net_rx = self.monitoring.system.net_rx.clone();
        let net_tx = self.monitoring.system.net_tx.clone();

        let (disk_used, disk_total) = self
            .monitoring
            .system
            .disks
            .iter()
            .fold((0u64, 0u64), |(u, t), d| {
                (u + (d.total_bytes - d.available_bytes), t + d.total_bytes)
            });
        let disk_pct = if disk_total > 0 {
            disk_used as f64 / disk_total as f64 * 100.0
        } else {
            0.0
        };

        let cpu_spark_data = self.monitoring.cpu_history.clone();
        let net_rx_history = self.monitoring.net_rx_history.clone();
        let net_tx_history = self.monitoring.net_tx_history.clone();
        let disks = self.monitoring.system.disks.clone();
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
            .when(self.monitoring.system.total_swap > 0, |this| {
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

    pub(super) fn render_sidebar_monitoring_panel(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let cpu_pct = self.monitoring.system.cpu_percent.clamp(0.0, 1.0);
        let mem_pct = self.monitoring.system.mem_percent.clamp(0.0, 1.0);
        let swap_pct = self.monitoring.system.swap_percent.clamp(0.0, 1.0);
        let cpu_progress_pct = self.monitoring.animated_cpu_percent.clamp(0.0, 1.0);
        let mem_progress_pct = self.monitoring.animated_mem_percent.clamp(0.0, 1.0);
        let swap_progress_pct = self.monitoring.animated_swap_percent.clamp(0.0, 1.0);
        let cpu_value_pct = self.monitoring.system.cpu_percent.clamp(0.0, 1.0);
        let mem_value_pct = self.monitoring.system.mem_percent.clamp(0.0, 1.0);
        let swap_value_pct = self.monitoring.system.swap_percent.clamp(0.0, 1.0);
        let metric_color = |value: f32| {
            if value >= 0.75 {
                Hsla::from(gpui::rgb(0xE5484D))
            } else if value >= 0.65 {
                Hsla::from(gpui::rgb(0xE7A008))
            } else {
                Hsla::from(gpui::rgb(0x1586F5))
            }
        };
        let metric_track_color = if cx.theme().background.l > 0.5 {
            hsla(0.0, 0.0, 0.0, 0.1)
        } else {
            hsla(0.0, 0.0, 1.0, 0.14)
        };
        let metric_bar = |id: &'static str, value: f32, color: Hsla| {
            div()
                .id(id)
                .relative()
                .w_full()
                .h(px(4.))
                .overflow_hidden()
                .rounded(px(999.))
                .bg(metric_track_color)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .h_full()
                        .w(relative(value.clamp(0.0, 1.0)))
                        .rounded(px(999.))
                        .bg(color),
                )
                .into_any_element()
        };
        let cpu_color = metric_color(cpu_pct);
        let mem_color = metric_color(mem_pct);
        let swap_color = metric_color(swap_pct);
        let mem_value_detail = format!(
            "{} · {:.0}%",
            self.monitoring.system.mem_detail,
            mem_value_pct * 100.0
        );
        let swap_value_detail = format!(
            "{} · {:.0}%",
            self.monitoring.system.swap_detail,
            swap_value_pct * 100.0
        );
        let net_color = Hsla::from(gpui::rgb(0x1586F5));
        let net_tx_color = cx.theme().danger;
        let net_grid_color = cx.theme().border.opacity(0.18);
        let muted_fg = cx.theme().muted_foreground;
        let selected_network_interface = self.monitoring.selected_network_interface.clone();
        let selected_network = selected_network_interface.as_ref().and_then(|selected| {
            self.monitoring
                .system
                .network_interfaces
                .iter()
                .find(|interface| &interface.name == selected)
        });
        let selected_network_label = selected_network_interface
            .clone()
            .unwrap_or_else(|| t!("total").to_string());
        let selected_rx_rate = selected_network
            .map(|interface| interface.receive_rate)
            .unwrap_or(self.monitoring.system.net_rx_rate);
        let selected_tx_rate = selected_network
            .map(|interface| interface.transmit_rate)
            .unwrap_or(self.monitoring.system.net_tx_rate);
        let selected_rx_values = selected_network_interface
            .as_ref()
            .and_then(|selected| self.monitoring.network_interface_histories.get(selected))
            .map(|history| history.receive.iter().copied().collect::<Vec<_>>())
            .unwrap_or_else(|| self.monitoring.net_rx_history.iter().copied().collect());
        let selected_tx_values = selected_network_interface
            .as_ref()
            .and_then(|selected| self.monitoring.network_interface_histories.get(selected))
            .map(|history| history.transmit.iter().copied().collect::<Vec<_>>())
            .unwrap_or_else(|| self.monitoring.net_tx_history.iter().copied().collect());
        let selected_rx_history = smooth_monitoring_series(&selected_rx_values);
        let selected_tx_history = smooth_monitoring_series(&selected_tx_values);
        let network_chart_max = nice_network_scale(
            selected_rx_history
                .iter()
                .chain(selected_tx_history.iter())
                .copied()
                .fold(0.0f32, f32::max),
        );
        let network_axis_labels = [
            format_network_axis(network_chart_max),
            format_network_axis(network_chart_max * 2.0 / 3.0),
            format_network_axis(network_chart_max / 3.0),
        ];
        let network_interface_names = self
            .monitoring
            .system
            .network_interfaces
            .iter()
            .map(|interface| interface.name.clone())
            .collect::<Vec<_>>();
        let process_view = self.monitoring.process_view;
        let process_view_epoch = process_view as u64;
        let mut displayed_processes = self.monitoring.system.processes.clone();
        match process_view {
            ProcessView::Memory => {
                displayed_processes.sort_by_key(|process| Reverse(process.memory_bytes))
            }
            ProcessView::Cpu => displayed_processes.sort_by(|left, right| {
                right
                    .cpu_percent
                    .partial_cmp(&left.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            ProcessView::Activity => {
                let max_memory = displayed_processes
                    .iter()
                    .map(|process| process.memory_bytes)
                    .max()
                    .unwrap_or(1) as f32;
                displayed_processes.sort_by(|left, right| {
                    let left_score =
                        left.cpu_percent * 10.0 + left.memory_bytes as f32 / max_memory * 100.0;
                    let right_score =
                        right.cpu_percent * 10.0 + right.memory_bytes as f32 / max_memory * 100.0;
                    right_score
                        .partial_cmp(&left_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
        }
        displayed_processes.truncate(4);
        let no_processes = displayed_processes.is_empty();

        v_flex()
            .gap(px(6.))
            .w_full()
            .h_full()
            .min_h(px(0.))
            .overflow_hidden()
            .p_1()
            .child(
                v_flex()
                    .flex_none()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .h(px(28.))
                            .items_center()
                            .px_2()
                            .bg(cx.theme().muted)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(rems(0.78))
                            .child(t!("resources")),
                    )
                    .child(
                        v_flex()
                            .px_2()
                            .py_2()
                            .gap_2()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .text_size(rems(0.75))
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(t!("cpu")),
                                            )
                                            .child(
                                                div()
                                                    .text_color(muted_fg)
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(format!(
                                                        "{:.1}%",
                                                        cpu_value_pct * 100.0
                                                    )),
                                            ),
                                    )
                                    .child(
                                        metric_bar(
                                            "sidebar-cpu",
                                            cpu_progress_pct,
                                            cpu_color,
                                        ),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .text_size(rems(0.75))
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(t!("mem")),
                                            )
                                            .child(
                                                div()
                                                    .text_color(muted_fg)
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(mem_value_detail),
                                            ),
                                    )
                                    .child(
                                        metric_bar(
                                            "sidebar-mem",
                                            mem_progress_pct,
                                            mem_color,
                                        ),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .text_size(rems(0.75))
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(t!("swap")),
                                            )
                                            .child(
                                                div()
                                                    .text_color(muted_fg)
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(swap_value_detail),
                                            ),
                                    )
                                    .child(
                                        metric_bar(
                                            "sidebar-swap",
                                            swap_progress_pct,
                                            swap_color,
                                        ),
                                    ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .flex_none()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .h(px(26.))
                            .items_center()
                            .p(px(2.))
                            .gap_1()
                            .bg(cx.theme().muted)
                            .child(
                                div()
                                    .id("process-view-memory")
                                    .flex_1()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .rounded_sm()
                                    .bg(if process_view == ProcessView::Memory {
                                        cx.theme().background
                                    } else {
                                        cx.theme().muted
                                    })
                                    .text_color(if process_view == ProcessView::Memory {
                                        cx.theme().foreground
                                    } else {
                                        muted_fg
                                    })
                                    .text_center()
                                    .text_size(rems(0.7))
                                    .font_weight(if process_view == ProcessView::Memory {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.monitoring.process_view = ProcessView::Memory;
                                        cx.notify();
                                    }))
                                    .child(t!("process_memory"))
                                    .with_animation(
                                        ElementId::NamedInteger(
                                            "process-view-transition".into(),
                                            process_view_epoch,
                                        ),
                                        Animation::new(Duration::from_millis(180))
                                            .with_easing(ease_out_quint()),
                                        |this, delta| this.opacity(0.45 + 0.55 * delta),
                                    ),
                            )
                            .child(
                                div()
                                    .id("process-view-cpu")
                                    .flex_1()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .rounded_sm()
                                    .bg(if process_view == ProcessView::Cpu {
                                        cx.theme().background
                                    } else {
                                        cx.theme().muted
                                    })
                                    .text_color(if process_view == ProcessView::Cpu {
                                        cx.theme().foreground
                                    } else {
                                        muted_fg
                                    })
                                    .text_center()
                                    .text_size(rems(0.7))
                                    .font_weight(if process_view == ProcessView::Cpu {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.monitoring.process_view = ProcessView::Cpu;
                                        cx.notify();
                                    }))
                                    .child(t!("cpu"))
                                    .with_animation(
                                        ElementId::NamedInteger(
                                            "process-view-transition".into(),
                                            process_view_epoch,
                                        ),
                                        Animation::new(Duration::from_millis(180))
                                            .with_easing(ease_out_quint()),
                                        |this, delta| this.opacity(0.45 + 0.55 * delta),
                                    ),
                            )
                            .child(
                                div()
                                    .id("process-view-activity")
                                    .flex_1()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .rounded_sm()
                                    .bg(if process_view == ProcessView::Activity {
                                        cx.theme().background
                                    } else {
                                        cx.theme().muted
                                    })
                                    .text_color(if process_view == ProcessView::Activity {
                                        cx.theme().foreground
                                    } else {
                                        muted_fg
                                    })
                                    .text_center()
                                    .text_size(rems(0.7))
                                    .font_weight(if process_view == ProcessView::Activity {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.monitoring.process_view = ProcessView::Activity;
                                        cx.notify();
                                    }))
                                    .child(t!("process_command"))
                                    .with_animation(
                                        ElementId::NamedInteger(
                                            "process-view-transition".into(),
                                            process_view_epoch,
                                        ),
                                        Animation::new(Duration::from_millis(180))
                                            .with_easing(ease_out_quint()),
                                        |this, delta| this.opacity(0.45 + 0.55 * delta),
                                    ),
                            ),
                    )
                    .children(displayed_processes.into_iter().enumerate().map(
                        |(index, process)| {
                            h_flex()
                                .h(px(23.))
                                .items_center()
                                .when(index % 2 == 1, |this| {
                                    this.bg(cx.theme().muted.opacity(0.22))
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .text_center()
                                        .text_size(rems(0.69))
                                        .child(format_bytes(process.memory_bytes)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .text_center()
                                        .text_size(rems(0.69))
                                        .child(format!("{:.1}%", process.cpu_percent)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .px_1()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_center()
                                        .text_size(rems(0.69))
                                        .child(process.command),
                                )
                        },
                    ))
                    .when(no_processes, |this| {
                        this.child(
                            div()
                                .h(px(26.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(rems(0.7))
                                .text_color(muted_fg)
                                .child(t!("no_process_data")),
                        )
                    }),
            )
            .child(
                v_flex()
                    .flex_none()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .h(px(36.))
                            .px_3()
                            .items_center()
                            .justify_between()
                            .bg(cx.theme().muted)
                            .child(
                                div()
                                    .text_size(rems(0.82))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("net").to_string()),
                            )
                            .child(
                                Button::new("sidebar-network-selector")
                                    .secondary()
                                    .xsmall()
                                    .label(selected_network_label)
                                    .icon(IconName::ChevronDown)
                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                        let view = cx.entity();
                                        move |mut menu, window, cx| {
                                            let selected =
                                                view.read(cx).monitoring.selected_network_interface.clone();
                                            menu = menu.item(
                                                PopupMenuItem::new(t!("total").to_string())
                                                    .checked(selected.is_none())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.monitoring.selected_network_interface = None;
                                                            cx.notify();
                                                        },
                                                    )),
                                            );
                                            for name in network_interface_names.clone() {
                                                let selected_name: String = name.clone();
                                                menu = menu.item(
                                                    PopupMenuItem::new(name.clone())
                                                        .checked(selected.as_ref() == Some(&name))
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            move |this, _, _, cx| {
                                                                this.monitoring.selected_network_interface =
                                                                    Some(selected_name.clone());
                                                                cx.notify();
                                                            },
                                                        )),
                                                );
                                            }
                                            menu
                                        }
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_4()
                            .px_3()
                            .pt_2()
                            .text_size(rems(0.74))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .size(px(7.))
                                            .rounded_full()
                                            .bg(net_tx_color),
                                    )
                                    .child(
                                        div().child(format!(
                                            "{} {}/s",
                                            t!("upload"),
                                            format_bytes(selected_tx_rate)
                                        )),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .size(px(7.))
                                            .rounded_full()
                                            .bg(net_color),
                                    )
                                    .child(
                                        div().child(format!(
                                            "{} {}/s",
                                            t!("download"),
                                            format_bytes(selected_rx_rate)
                                        )),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .h(px(68.))
                            .p_1()
                            .mx_2()
                            .mb_3()
                            .rounded_md()
                            .bg(cx.theme().muted.opacity(0.18))
                            .child(
                                h_flex()
                                    .size_full()
                                    .gap_1()
                                    .child(
                                        v_flex()
                                            .w(px(28.))
                                            .h_full()
                                            .flex_none()
                                            .py_1()
                                            .justify_between()
                                            .text_size(rems(0.58))
                                            .text_color(muted_fg)
                                            .child(
                                                div()
                                                    .w_full()
                                                    .text_right()
                                                    .child(network_axis_labels[0].clone()),
                                            )
                                            .child(
                                                div()
                                                    .w_full()
                                                    .text_right()
                                                    .child(network_axis_labels[1].clone()),
                                            )
                                            .child(
                                                div()
                                                    .w_full()
                                                    .text_right()
                                                    .child(network_axis_labels[2].clone()),
                                            )
                                            .child(
                                                div().w_full().text_right().child("0K"),
                                            ),
                                    )
                                    .child(canvas(
                                    move |bounds, _window, _cx| {
                                        let point_count = selected_rx_history
                                            .len()
                                            .max(selected_tx_history.len());
                                        if point_count < 2 {
                                            return None;
                                        }
                                        let max_value = network_chart_max.max(1.0);
                                        let plot_left = bounds.origin.x + px(1.);
                                        let plot_right = bounds.origin.x + bounds.size.width - px(2.);
                                        let plot_top = bounds.origin.y + px(5.);
                                        let baseline = bounds.origin.y + bounds.size.height - px(5.);
                                        let plot_width = plot_right - plot_left;
                                        let plot_height = baseline - plot_top;
                                        let mut paths = Vec::new();
                                        for step in 1..=2 {
                                            let y = plot_top + plot_height * step as f32 / 3.0;
                                            let mut guide = PathBuilder::stroke(px(1.))
                                                .dash_array(&[px(3.), px(4.)]);
                                            guide.move_to(point(plot_left, y));
                                            guide.line_to(point(plot_right, y));
                                            if let Ok(path) = guide.build() {
                                                paths.push((path, net_grid_color));
                                            }
                                        }
                                        let mut fills = Vec::new();
                                        let mut strokes = Vec::new();
                                        for (values, color) in [
                                            (&selected_tx_history, net_tx_color),
                                            (&selected_rx_history, net_color),
                                        ] {
                                            if values.len() < 2 {
                                                continue;
                                            }
                                            let points = values
                                                .iter()
                                                .enumerate()
                                                .map(|(index, value)| {
                                                    let x = plot_left
                                                        + plot_width * index as f32
                                                            / (values.len() - 1) as f32;
                                                    let normalized = if *value > 0.0 {
                                                        (*value / max_value * 0.92).max(0.025)
                                                    } else {
                                                        0.0
                                                    };
                                                    let y = baseline - plot_height * normalized;
                                                    point(x, y)
                                                })
                                                .collect::<Vec<_>>();

                                            let append_curve = |builder: &mut PathBuilder| {
                                                builder.move_to(points[0]);
                                                for pair in points.windows(2) {
                                                    let previous = pair[0];
                                                    let current = pair[1];
                                                    let midpoint = point(
                                                        previous.x
                                                            + (current.x - previous.x) * 0.5,
                                                        previous.y
                                                            + (current.y - previous.y) * 0.5,
                                                    );
                                                    builder.curve_to(midpoint, previous);
                                                }
                                                if let Some(last) = points.last().copied() {
                                                    builder.line_to(last);
                                                }
                                            };

                                            let mut fill = PathBuilder::fill();
                                            fill.move_to(point(points[0].x, baseline));
                                            fill.line_to(points[0]);
                                            for pair in points.windows(2) {
                                                let previous = pair[0];
                                                let current = pair[1];
                                                let midpoint = point(
                                                    previous.x + (current.x - previous.x) * 0.5,
                                                    previous.y + (current.y - previous.y) * 0.5,
                                                );
                                                fill.curve_to(midpoint, previous);
                                            }
                                            if let Some(last) = points.last().copied() {
                                                fill.line_to(last);
                                                fill.line_to(point(last.x, baseline));
                                            }
                                            fill.close();
                                            if let Ok(path) = fill.build() {
                                                fills.push((path, color.opacity(0.055)));
                                            }

                                            let mut stroke = PathBuilder::stroke(px(1.5));
                                            append_curve(&mut stroke);
                                            if let Ok(path) = stroke.build() {
                                                strokes.push((path, color));
                                            }
                                        }
                                        paths.extend(fills);
                                        paths.extend(strokes);
                                        Some(paths)
                                    },
                                    move |_bounds, paths, window, _cx| {
                                        if let Some(paths) = paths {
                                            for (path, color) in paths {
                                                window.paint_path(path, color);
                                            }
                                        }
                                    },
                                )
                                .flex_1()
                                .h_full()),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h(px(62.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .h(px(30.))
                            .flex_none()
                            .px_2()
                            .justify_between()
                            .items_center()
                            .bg(cx.theme().muted)
                            .child(
                                div()
                                    .text_size(rems(0.76))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("disk").to_string()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.64))
                                    .text_color(muted_fg)
                                    .child(t!("available_size")),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_hidden()
                            .child(
                                v_flex()
                                    .id("sidebar-disk-scroll")
                                    .track_scroll(&self.disk_scroll_handle)
                                    .overflow_y_scroll()
                                    .h_full()
                                    .min_h(px(0.))
                                    .children(self.monitoring.system.filesystems.iter().enumerate().map(
                                        |(index, disk)| {
                                            let mount = disk.mount.clone();
                                            let capacity = format!(
                                                "{} / {}",
                                                format_bytes(disk.available_bytes),
                                                format_bytes(disk.total_bytes)
                                            );
                                            h_flex()
                                                .h(px(28.))
                                                .flex_none()
                                                .items_center()
                                                .px_2()
                                                    .border_b_1()
                                                    .border_color(cx.theme().border.opacity(0.35))
                                                    .when(index % 2 == 1, |this| {
                                                        this.bg(cx.theme().muted.opacity(0.22))
                                                    })
                                                    .child(
                                                        div()
                                                            .id(("sidebar-disk-mount", index))
                                                            .flex_1()
                                                            .min_w(px(0.))
                                                            .overflow_hidden()
                                                        .whitespace_nowrap()
                                                        .text_ellipsis()
                                                        .text_size(rems(0.68))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .tooltip({
                                                                let mount = mount.clone();
                                                                move |window, cx| {
                                                                    gpui_component::tooltip::Tooltip::new(
                                                                        mount.clone(),
                                                                    )
                                                                    .build(window, cx)
                                                                }
                                                            })
                                                            .child(mount),
                                                    )
                                                    .child(
                                                        div()
                                                            .id(("sidebar-disk-capacity", index))
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .text_right()
                                                        .pr_1()
                                                            .overflow_hidden()
                                                            .whitespace_nowrap()
                                                            .text_ellipsis()
                                                        .text_size(rems(0.66))
                                                            .text_color(muted_fg)
                                                            .tooltip({
                                                                let capacity = capacity.clone();
                                                                move |window, cx| {
                                                                    gpui_component::tooltip::Tooltip::new(
                                                                        capacity.clone(),
                                                                    )
                                                                    .build(window, cx)
                                                                }
                                                            })
                                                            .child(capacity),
                                                    )
                                        },
                                    )),
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
    }
}
