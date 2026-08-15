use gpui::{Bounds, Context, Pixels, Size, Window, px, size};
use gpui_component::{WindowExt as _, button::ButtonVariant, dialog::DialogButtonProps};
use rust_i18n::t;

use crate::{
    TinyShell,
    docker::{
        DockerAction, DockerContainer, DockerImage, DockerOperation, DockerPage, DockerPayload,
        DockerRequest, DockerResponse,
    },
    terminal::{BackendCommand, TabKind},
};

pub(crate) const TOOL_PANEL_WIDTH: f32 = 360.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ToolPanelPresentation {
    Extended,
    #[default]
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolPanelLayout {
    Closed,
    Extended,
    Overlay,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DockerContainerFilter {
    #[default]
    All,
    Running,
    Stopped,
}

pub(crate) fn tool_panel_layout(
    open: bool,
    presentation: ToolPanelPresentation,
) -> ToolPanelLayout {
    if !open {
        ToolPanelLayout::Closed
    } else {
        match presentation {
            ToolPanelPresentation::Extended => ToolPanelLayout::Extended,
            ToolPanelPresentation::Overlay => ToolPanelLayout::Overlay,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PendingDockerRequest {
    pub(crate) id: u64,
    pub(crate) tab_id: String,
    pub(crate) operation: DockerOperation,
}

#[derive(Debug)]
pub(crate) struct ToolPanelState {
    pub(crate) open: bool,
    pub(crate) presentation: ToolPanelPresentation,
    pub(crate) base_window_bounds: Option<Bounds<Pixels>>,
    pub(crate) base_viewport_size: Option<Size<Pixels>>,
    pub(crate) page: DockerPage,
    pub(crate) container_filter: DockerContainerFilter,
    pub(crate) target_tab_id: Option<String>,
    pub(crate) target_generation: Option<u64>,
    pub(crate) target_label: String,
    pub(crate) target_detail: String,
    pub(crate) target_connected: bool,
    pub(crate) containers: Vec<DockerContainer>,
    pub(crate) images: Vec<DockerImage>,
    pub(crate) error: Option<String>,
    pub(crate) pending: Option<PendingDockerRequest>,
    next_request_id: u64,
}

impl Default for ToolPanelState {
    fn default() -> Self {
        Self {
            open: false,
            presentation: ToolPanelPresentation::Overlay,
            base_window_bounds: None,
            base_viewport_size: None,
            page: DockerPage::Containers,
            container_filter: DockerContainerFilter::All,
            target_tab_id: None,
            target_generation: None,
            target_label: String::new(),
            target_detail: String::new(),
            target_connected: false,
            containers: Vec::new(),
            images: Vec::new(),
            error: None,
            pending: None,
            next_request_id: 1,
        }
    }
}

impl ToolPanelState {
    fn next_request_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        id
    }

    pub(crate) fn persisted_window_bounds(
        &self,
        current: gpui::WindowBounds,
    ) -> gpui::WindowBounds {
        if self.open
            && self.presentation == ToolPanelPresentation::Extended
            && let Some(bounds) = self.base_window_bounds
        {
            return gpui::WindowBounds::Windowed(bounds);
        }
        current
    }
}

pub(crate) fn should_extend_panel(
    is_windowed: bool,
    is_maximized: bool,
    is_fullscreen: bool,
    right_space: f32,
) -> bool {
    is_windowed && !is_maximized && !is_fullscreen && right_space >= TOOL_PANEL_WIDTH
}

impl TinyShell {
    pub(crate) fn toggle_tool_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tool_panel.open {
            self.close_tool_panel(window, cx);
        } else {
            self.open_tool_panel(window, cx);
        }
    }

    pub(crate) fn open_tool_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tool_panel.open {
            return;
        }
        let current = window.window_bounds();
        let (bounds, is_windowed) = match current {
            gpui::WindowBounds::Windowed(bounds) => (bounds, true),
            gpui::WindowBounds::Maximized(bounds) | gpui::WindowBounds::Fullscreen(bounds) => {
                (bounds, false)
            }
        };
        let right_space = cx
            .displays()
            .iter()
            .map(|display| display.bounds())
            .max_by(|left, right| {
                overlap_area(*left, bounds).total_cmp(&overlap_area(*right, bounds))
            })
            .map(|display| {
                (display.origin.x + display.size.width - bounds.origin.x - bounds.size.width)
                    .as_f32()
                    .max(0.0)
            })
            .unwrap_or(0.0);

        self.tool_panel.open = true;
        self.tool_panel.base_window_bounds = Some(bounds);
        let viewport_size = window.viewport_size();
        self.tool_panel.base_viewport_size = Some(viewport_size);
        if should_extend_panel(
            is_windowed,
            window.is_maximized(),
            window.is_fullscreen(),
            right_space,
        ) {
            self.tool_panel.presentation = ToolPanelPresentation::Extended;
            window.resize(size(
                viewport_size.width + px(TOOL_PANEL_WIDTH),
                viewport_size.height,
            ));
        } else {
            self.tool_panel.presentation = ToolPanelPresentation::Overlay;
        }
        self.sync_tool_panel_target(cx);
        cx.notify();
    }

    pub(crate) fn close_tool_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tool_panel.open {
            return;
        }
        if self.tool_panel.presentation == ToolPanelPresentation::Extended
            && let Some(viewport_size) = self.tool_panel.base_viewport_size
        {
            window.resize(viewport_size);
        }
        self.tool_panel.open = false;
        self.tool_panel.base_window_bounds = None;
        self.tool_panel.base_viewport_size = None;
        self.tool_panel.pending = None;
        self.tool_panel.target_tab_id = None;
        self.tool_panel.target_generation = None;
        self.tool_panel.target_label.clear();
        self.tool_panel.target_detail.clear();
        self.tool_panel.target_connected = false;
        self.tool_panel.containers.clear();
        self.tool_panel.images.clear();
        self.tool_panel.error = None;
        cx.notify();
    }

    pub(crate) fn sync_tool_panel_target(&mut self, cx: &mut Context<Self>) {
        if !self.tool_panel.open {
            return;
        }
        let target = if self.home_page_open {
            None
        } else {
            self.workspace()
                .active_tab_id()
                .and_then(|id| self.workspace().terminal_tab(id))
                .map(|tab| {
                    let (label, detail) = match tab.kind {
                        TabKind::Local => (
                            t!("local_terminal").to_string(),
                            t!("docker_local_host").to_string(),
                        ),
                        TabKind::Ssh => tab.session.as_ref().map_or_else(
                            || (tab.title.clone(), String::new()),
                            |session| {
                                let address = if session.port == 22 {
                                    session.host.clone()
                                } else {
                                    format!("{}:{}", session.host, session.port)
                                };
                                (session.name.clone(), address)
                            },
                        ),
                        TabKind::Rdp => tab.session.as_ref().map_or_else(
                            || (tab.title.clone(), String::new()),
                            |session| {
                                let address = if session.port == 3389 {
                                    session.host.clone()
                                } else {
                                    format!("{}:{}", session.host, session.port)
                                };
                                (session.name.clone(), address)
                            },
                        ),
                    };
                    (
                        tab.id.clone(),
                        label,
                        detail,
                        tab.connected,
                        tab.backend_generation,
                    )
                })
        };
        let target_id = target.as_ref().map(|target| target.0.as_str());
        if self.tool_panel.target_tab_id.as_deref() == target_id
            && self.tool_panel.target_detail
                == target
                    .as_ref()
                    .map(|target| target.2.as_str())
                    .unwrap_or_default()
            && self.tool_panel.target_connected == target.as_ref().is_some_and(|target| target.3)
            && self.tool_panel.target_generation == target.as_ref().map(|target| target.4)
            && self.tool_panel.target_label
                == target
                    .as_ref()
                    .map(|target| target.1.as_str())
                    .unwrap_or_default()
        {
            return;
        }

        self.tool_panel.pending = None;
        self.tool_panel.containers.clear();
        self.tool_panel.images.clear();
        self.tool_panel.error = None;
        match target {
            Some((tab_id, label, detail, connected, generation)) => {
                self.tool_panel.target_tab_id = Some(tab_id);
                self.tool_panel.target_generation = Some(generation);
                self.tool_panel.target_label = label;
                self.tool_panel.target_detail = detail;
                self.tool_panel.target_connected = connected;
                if connected {
                    self.request_current_docker_page(cx);
                }
            }
            None => {
                self.tool_panel.target_tab_id = None;
                self.tool_panel.target_generation = None;
                self.tool_panel.target_label.clear();
                self.tool_panel.target_detail.clear();
                self.tool_panel.target_connected = false;
            }
        }
    }

    pub(crate) fn set_docker_page(&mut self, page: DockerPage, cx: &mut Context<Self>) {
        if self.tool_panel.page == page {
            return;
        }
        self.tool_panel.page = page;
        self.tool_panel.error = None;
        self.request_current_docker_page(cx);
        cx.notify();
    }

    pub(crate) fn set_docker_container_filter(
        &mut self,
        filter: DockerContainerFilter,
        cx: &mut Context<Self>,
    ) {
        if self.tool_panel.container_filter == filter {
            return;
        }
        self.tool_panel.container_filter = filter;
        cx.notify();
    }

    pub(crate) fn request_current_docker_page(&mut self, cx: &mut Context<Self>) {
        let operation = match self.tool_panel.page {
            DockerPage::Containers => DockerOperation::ListContainers,
            DockerPage::Images => DockerOperation::ListImages,
        };
        self.send_docker_operation(operation, cx);
    }

    pub(crate) fn request_docker_action(
        &mut self,
        container_id: String,
        action: DockerAction,
        cx: &mut Context<Self>,
    ) {
        self.send_docker_operation(
            DockerOperation::ContainerAction {
                action,
                container_id,
            },
            cx,
        );
    }

    fn send_docker_operation(&mut self, operation: DockerOperation, cx: &mut Context<Self>) {
        if self.tool_panel.pending.is_some() {
            return;
        }
        let Some(tab_id) = self.tool_panel.target_tab_id.clone() else {
            return;
        };
        let request_id = self.tool_panel.next_request_id();
        let sent = self
            .workspace()
            .terminal_tab(&tab_id)
            .filter(|tab| tab.connected)
            .is_some_and(|tab| {
                tab.send_backend(BackendCommand::Docker(DockerRequest {
                    request_id,
                    operation: operation.clone(),
                }))
            });
        if sent {
            self.tool_panel.error = None;
            self.tool_panel.pending = Some(PendingDockerRequest {
                id: request_id,
                tab_id,
                operation,
            });
        } else {
            self.tool_panel.error = Some(t!("docker_request_unavailable").to_string());
        }
        cx.notify();
    }

    pub(crate) fn confirm_docker_action(
        &mut self,
        container: DockerContainer,
        action: DockerAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !action.requires_confirmation() {
            self.request_docker_action(container.id, action, cx);
            return;
        }
        let target_tab_id = self.tool_panel.target_tab_id.clone();
        let target_generation = self.tool_panel.target_generation;
        let target_label = self.tool_panel.target_label.clone();
        let owner = cx.entity();
        let title = match action {
            DockerAction::Stop => t!("docker_confirm_stop_title").to_string(),
            DockerAction::Restart => t!("docker_confirm_restart_title").to_string(),
            DockerAction::Remove | DockerAction::ForceRemove => {
                t!("docker_confirm_remove_title").to_string()
            }
            DockerAction::Start => t!("docker_start").to_string(),
            DockerAction::EnableAutostart => t!("docker_enable_autostart").to_string(),
            DockerAction::DisableAutostart => t!("docker_disable_autostart").to_string(),
        };
        let description = if action == DockerAction::ForceRemove {
            t!(
                "docker_confirm_force_remove_desc",
                container = container.names.clone(),
                target = target_label
            )
            .to_string()
        } else if action == DockerAction::Remove {
            t!(
                "docker_confirm_remove_desc",
                container = container.names.clone(),
                target = target_label
            )
            .to_string()
        } else {
            t!(
                "docker_confirm_action_desc",
                container = container.names.clone(),
                target = target_label
            )
            .to_string()
        };
        window.open_alert_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title.clone())
                .description(description.clone())
                .button_props(
                    DialogButtonProps::default()
                        .show_cancel(true)
                        .cancel_text(t!("cancel").to_string())
                        .ok_text(t!("confirm").to_string())
                        .ok_variant(
                            if matches!(
                                action,
                                DockerAction::Stop
                                    | DockerAction::Remove
                                    | DockerAction::ForceRemove
                            ) {
                                ButtonVariant::Danger
                            } else {
                                ButtonVariant::Primary
                            },
                        ),
                )
                .on_ok({
                    let owner = owner.clone();
                    let container_id = container.id.clone();
                    let target_tab_id = target_tab_id.clone();
                    move |_, _, cx| {
                        owner.update(cx, |this, cx| {
                            if this.tool_panel.target_tab_id == target_tab_id
                                && this.tool_panel.target_generation == target_generation
                            {
                                this.request_docker_action(container_id.clone(), action, cx);
                            }
                        });
                        true
                    }
                })
        });
    }

    pub(crate) fn handle_docker_response(
        &mut self,
        tab_id: String,
        response: DockerResponse,
        cx: &mut Context<Self>,
    ) {
        if !docker_response_matches(
            self.tool_panel.pending.as_ref(),
            self.tool_panel.target_tab_id.as_deref(),
            &tab_id,
            response.request_id,
        ) {
            return;
        }
        let Some(completed_operation) = self
            .tool_panel
            .pending
            .as_ref()
            .map(|pending| pending.operation.clone())
        else {
            return;
        };
        self.tool_panel.pending = None;
        match response.result {
            Ok(DockerPayload::Containers(containers)) => {
                self.tool_panel.containers = containers;
                self.tool_panel.error = None;
            }
            Ok(DockerPayload::Images(images)) => {
                self.tool_panel.images = images;
                self.tool_panel.error = None;
            }
            Ok(DockerPayload::ActionCompleted(_)) => {
                self.tool_panel.error = None;
                self.tool_panel.page = DockerPage::Containers;
                self.request_current_docker_page(cx);
            }
            Err(error) => self.tool_panel.error = Some(docker_error_message(error)),
        }
        let completed_page = match completed_operation {
            DockerOperation::ListContainers | DockerOperation::ContainerAction { .. } => {
                DockerPage::Containers
            }
            DockerOperation::ListImages => DockerPage::Images,
        };
        if self.tool_panel.pending.is_none() && self.tool_panel.page != completed_page {
            self.request_current_docker_page(cx);
        }
        cx.notify();
    }
}

