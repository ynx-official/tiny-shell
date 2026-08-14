use super::*;
use crate::app::tool_panel::DockerContainerFilter;
use crate::docker::{
    DockerAction, DockerContainer, DockerContainerState, DockerImage, DockerPage,
    DockerRestartPolicy,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DockerContainerGroup {
    Running,
    Stopped,
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DockerContainerSummary {
    total: usize,
    running: usize,
    stopped: usize,
    other: usize,
}

fn docker_container_group(state: &DockerContainerState) -> DockerContainerGroup {
    match state {
        DockerContainerState::Running => DockerContainerGroup::Running,
        DockerContainerState::Created | DockerContainerState::Exited => {
            DockerContainerGroup::Stopped
        }
        DockerContainerState::Paused
        | DockerContainerState::Restarting
        | DockerContainerState::Removing
        | DockerContainerState::Dead
        | DockerContainerState::Unknown(_) => DockerContainerGroup::Other,
    }
}

fn docker_container_summary(containers: &[DockerContainer]) -> DockerContainerSummary {
    let mut summary = DockerContainerSummary {
        total: containers.len(),
        ..DockerContainerSummary::default()
    };
    for container in containers {
        match docker_container_group(&container.state) {
            DockerContainerGroup::Running => summary.running += 1,
            DockerContainerGroup::Stopped => summary.stopped += 1,
            DockerContainerGroup::Other => summary.other += 1,
        }
    }
    summary
}

fn docker_container_matches(container: &DockerContainer, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || [
            container.names.as_str(),
            container.image.as_str(),
            container.status.as_str(),
            container.ports.as_str(),
            container.id.as_str(),
        ]
        .iter()
        .any(|value| value.to_lowercase().contains(&query))
}

fn docker_filter_matches(state: &DockerContainerState, filter: DockerContainerFilter) -> bool {
    match filter {
        DockerContainerFilter::All => true,
        DockerContainerFilter::Running => *state == DockerContainerState::Running,
        DockerContainerFilter::Stopped => matches!(
            state,
            DockerContainerState::Created | DockerContainerState::Exited
        ),
    }
}

fn docker_remove_action(state: &DockerContainerState) -> DockerAction {
    if matches!(
        state,
        DockerContainerState::Running
            | DockerContainerState::Paused
            | DockerContainerState::Restarting
    ) {
        DockerAction::ForceRemove
    } else {
        DockerAction::Remove
    }
}

fn docker_autostart_action(policy: &DockerRestartPolicy) -> DockerAction {
    if policy.autostart_enabled() {
        DockerAction::DisableAutostart
    } else {
        DockerAction::EnableAutostart
    }
}

fn docker_image_matches(image: &DockerImage, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || [
            image.repository.as_str(),
            image.tag.as_str(),
            image.created_since.as_str(),
            image.size.as_str(),
            image.id.as_str(),
        ]
        .iter()
        .any(|value| value.to_lowercase().contains(&query))
}

impl TinyShell {
    // Keep panel composition outside TinyShell::render and erase both render
    // boundaries. On Windows, carrying the large concrete GPUI element type in
    // the already deep SSH workspace render frame can exhaust the UI stack.
    pub(super) fn render_workspace_with_tool_panel(
        &self,
        workspace: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match crate::app::tool_panel::tool_panel_layout(
            self.tool_panel.open,
            self.tool_panel.presentation,
        ) {
            crate::app::tool_panel::ToolPanelLayout::Closed => workspace,
            crate::app::tool_panel::ToolPanelLayout::Extended => h_flex()
                .size_full()
                .min_w(px(0.))
                .child(div().flex_1().min_w(px(0.)).h_full().child(workspace))
                .child(self.render_tool_panel(cx))
                .into_any_element(),
            crate::app::tool_panel::ToolPanelLayout::Overlay => div()
                .size_full()
                .relative()
                .overflow_hidden()
                .child(workspace)
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .right(px(0.))
                        .bottom(px(0.))
                        .shadow_lg()
                        .child(self.render_tool_panel(cx)),
                )
                .into_any_element(),
        }
    }

    pub(super) fn render_tool_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let pending = self.tool_panel.pending.is_some();
        let target_available = self.tool_panel.target_tab_id.is_some();
        let connected = self.tool_panel.target_connected;
        let search_query = self.docker_search_input.read(cx).value().trim().to_string();
        let view = cx.entity();

        let page_content = if !target_available {
            self.render_docker_empty_state(
                IconName::SquareTerminal,
                t!("docker_no_active_session").to_string(),
                cx,
            )
            .into_any_element()
        } else if !connected {
            self.render_docker_empty_state(
                IconName::Network,
                t!("docker_session_disconnected").to_string(),
                cx,
            )
            .into_any_element()
        } else if pending
            && match self.tool_panel.page {
                DockerPage::Containers => self.tool_panel.containers.is_empty(),
                DockerPage::Images => self.tool_panel.images.is_empty(),
            }
        {
            self.render_docker_empty_state(
                IconName::ArrowRight,
                t!("docker_loading").to_string(),
                cx,
            )
            .into_any_element()
        } else {
            match self.tool_panel.page {
                DockerPage::Containers => self
                    .render_docker_containers(&search_query, cx)
                    .into_any_element(),
                DockerPage::Images => self
                    .render_docker_images(&search_query, cx)
                    .into_any_element(),
            }
        };

        let summary = docker_container_summary(&self.tool_panel.containers);
        let target_label = if target_available {
            self.tool_panel.target_label.clone()
        } else {
            t!("docker_target_none_short").to_string()
        };
        let target_status = if connected {
            t!("docker_connected").to_string()
        } else {
            t!("docker_disconnected").to_string()
        };
        let target_detail = if self.tool_panel.target_detail.is_empty() {
            target_status
        } else {
            format!("{} · {}", self.tool_panel.target_detail, target_status)
        };
        let container_filter = self.tool_panel.container_filter;

        v_flex()
            .id("tool-panel")
            .flex_none()
            .w(px(crate::app::tool_panel::TOOL_PANEL_WIDTH))
            .h_full()
            .min_h(px(0.))
            .overflow_hidden()
            .border_l_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .flex_none()
                    .px_3()
                    .pt_3()
                    .gap_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .h(px(28.))
                            .justify_between()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Docker"),
                            )
                            .child(
                                Button::new("tool-panel-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .tooltip(t!("tool_panel_close").to_string())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_tool_panel(window, cx)
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("docker_current_host").to_string()),
                            )
                            .child(
                                h_flex()
                                    .h(px(50.))
                                    .gap_2()
                                    .child(div().size(px(9.)).rounded(px(999.)).bg(if connected {
                                        cx.theme().success
                                    } else {
                                        cx.theme().muted_foreground
                                    }))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .gap(px(2.))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_sm()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(target_label),
                                            )
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(target_detail),
                                            ),
                                    )
                                    .child(
                                        Button::new("docker-refresh")
                                            .small()
                                            .icon(IconName::Redo)
                                            .tooltip(t!("refresh").to_string())
                                            .disabled(!target_available || !connected || pending)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.request_current_docker_page(cx)
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(36.))
                            .border_y_1()
                            .border_color(cx.theme().border)
                            .text_xs()
                            .child(
                                div()
                                    .id("docker-filter-all-summary")
                                    .h_full()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(container_filter == DockerContainerFilter::All, |this| {
                                        this.text_color(cx.theme().foreground)
                                            .border_b_2()
                                            .border_color(cx.theme().foreground)
                                    })
                                    .when(container_filter != DockerContainerFilter::All, |this| {
                                        this.text_color(cx.theme().muted_foreground)
                                    })
                                    .child(
                                        t!("docker_summary_total", count = summary.total)
                                            .to_string(),
                                    )
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _, cx| {
                                            view.update(cx, |this, cx| {
                                                this.set_docker_container_filter(
                                                    DockerContainerFilter::All,
                                                    cx,
                                                );
                                                this.set_docker_page(DockerPage::Containers, cx);
                                            });
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .h(px(16.))
                                    .border_l_1()
                                    .border_color(cx.theme().border),
                            )
                            .child(
                                div()
                                    .id("docker-filter-running-summary")
                                    .h_full()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(
                                        container_filter == DockerContainerFilter::Running,
                                        |this| {
                                            this.text_color(cx.theme().success)
                                                .border_b_2()
                                                .border_color(cx.theme().success)
                                        },
                                    )
                                    .when(
                                        container_filter != DockerContainerFilter::Running,
                                        |this| this.text_color(cx.theme().muted_foreground),
                                    )
                                    .child(
                                        t!("docker_summary_running", count = summary.running)
                                            .to_string(),
                                    )
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _, cx| {
                                            view.update(cx, |this, cx| {
                                                this.set_docker_container_filter(
                                                    DockerContainerFilter::Running,
                                                    cx,
                                                );
                                                this.set_docker_page(DockerPage::Containers, cx);
                                            });
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .h(px(16.))
                                    .border_l_1()
                                    .border_color(cx.theme().border),
                            )
                            .child(
                                div()
                                    .id("docker-filter-stopped-summary")
                                    .h_full()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(
                                        container_filter == DockerContainerFilter::Stopped,
                                        |this| {
                                            this.text_color(cx.theme().foreground)
                                                .border_b_2()
                                                .border_color(cx.theme().foreground)
                                        },
                                    )
                                    .when(
                                        container_filter != DockerContainerFilter::Stopped,
                                        |this| this.text_color(cx.theme().muted_foreground),
                                    )
                                    .child(
                                        t!("docker_summary_stopped", count = summary.stopped)
                                            .to_string(),
                                    )
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _, cx| {
                                            view.update(cx, |this, cx| {
                                                this.set_docker_container_filter(
                                                    DockerContainerFilter::Stopped,
                                                    cx,
                                                );
                                                this.set_docker_page(DockerPage::Containers, cx);
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(44.))
                            .gap_2()
                            .child(
                                div().flex_1().h_full().child(
                                    Input::new(&self.docker_search_input)
                                        .large()
                                        .prefix(Icon::new(IconName::Search).small()),
                                ),
                            )
                            .child(
                                Button::new("docker-filter-menu")
                                    .large()
                                    .icon(IconName::SortAscending)
                                    .tooltip(t!("docker_filter").to_string())
                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                        let view = view.clone();
                                        move |menu, window, _| {
                                            menu.item(
                                                PopupMenuItem::new(
                                                    t!("docker_filter_all").to_string(),
                                                )
                                                .checked(
                                                    container_filter == DockerContainerFilter::All,
                                                )
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.set_docker_container_filter(
                                                            DockerContainerFilter::All,
                                                            cx,
                                                        );
                                                        this.set_docker_page(
                                                            DockerPage::Containers,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                            )
                                            .item(
                                                PopupMenuItem::new(
                                                    t!("docker_filter_running").to_string(),
                                                )
                                                .checked(
                                                    container_filter
                                                        == DockerContainerFilter::Running,
                                                )
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.set_docker_container_filter(
                                                            DockerContainerFilter::Running,
                                                            cx,
                                                        );
                                                        this.set_docker_page(
                                                            DockerPage::Containers,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                            )
                                            .item(
                                                PopupMenuItem::new(
                                                    t!("docker_filter_stopped").to_string(),
                                                )
                                                .checked(
                                                    container_filter
                                                        == DockerContainerFilter::Stopped,
                                                )
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.set_docker_container_filter(
                                                            DockerContainerFilter::Stopped,
                                                            cx,
                                                        );
                                                        this.set_docker_page(
                                                            DockerPage::Containers,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                            )
                                        }
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(40.))
                            .gap_4()
                            .child(
                                h_flex()
                                    .id("docker-page-containers")
                                    .h_full()
                                    .gap_2()
                                    .px_1()
                                    .cursor_pointer()
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(self.tool_panel.page == DockerPage::Containers, |this| {
                                        this.border_b_2().border_color(cx.theme().foreground)
                                    })
                                    .child(t!("docker_containers").to_string())
                                    .child(
                                        div()
                                            .px_2()
                                            .py(px(1.))
                                            .rounded(px(999.))
                                            .bg(cx.theme().muted)
                                            .text_xs()
                                            .child(summary.total.to_string()),
                                    )
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _, cx| {
                                            view.update(cx, |this, cx| {
                                                this.set_docker_page(DockerPage::Containers, cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                h_flex()
                                    .id("docker-page-images")
                                    .h_full()
                                    .px_1()
                                    .cursor_pointer()
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(self.tool_panel.page == DockerPage::Images, |this| {
                                        this.border_b_2().border_color(cx.theme().foreground)
                                    })
                                    .child(t!("docker_images").to_string())
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _, cx| {
                                            view.update(cx, |this, cx| {
                                                this.set_docker_page(DockerPage::Images, cx)
                                            });
                                        }
                                    }),
                            ),
                    ),
            )
            .when_some(self.tool_panel.error.clone(), |this, error| {
                this.child(
                    div()
                        .flex_none()
                        .mx_3()
                        .mt_2()
                        .mb_2()
                        .px_3()
                        .py_2()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(cx.theme().danger.opacity(0.35))
                        .bg(cx.theme().danger.opacity(0.08))
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(page_content),
            )
            .into_any_element()
    }

    fn render_docker_empty_state(
        &self,
        icon: IconName,
        message: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .px_5()
            .text_center()
            .text_color(cx.theme().muted_foreground)
            .child(Icon::new(icon).with_size(Size::Large))
            .child(div().text_sm().child(message))
    }

    fn render_docker_containers(&self, query: &str, cx: &mut Context<Self>) -> impl IntoElement {
        if self.tool_panel.containers.is_empty() {
            return self
                .render_docker_empty_state(
                    IconName::SquareTerminal,
                    t!("docker_containers_empty").to_string(),
                    cx,
                )
                .into_any_element();
        }
        let pending_container = self.tool_panel.pending.as_ref().and_then(|pending| {
            if let crate::docker::DockerOperation::ContainerAction { container_id, .. } =
                &pending.operation
            {
                Some(container_id.as_str())
            } else {
                None
            }
        });
        let filtered = self
            .tool_panel
            .containers
            .iter()
            .enumerate()
            .filter(|(_, container)| {
                docker_container_matches(container, query)
                    && docker_filter_matches(&container.state, self.tool_panel.container_filter)
            })
            .collect::<Vec<_>>();
        if filtered.is_empty() {
            return self
                .render_docker_empty_state(
                    IconName::SquareTerminal,
                    if query.trim().is_empty() {
                        t!("docker_filter_empty").to_string()
                    } else {
                        t!("docker_search_empty").to_string()
                    },
                    cx,
                )
                .into_any_element();
        }
        let running = filtered
            .iter()
            .copied()
            .filter(|(_, container)| {
                docker_container_group(&container.state) == DockerContainerGroup::Running
            })
            .collect::<Vec<_>>();
        let stopped = filtered
            .iter()
            .copied()
            .filter(|(_, container)| {
                docker_container_group(&container.state) == DockerContainerGroup::Stopped
            })
            .collect::<Vec<_>>();
        let other = filtered
            .into_iter()
            .filter(|(_, container)| {
                docker_container_group(&container.state) == DockerContainerGroup::Other
            })
            .collect::<Vec<_>>();
        let running_count = running.len();
        let stopped_count = stopped.len();
        let other_count = other.len();

        v_flex()
            .id("docker-containers-scroll")
            .size_full()
            .overflow_y_scroll()
            .px_3()
            .py_3()
            .gap_4()
            .when(!running.is_empty(), |this| {
                this.child(self.render_docker_container_group(
                    t!("docker_group_running", count = running_count).to_string(),
                    running,
                    pending_container,
                    cx,
                ))
            })
            .when(!stopped.is_empty(), |this| {
                this.child(self.render_docker_container_group(
                    t!("docker_group_stopped", count = stopped_count).to_string(),
                    stopped,
                    pending_container,
                    cx,
                ))
            })
            .when(!other.is_empty(), |this| {
                this.child(self.render_docker_container_group(
                    t!("docker_group_other", count = other_count).to_string(),
                    other,
                    pending_container,
                    cx,
                ))
            })
            .into_any_element()
    }

    fn render_docker_container_group(
        &self,
        title: String,
        containers: Vec<(usize, &DockerContainer)>,
        pending_container: Option<&str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let container_count = containers.len();
        let view = cx.entity();
        v_flex()
            .gap_2()
            .child(
                div()
                    .px_1()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .child(
                v_flex()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().muted.opacity(0.1))
                    .overflow_hidden()
                    .children(containers.into_iter().enumerate().map(
                        |(row_index, (index, container))| {
                            let actions = container.state.actions();
                            let is_pending = pending_container == Some(container.id.as_str());
                            let is_running = container.state == DockerContainerState::Running;
                            let state_label = docker_state_label(&container.state);
                            let (direct_action, direct_label) = if actions.start {
                                (Some(DockerAction::Start), t!("docker_start").to_string())
                            } else if actions.stop {
                                (Some(DockerAction::Stop), t!("docker_stop").to_string())
                            } else {
                                (None, state_label.clone())
                            };
                            let direct_container = container.clone();
                            let menu_container = container.clone();
                            let menu_view = view.clone();
                            let autostart_action =
                                docker_autostart_action(&container.restart_policy);
                            let autostart_label =
                                if autostart_action == DockerAction::DisableAutostart {
                                    t!("docker_disable_autostart").to_string()
                                } else {
                                    t!("docker_enable_autostart").to_string()
                                };
                            let remove_action = docker_remove_action(&container.state);
                            v_flex()
                                .id(("docker-container", index))
                                .gap_1()
                                .px_3()
                                .py_2()
                                .when(row_index + 1 < container_count, |this| {
                                    this.border_b_1().border_color(cx.theme().border)
                                })
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(div().size(px(7.)).rounded(px(999.)).bg(
                                            if is_running {
                                                cx.theme().success
                                            } else {
                                                cx.theme().muted_foreground
                                            },
                                        ))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .truncate()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(container.names.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(if is_running {
                                                    cx.theme().success
                                                } else {
                                                    cx.theme().muted_foreground
                                                })
                                                .child(state_label.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .pl(px(15.))
                                        .text_sm()
                                        .truncate()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(container.image.clone()),
                                )
                                .child(
                                    h_flex()
                                        .pl(px(15.))
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .text_xs()
                                                .truncate()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(if container.ports.is_empty() {
                                                    container.status.clone()
                                                } else {
                                                    format!(
                                                        "{} · {}",
                                                        container.status, container.ports
                                                    )
                                                }),
                                        )
                                        .when_some(direct_action, |this, action| {
                                            this.child(
                                                Button::new(("docker-direct-action", index))
                                                    .small()
                                                    .label(if is_pending {
                                                        t!("docker_action_running").to_string()
                                                    } else {
                                                        direct_label.clone()
                                                    })
                                                    .when(action == DockerAction::Start, |button| {
                                                        button.primary()
                                                    })
                                                    .when(action == DockerAction::Stop, |button| {
                                                        button.danger()
                                                    })
                                                    .disabled(is_pending)
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.confirm_docker_action(
                                                                direct_container.clone(),
                                                                action,
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                        })
                                        .when(direct_action.is_none(), |this| {
                                            this.child(
                                                Button::new(("docker-direct-action", index))
                                                    .small()
                                                    .label(if is_pending {
                                                        t!("docker_action_running").to_string()
                                                    } else {
                                                        direct_label.clone()
                                                    })
                                                    .disabled(true),
                                            )
                                        })
                                        .child(
                                            Button::new(("docker-more-actions", index))
                                                .small()
                                                .icon(IconName::Ellipsis)
                                                .tooltip(t!("docker_more_actions").to_string())
                                                .disabled(is_pending)
                                                .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                                    move |menu, window, _| {
                                                        let restart_container =
                                                            menu_container.clone();
                                                        let autostart_container =
                                                            menu_container.clone();
                                                        let remove_container =
                                                            menu_container.clone();
                                                        menu.item(
                                                            PopupMenuItem::new(
                                                                t!("docker_restart").to_string(),
                                                            )
                                                            .on_click(window.listener_for(
                                                                &menu_view,
                                                                move |this, _, window, cx| {
                                                                    this.confirm_docker_action(
                                                                        restart_container.clone(),
                                                                        DockerAction::Restart,
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
                                                        )
                                                        .item(
                                                            PopupMenuItem::new(
                                                                autostart_label.clone(),
                                                            )
                                                            .on_click(window.listener_for(
                                                                &menu_view,
                                                                move |this, _, window, cx| {
                                                                    this.confirm_docker_action(
                                                                        autostart_container.clone(),
                                                                        autostart_action,
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
                                                        )
                                                        .separator()
                                                        .item(
                                                            PopupMenuItem::new(
                                                                t!("docker_remove").to_string(),
                                                            )
                                                            .on_click(window.listener_for(
                                                                &menu_view,
                                                                move |this, _, window, cx| {
                                                                    this.confirm_docker_action(
                                                                        remove_container.clone(),
                                                                        remove_action,
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
                                                        )
                                                    }
                                                }),
                                        ),
                                )
                                .into_any_element()
                        },
                    )),
            )
            .into_any_element()
    }

    fn render_docker_images(&self, query: &str, cx: &mut Context<Self>) -> impl IntoElement {
        if self.tool_panel.images.is_empty() {
            return self
                .render_docker_empty_state(
                    IconName::SquareTerminal,
                    t!("docker_images_empty").to_string(),
                    cx,
                )
                .into_any_element();
        }
        let images = self
            .tool_panel
            .images
            .iter()
            .enumerate()
            .filter(|(_, image)| docker_image_matches(image, query))
            .collect::<Vec<_>>();
        if images.is_empty() {
            return self
                .render_docker_empty_state(
                    IconName::SquareTerminal,
                    t!("docker_search_empty").to_string(),
                    cx,
                )
                .into_any_element();
        }
        let image_count = images.len();
        v_flex()
            .id("docker-images-scroll")
            .size_full()
            .overflow_y_scroll()
            .px_3()
            .py_3()
            .child(
                v_flex()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().muted.opacity(0.1))
                    .overflow_hidden()
                    .children(
                        images
                            .into_iter()
                            .enumerate()
                            .map(|(row_index, (index, image))| {
                                let repository = if image.repository == "<none>" {
                                    t!("docker_untagged").to_string()
                                } else {
                                    image.repository.clone()
                                };
                                v_flex()
                                    .id(("docker-image", index))
                                    .gap_1()
                                    .px_3()
                                    .py_2()
                                    .when(row_index + 1 < image_count, |this| {
                                        this.border_b_1().border_color(cx.theme().border)
                                    })
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .truncate()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(format!("{}:{}", repository, image.tag)),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(image.size.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .truncate()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "{} · {}",
                                                image.created_since, image.id
                                            )),
                                    )
                                    .into_any_element()
                            }),
                    ),
            )
            .into_any_element()
    }
}

fn docker_state_label(state: &DockerContainerState) -> String {
    match state {
        DockerContainerState::Created => t!("docker_state_created").to_string(),
        DockerContainerState::Running => t!("docker_state_running").to_string(),
        DockerContainerState::Paused => t!("docker_state_paused").to_string(),
        DockerContainerState::Restarting => t!("docker_state_restarting").to_string(),
        DockerContainerState::Removing => t!("docker_state_removing").to_string(),
        DockerContainerState::Exited => t!("docker_state_exited").to_string(),
        DockerContainerState::Dead => t!("docker_state_dead").to_string(),
        DockerContainerState::Unknown(state) => state.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DockerContainerGroup, docker_autostart_action, docker_container_group,
        docker_container_matches, docker_container_summary, docker_filter_matches,
        docker_image_matches, docker_remove_action,
    };
    use crate::app::tool_panel::DockerContainerFilter;
    use crate::docker::{
        DockerAction, DockerContainer, DockerContainerState, DockerImage, DockerRestartPolicy,
    };

    fn container(name: &str, image: &str, state: DockerContainerState) -> DockerContainer {
        DockerContainer {
            id: "0123456789abcdef".into(),
            names: name.into(),
            image: image.into(),
            state,
            status: "Up 2 hours".into(),
            ports: "127.0.0.1:8080->80/tcp".into(),
            restart_policy: DockerRestartPolicy::No,
        }
    }

    #[test]
    fn docker_summary_and_groups_preserve_every_container_state() {
        let containers = vec![
            container("web", "nginx:latest", DockerContainerState::Running),
            container("db", "mysql:8", DockerContainerState::Exited),
            container("job", "worker:1", DockerContainerState::Created),
            container("cache", "redis:7", DockerContainerState::Paused),
        ];

        let summary = docker_container_summary(&containers);
        assert_eq!(summary.total, 4);
        assert_eq!(summary.running, 1);
        assert_eq!(summary.stopped, 2);
        assert_eq!(summary.other, 1);
        assert_eq!(
            docker_container_group(&containers[0].state),
            DockerContainerGroup::Running
        );
        assert_eq!(
            docker_container_group(&containers[1].state),
            DockerContainerGroup::Stopped
        );
        assert_eq!(
            docker_container_group(&containers[3].state),
            DockerContainerGroup::Other
        );
    }

    #[test]
    fn docker_search_matches_name_image_status_ports_and_id_case_insensitively() {
        let container = container(
            "Tiny-Blog",
            "registry.example/app:1",
            DockerContainerState::Running,
        );

        for query in ["tiny", "REGISTRY", "2 HOURS", "8080", "012345"] {
            assert!(docker_container_matches(&container, query));
        }
        assert!(docker_container_matches(&container, ""));
        assert!(!docker_container_matches(&container, "postgres"));

        let image = DockerImage {
            id: "sha256:abcdef".into(),
            repository: "registry.example/worker".into(),
            tag: "v2".into(),
            created_since: "3 days ago".into(),
            size: "128MB".into(),
        };
        assert!(docker_image_matches(&image, "WORKER"));
        assert!(docker_image_matches(&image, "128mb"));
        assert!(!docker_image_matches(&image, "nginx"));
    }

    #[test]
    fn docker_container_filter_distinguishes_running_and_stopped_states() {
        assert!(docker_filter_matches(
            &DockerContainerState::Paused,
            DockerContainerFilter::All
        ));
        assert!(docker_filter_matches(
            &DockerContainerState::Running,
            DockerContainerFilter::Running
        ));
        assert!(!docker_filter_matches(
            &DockerContainerState::Exited,
            DockerContainerFilter::Running
        ));
        assert!(docker_filter_matches(
            &DockerContainerState::Exited,
            DockerContainerFilter::Stopped
        ));
        assert!(docker_filter_matches(
            &DockerContainerState::Created,
            DockerContainerFilter::Stopped
        ));
        assert!(!docker_filter_matches(
            &DockerContainerState::Paused,
            DockerContainerFilter::Stopped
        ));
    }

    #[test]
    fn docker_menu_selects_one_autostart_action_and_forces_running_removal() {
        assert_eq!(
            docker_autostart_action(&DockerRestartPolicy::UnlessStopped),
            DockerAction::DisableAutostart
        );
        assert_eq!(
            docker_autostart_action(&DockerRestartPolicy::Always),
            DockerAction::DisableAutostart
        );
        assert_eq!(
            docker_autostart_action(&DockerRestartPolicy::No),
            DockerAction::EnableAutostart
        );
        assert_eq!(
            docker_autostart_action(&DockerRestartPolicy::OnFailure),
            DockerAction::EnableAutostart
        );
        assert_eq!(
            docker_remove_action(&DockerContainerState::Running),
            DockerAction::ForceRemove
        );
        assert_eq!(
            docker_remove_action(&DockerContainerState::Paused),
            DockerAction::ForceRemove
        );
        assert_eq!(
            docker_remove_action(&DockerContainerState::Exited),
            DockerAction::Remove
        );
    }
}
