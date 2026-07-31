use super::*;

impl TinyShell {
    pub(super) fn render_sidebar_monitoring_panel(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let cpu_pct = self.system.cpu_percent.clamp(0.0, 1.0);
        let mem_pct = self.system.mem_percent.clamp(0.0, 1.0);
        let swap_pct = self.system.swap_percent.clamp(0.0, 1.0);
        let cpu_progress_pct = self.animated_cpu_percent.clamp(0.0, 1.0);
        let mem_progress_pct = self.animated_mem_percent.clamp(0.0, 1.0);
        let swap_progress_pct = self.animated_swap_percent.clamp(0.0, 1.0);
        let cpu_value_pct = self.system.cpu_percent.clamp(0.0, 1.0);
        let mem_value_pct = self.system.mem_percent.clamp(0.0, 1.0);
        let swap_value_pct = self.system.swap_percent.clamp(0.0, 1.0);
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
        let mem_value_detail =
            format!("{} · {:.0}%", self.system.mem_detail, mem_value_pct * 100.0);
        let swap_value_detail = format!(
            "{} · {:.0}%",
            self.system.swap_detail,
            swap_value_pct * 100.0
        );
        let net_color = Hsla::from(gpui::rgb(0x1586F5));
        let net_tx_color = cx.theme().danger;
        let net_grid_color = cx.theme().border.opacity(0.18);
        let muted_fg = cx.theme().muted_foreground;
        let selected_network_interface = self.selected_network_interface.clone();
        let selected_network = selected_network_interface.as_ref().and_then(|selected| {
            self.system
                .network_interfaces
                .iter()
                .find(|interface| &interface.name == selected)
        });
        let selected_network_label = selected_network_interface
            .clone()
            .unwrap_or_else(|| t!("total").to_string());
        let selected_rx_rate = selected_network
            .map(|interface| interface.receive_rate)
            .unwrap_or(self.system.net_rx_rate);
        let selected_tx_rate = selected_network
            .map(|interface| interface.transmit_rate)
            .unwrap_or(self.system.net_tx_rate);
        let selected_rx_values = selected_network_interface
            .as_ref()
            .and_then(|selected| self.network_interface_histories.get(selected))
            .map(|history| history.receive.iter().copied().collect::<Vec<_>>())
            .unwrap_or_else(|| self.net_rx_history.iter().copied().collect());
        let selected_tx_values = selected_network_interface
            .as_ref()
            .and_then(|selected| self.network_interface_histories.get(selected))
            .map(|history| history.transmit.iter().copied().collect::<Vec<_>>())
            .unwrap_or_else(|| self.net_tx_history.iter().copied().collect());
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
            .system
            .network_interfaces
            .iter()
            .map(|interface| interface.name.clone())
            .collect::<Vec<_>>();
        let process_view = self.process_view;
        let process_view_epoch = process_view as u64;
        let mut displayed_processes = self.system.processes.clone();
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
                                        this.process_view = ProcessView::Memory;
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
                                        this.process_view = ProcessView::Cpu;
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
                                        this.process_view = ProcessView::Activity;
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
                                                view.read(cx).selected_network_interface.clone();
                                            menu = menu.item(
                                                PopupMenuItem::new(t!("total").to_string())
                                                    .checked(selected.is_none())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.selected_network_interface = None;
                                                            cx.notify();
                                                        },
                                                    )),
                                            );
                                            for name in network_interface_names.clone() {
                                                let selected_name = name.clone();
                                                menu = menu.item(
                                                    PopupMenuItem::new(name.clone())
                                                        .checked(selected.as_ref() == Some(&name))
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            move |this, _, _, cx| {
                                                                this.selected_network_interface =
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
                                    .children(self.system.filesystems.iter().enumerate().map(
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

    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_update = matches!(
            self.update_runtime.status,
            Some(crate::app::updater::UpdateStatus::UpdateAvailable(_))
                | Some(crate::app::updater::UpdateStatus::DownloadCancelled(_))
                | Some(crate::app::updater::UpdateStatus::DownloadFailed(_, _))
        );
        let active_tab = self
            .active_tab
            .as_ref()
            .and_then(|active_id| self.tabs.iter().find(|tab| &tab.id == active_id));
        let active_session = active_tab.and_then(|tab| tab.session.as_ref());
        let host_text = active_session
            .map(|session| session.host.clone())
            .unwrap_or_else(|| t!("local_host").to_string());
        let connection_text = active_session
            .map(|session| format!("{}@{}:{}", session.user, session.host, session.port))
            .unwrap_or_else(|| t!("local_terminal").to_string());
        let mut ip_address_entries = self.system.ip_address_entries.clone();
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
                                    .child(format_uptime(self.system.uptime_seconds)),
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
                    || self.config.monitoring_position() == "Sidebar",
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