fn overlap_area(left: Bounds<Pixels>, right: Bounds<Pixels>) -> f32 {
    let left_x = left.origin.x.as_f32().max(right.origin.x.as_f32());
    let top_y = left.origin.y.as_f32().max(right.origin.y.as_f32());
    let right_x = (left.origin.x + left.size.width)
        .as_f32()
        .min((right.origin.x + right.size.width).as_f32());
    let bottom_y = (left.origin.y + left.size.height)
        .as_f32()
        .min((right.origin.y + right.size.height).as_f32());
    (right_x - left_x).max(0.0) * (bottom_y - top_y).max(0.0)
}

fn docker_error_message(error: String) -> String {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("not found")
        || normalized.contains("not recognized")
        || normalized.contains("no such file")
    {
        t!("docker_cli_not_found").to_string()
    } else if normalized.contains("cannot connect to the docker daemon")
        || normalized.contains("docker daemon is not running")
        || normalized.contains("docker_engine")
    {
        t!("docker_daemon_unavailable").to_string()
    } else if normalized.contains("permission denied")
        || normalized.contains("access is denied")
        || normalized.contains("got permission denied")
    {
        t!("docker_permission_denied").to_string()
    } else {
        error
    }
}

fn docker_response_matches(
    pending: Option<&PendingDockerRequest>,
    target_tab_id: Option<&str>,
    response_tab_id: &str,
    response_id: u64,
) -> bool {
    pending.is_some_and(|pending| {
        pending.id == response_id
            && pending.tab_id == response_tab_id
            && target_tab_id == Some(response_tab_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_only_extends_for_windowed_mode_with_enough_right_space() {
        assert!(should_extend_panel(true, false, false, TOOL_PANEL_WIDTH));
        assert!(!should_extend_panel(
            true,
            false,
            false,
            TOOL_PANEL_WIDTH - 1.0
        ));
        assert!(!should_extend_panel(false, false, false, 1000.0));
        assert!(!should_extend_panel(true, true, false, 1000.0));
        assert!(!should_extend_panel(true, false, true, 1000.0));
    }

    #[test]
    fn stale_or_wrong_target_docker_responses_are_rejected() {
        let pending = PendingDockerRequest {
            id: 7,
            tab_id: "tab-a".into(),
            operation: DockerOperation::ListContainers,
        };
        assert!(docker_response_matches(
            Some(&pending),
            Some("tab-a"),
            "tab-a",
            7
        ));
        assert!(!docker_response_matches(
            Some(&pending),
            Some("tab-a"),
            "tab-a",
            8
        ));
        assert!(!docker_response_matches(
            Some(&pending),
            Some("tab-b"),
            "tab-a",
            7
        ));
        assert!(!docker_response_matches(None, Some("tab-a"), "tab-a", 7));
    }

    #[test]
    fn expanded_panel_persists_the_pre_open_window_bounds() {
        let base = Bounds::new(gpui::point(px(10.), px(20.)), size(px(900.), px(700.)));
        let expanded = Bounds::new(gpui::point(px(10.), px(20.)), size(px(1260.), px(700.)));
        let state = ToolPanelState {
            open: true,
            presentation: ToolPanelPresentation::Extended,
            base_window_bounds: Some(base),
            ..ToolPanelState::default()
        };
        assert_eq!(
            state.persisted_window_bounds(gpui::WindowBounds::Windowed(expanded)),
            gpui::WindowBounds::Windowed(base)
        );
    }

    #[test]
    fn panel_layout_selects_a_single_non_recursive_render_path() {
        assert_eq!(
            tool_panel_layout(false, ToolPanelPresentation::Extended),
            ToolPanelLayout::Closed
        );
        assert_eq!(
            tool_panel_layout(true, ToolPanelPresentation::Extended),
            ToolPanelLayout::Extended
        );
        assert_eq!(
            tool_panel_layout(true, ToolPanelPresentation::Overlay),
            ToolPanelLayout::Overlay
        );
    }
}
