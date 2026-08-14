use super::*;
use crate::docker::{DockerAction, DockerContainerState, DockerPage};

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
                DockerPage::Containers => self.render_docker_containers(cx).into_any_element(),
                DockerPage::Images => self.render_docker_images(cx).into_any_element(),
            }
        };

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
                h_flex()
                    .flex_none()
                    .h(px(42.))
                    .px_3()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("tool_panel").to_string()),
                    )
                    .child(
                        Button::new("tool-panel-close")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip(t!("tool_panel_close").to_string())
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.close_tool_panel(window, cx)
                                }),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .h(px(38.))
                    .px_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .h_full()
                            .flex()
                            .items_center()
                            .border_b_2()
                            .border_color(cx.theme().primary)
                            .font_weight(FontWeight::MEDIUM)
                            .child("Docker"),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .min_h(px(44.))
                    .px_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_sm()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(if target_available {
                                t!(
                                    "docker_target",
                                    target = self.tool_panel.target_label.clone()
                                )
                                .to_string()
                            } else {
                                t!("docker_target_none").to_string()
                            }),
                    )
                    .child(
                        Button::new("docker-refresh")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ArrowRight)
                            .tooltip(t!("refresh").to_string())
                            .disabled(!target_available || !connected || pending)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.request_current_docker_page(cx)),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .child(
                        Button::new("docker-page-containers")
                            .small()
                            .label(t!("docker_containers").to_string())
                            .when(self.tool_panel.page == DockerPage::Containers, |button| {
                                button.primary()
                            })
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
                        Button::new("docker-page-images")
                            .small()
                            .label(t!("docker_images").to_string())
                            .when(self.tool_panel.page == DockerPage::Images, |button| {
                                button.primary()
                            })
                            .on_click(move |_, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.set_docker_page(DockerPage::Images, cx)
                                });
                            }),
                    ),
            )
            .when_some(self.tool_panel.error.clone(), |this, error| {
                this.child(
                    div()
                        .flex_none()
                        .mx_3()
                        .mb_2()
                        .px_3()
                        .py_2()
                        .rounded(px(6.))
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

    fn render_docker_containers(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

        v_flex()
            .id("docker-containers-scroll")
            .size_full()
            .overflow_y_scroll()
            .px_3()
            .pb_3()
            .children(
                self.tool_panel
                    .containers
                    .iter()
                    .enumerate()
                    .map(|(index, container)| {
                        let actions = container.state.actions();
                        let is_pending = pending_container == Some(container.id.as_str());
                        let state_label = docker_state_label(&container.state);
                        let start_container = container.clone();
                        let stop_container = container.clone();
                        let restart_container = container.clone();
                        v_flex()
                            .id(("docker-container", index))
                            .gap_1()
                            .py_3()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().size(px(7.)).rounded(px(999.)).bg(
                                        if container.state == DockerContainerState::Running {
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
                                            .text_color(cx.theme().muted_foreground)
                                            .child(state_label),
                                    ),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(container.image.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if container.ports.is_empty() {
                                        container.status.clone()
                                    } else {
                                        format!("{} · {}", container.status, container.ports)
                                    }),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .mt_1()
                                    .when(actions.start, |this| {
                                        this.child(
                                            Button::new(("docker-start", index))
                                                .small()
                                                .primary()
                                                .label(t!("docker_start").to_string())
                                                .disabled(is_pending)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.confirm_docker_action(
                                                            start_container.clone(),
                                                            DockerAction::Start,
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        )
                                    })
                                    .when(actions.stop, |this| {
                                        this.child(
                                            Button::new(("docker-stop", index))
                                                .small()
                                                .danger()
                                                .label(t!("docker_stop").to_string())
                                                .disabled(is_pending)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.confirm_docker_action(
                                                            stop_container.clone(),
                                                            DockerAction::Stop,
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        )
                                    })
                                    .when(actions.restart, |this| {
                                        this.child(
                                            Button::new(("docker-restart", index))
                                                .small()
                                                .label(t!("docker_restart").to_string())
                                                .disabled(is_pending)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.confirm_docker_action(
                                                            restart_container.clone(),
                                                            DockerAction::Restart,
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        )
                                    })
                                    .when(is_pending, |this| {
                                        this.child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(t!("docker_action_running").to_string()),
                                        )
                                    }),
                            )
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    fn render_docker_images(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.tool_panel.images.is_empty() {
            return self
                .render_docker_empty_state(
                    IconName::SquareTerminal,
                    t!("docker_images_empty").to_string(),
                    cx,
                )
                .into_any_element();
        }
        v_flex()
            .id("docker-images-scroll")
            .size_full()
            .overflow_y_scroll()
            .px_3()
            .pb_3()
            .children(
                self.tool_panel
                    .images
                    .iter()
                    .enumerate()
                    .map(|(index, image)| {
                        let repository = if image.repository == "<none>" {
                            t!("docker_untagged").to_string()
                        } else {
                            image.repository.clone()
                        };
                        v_flex()
                            .id(("docker-image", index))
                            .gap_1()
                            .py_3()
                            .border_b_1()
                            .border_color(cx.theme().border)
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
                                    .child(format!("{} · {}", image.created_since, image.id)),
                            )
                            .into_any_element()
                    }),
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
