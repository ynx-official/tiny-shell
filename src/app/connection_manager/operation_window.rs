use std::path::PathBuf;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, PathPromptOptions, Render, StatefulInteractiveElement as _,
    Styled, Window, WindowOptions, point, px, rems, size,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Root, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use rust_i18n::t;

use crate::TinyShell;

#[derive(Clone)]
pub(crate) enum ConnectionOperation {
    EditGroup {
        original: Option<String>,
        parent: Option<String>,
    },
    MoveSession {
        session_id: String,
        session_name: String,
    },
    MoveGroup {
        group: String,
    },
    Archive {
        path: PathBuf,
        importing: bool,
    },
}

pub(crate) struct ConnectionOperationWindow {
    owner: Entity<TinyShell>,
    operation: ConnectionOperation,
    group_name_input: Option<Entity<InputState>>,
    archive_password_input: Option<Entity<InputState>>,
    focus_handle: FocusHandle,
    _owner_subscription: gpui::Subscription,
    _input_subscriptions: Vec<gpui::Subscription>,
}

impl ConnectionOperationWindow {
    fn new(
        owner: Entity<TinyShell>,
        operation: ConnectionOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let group_name_input = match &operation {
            ConnectionOperation::EditGroup { original, .. } => Some(cx.new(|cx| {
                InputState::new(window, cx).default_value(
                    original
                        .as_deref()
                        .and_then(|path| path.rsplit('/').next())
                        .unwrap_or_default(),
                )
            })),
            _ => None,
        };
        let archive_password_input = match &operation {
            ConnectionOperation::Archive { .. } => Some(cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("connection_archive_password").to_string())
                    .masked(true)
            })),
            _ => None,
        };
        let input_subscriptions = group_name_input
            .iter()
            .chain(archive_password_input.iter())
            .map(|input| cx.subscribe_in(input, window, |_, _, _: &InputEvent, _, cx| cx.notify()))
            .collect();
        let owner_subscription = cx.observe(&owner, |_, _, cx| cx.notify());

        Self {
            owner,
            operation,
            group_name_input,
            archive_password_input,
            focus_handle: cx.focus_handle(),
            _owner_subscription: owner_subscription,
            _input_subscriptions: input_subscriptions,
        }
    }

    fn submit_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = &self.group_name_input else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        let ConnectionOperation::EditGroup { original, parent } = &self.operation else {
            return;
        };
        let original = original.clone();
        let parent = parent.clone();
        let full_name = parent
            .as_deref()
            .map(|parent| format!("{parent}/{name}"))
            .unwrap_or(name);
        let saved = self.owner.update(cx, |owner, cx| {
            let mut staged = owner.config.clone();
            if let Some(original) = &original {
                staged.rename_connection_group(original, full_name.clone());
            } else {
                staged.add_connection_group(full_name.clone());
            }
            match crate::app::config_persistence::save_full(&staged) {
                Ok(()) => {
                    if owner.connection_group_filter.as_deref() == original.as_deref()
                        || original.is_none()
                    {
                        owner.connection_group_filter = Some(full_name);
                    }
                    owner.config = staged;
                    cx.notify();
                    true
                }
                Err(error) => {
                    owner.status = t!(
                        "connection_manager_action_failed",
                        error = error.to_string()
                    )
                    .to_string()
                    .into();
                    cx.notify();
                    false
                }
            }
        });
        if saved {
            window.remove_window();
        }
    }

    fn move_session(
        &mut self,
        target: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ConnectionOperation::MoveSession { session_id, .. } = &self.operation else {
            return;
        };
        let session_id = session_id.clone();
        if commit_catalog_change(&self.owner, window, cx, move |config| {
            crate::session::connection_catalog::move_session(config, &session_id, target.as_deref())
        }) {
            window.remove_window();
        }
    }

    fn move_group(&mut self, target: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let ConnectionOperation::MoveGroup { group } = &self.operation else {
            return;
        };
        let group = group.clone();
        if commit_catalog_change(&self.owner, window, cx, move |config| {
            crate::session::connection_catalog::move_connection_group(
                config,
                &group,
                target.as_deref(),
            )
            .map(|_| ())
        }) {
            window.remove_window();
        }
    }

    fn run_archive(&self, path: &PathBuf, password: &str, cx: &mut Context<Self>) -> bool {
        self.owner.update(cx, |owner, cx| {
            let result = match self.operation {
                ConnectionOperation::Archive {
                    importing: true, ..
                } => owner.import_connection_archive(path, password),
                ConnectionOperation::Archive {
                    importing: false, ..
                } => owner.export_connection_archive(path, password),
                _ => return false,
            };
            if let Err(error) = result {
                owner.status = t!("connection_archive_failed", error = error.to_string())
                    .to_string()
                    .into();
                cx.notify();
                false
            } else {
                cx.notify();
                true
            }
        })
    }

    fn submit_archive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(password_input) = &self.archive_password_input else {
            return;
        };
        let password = password_input.read(cx).value().to_string();
        if password.is_empty() {
            self.owner.update(cx, |owner, cx| {
                owner.status = t!("connection_archive_password_required")
                    .to_string()
                    .into();
                cx.notify();
            });
            return;
        }
        let ConnectionOperation::Archive { path, importing } = &self.operation else {
            return;
        };
        if !path.as_os_str().is_empty() {
            if self.run_archive(path, &password, cx) {
                window.remove_window();
            }
            return;
        }

        let importing = *importing;
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: importing,
            directories: !importing,
            multiple: false,
            prompt: Some(
                t!(if importing {
                    "connection_archive_import"
                } else {
                    "connection_archive_export"
                })
                .to_string()
                .into(),
            ),
        });
        let window_handle = window.window_handle();
        cx.spawn_in(window, async move |this, cx| {
            match prompt.await {
                Ok(Ok(Some(mut paths))) => {
                    if let Some(path) = paths.pop() {
                        let path = if importing {
                            path
                        } else {
                            path.join("tiny-shell-connections.json")
                        };
                        let succeeded =
                            this.update(cx, |this, cx| this.run_archive(&path, &password, cx))?;
                        if succeeded {
                            let _ = window_handle.update(cx, |_, window, _| window.remove_window());
                        }
                    }
                }
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.owner.update(cx, |owner, cx| {
                            owner.status = t!(
                                "connection_archive_picker_failed",
                                error = error.to_string()
                            )
                            .to_string()
                            .into();
                            cx.notify();
                        });
                    })?;
                }
                _ => {}
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    fn render_archive(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(password) = self.archive_password_input.clone() else {
            return v_flex().into_any_element();
        };
        v_flex()
            .size_full()
            .gap_3()
            .child(Input::new(&password).mask_toggle())
            .child(
                h_flex()
                    .mt_auto()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("connection-archive-window-cancel")
                            .secondary()
                            .label(t!("cancel").to_string())
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        Button::new("connection-archive-window-confirm")
                            .primary()
                            .label(t!("confirm").to_string())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.submit_archive(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_group_editor(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(input) = self.group_name_input.clone() else {
            return v_flex().into_any_element();
        };
        v_flex()
            .size_full()
            .gap_3()
            .child(
                gpui::div()
                    .text_size(rems(0.917))
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("connection_group_name")),
            )
            .child(Input::new(&input).w_full())
            .child(
                h_flex()
                    .mt_auto()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("connection-operation-cancel")
                            .secondary()
                            .label(t!("cancel").to_string())
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        Button::new("connection-operation-save")
                            .primary()
                            .label(t!("save").to_string())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.submit_group(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_move_picker(
        &self,
        source_label: String,
        groups: Vec<String>,
        is_group: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let root_label = if is_group {
            t!("connection_group_move_root").to_string()
        } else {
            t!("connection_group_ungrouped").to_string()
        };
        v_flex()
            .size_full()
            .gap_3()
            .child(
                gpui::div()
                    .text_size(rems(0.917))
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{}: {}",
                        t!("connection_group_move_source"),
                        source_label
                    )),
            )
            .child(
                v_flex()
                    .id("connection-operation-targets")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .p_2()
                    .gap_1()
                    .child(move_target_row(
                        "connection-operation-root",
                        root_label,
                        0,
                        None,
                        is_group,
                        window,
                        cx,
                    ))
                    .children(groups.into_iter().enumerate().map(|(index, group)| {
                        let depth = group.matches('/').count();
                        let label = group.rsplit('/').next().unwrap_or(&group).to_string();
                        move_target_row(
                            ("connection-operation-target", index),
                            label,
                            depth,
                            Some(group),
                            is_group,
                            window,
                            cx,
                        )
                    })),
            )
            .into_any_element()
    }
}

impl Render for ConnectionOperationWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.operation {
            ConnectionOperation::EditGroup { .. } => self.render_group_editor(window, cx),
            ConnectionOperation::MoveSession { session_name, .. } => self.render_move_picker(
                session_name.clone(),
                self.owner.read(cx).config.connection_groups().to_vec(),
                false,
                window,
                cx,
            ),
            ConnectionOperation::MoveGroup { group } => {
                let candidates = self
                    .owner
                    .read(cx)
                    .config
                    .connection_groups()
                    .iter()
                    .filter(|candidate| {
                        candidate.as_str() != group && !candidate.starts_with(&format!("{group}/"))
                    })
                    .cloned()
                    .collect();
                self.render_move_picker(
                    group.rsplit('/').next().unwrap_or(group).to_string(),
                    candidates,
                    true,
                    window,
                    cx,
                )
            }
            ConnectionOperation::Archive { .. } => self.render_archive(cx),
        };

        v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .bg(cx.theme().background)
            .p_4()
            .child(content)
    }
}

fn move_target_row(
    id: impl Into<gpui::ElementId>,
    label: String,
    depth: usize,
    target: Option<String>,
    is_group: bool,
    _window: &mut Window,
    cx: &mut Context<ConnectionOperationWindow>,
) -> gpui::AnyElement {
    gpui::div()
        .id(id)
        .w_full()
        .cursor_pointer()
        .rounded_md()
        .hover(|this| this.bg(cx.theme().secondary))
        .on_click(cx.listener(move |this, _, window, cx| {
            if is_group {
                this.move_group(target.clone(), window, cx);
            } else {
                this.move_session(target.clone(), window, cx);
            }
        }))
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .p_2()
                .pl(px(8. + depth as f32 * 16.))
                .child(Icon::new(IconName::Folder).with_size(Size::Small))
                .child(label),
        )
        .into_any_element()
}

fn commit_catalog_change(
    owner: &Entity<TinyShell>,
    _window: &mut Window,
    cx: &mut Context<ConnectionOperationWindow>,
    change: impl FnOnce(&mut crate::session::config::ConfigStore) -> anyhow::Result<()>,
) -> bool {
    owner.update(cx, |owner, cx| {
        let mut staged = owner.config.clone();
        match change(&mut staged).and_then(|_| crate::app::config_persistence::save_full(&staged)) {
            Ok(()) => {
                owner.config = staged;
                cx.notify();
                true
            }
            Err(error) => {
                owner.status = t!(
                    "connection_manager_action_failed",
                    error = error.to_string()
                )
                .to_string()
                .into();
                cx.notify();
                false
            }
        }
    })
}

fn window_options(cx: &App, compact: bool) -> WindowOptions {
    let mut options = WindowOptions {
        is_movable: true,
        is_resizable: !compact,
        is_minimizable: true,
        window_min_size: Some(if compact {
            size(px(380.), px(180.))
        } else {
            size(px(440.), px(420.))
        }),
        ..Default::default()
    };
    if let Some(display) = cx.displays().first().cloned() {
        let display_bounds = display.bounds();
        let window_size = if compact {
            size(px(420.), px(220.))
        } else {
            size(px(480.), px(560.))
        };
        let origin = point(
            display_bounds.origin.x + (display_bounds.size.width - window_size.width) / 2.,
            display_bounds.origin.y + (display_bounds.size.height - window_size.height) / 2.,
        );
        options.window_bounds = Some(gpui::WindowBounds::Windowed(Bounds::new(
            origin,
            window_size,
        )));
    }
    #[cfg(not(target_os = "macos"))]
    if let Ok(image) =
        image::load_from_memory(include_bytes!("../../../assets/icons/tiny-shell.png"))
    {
        options.icon = Some(std::sync::Arc::new(image.into_rgba8()));
    }
    options
}

impl TinyShell {
    pub(crate) fn open_connection_operation_window(
        &mut self,
        operation: ConnectionOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity();
        window.defer(cx, move |_, cx| open(owner, operation, cx));
    }
}

pub(crate) fn open(owner: Entity<TinyShell>, operation: ConnectionOperation, cx: &mut App) {
    let compact = matches!(
        operation,
        ConnectionOperation::EditGroup { .. } | ConnectionOperation::Archive { .. }
    );
    let title = match operation {
        ConnectionOperation::EditGroup { .. } => t!("connection_group_dialog_title").to_string(),
        ConnectionOperation::MoveSession { .. } => t!("connection_group_move_to").to_string(),
        ConnectionOperation::MoveGroup { .. } => {
            t!("connection_group_move_dialog_title").to_string()
        }
        ConnectionOperation::Archive { importing, .. } => t!(if importing {
            "connection_archive_import"
        } else {
            "connection_archive_export"
        })
        .to_string(),
    };
    let opened = cx.open_window(window_options(cx, compact), move |window, cx| {
        window.set_window_title(&title);
        let view =
            cx.new(|cx| ConnectionOperationWindow::new(owner.clone(), operation, window, cx));
        let focus_input = view
            .read(cx)
            .group_name_input
            .clone()
            .or_else(|| view.read(cx).archive_password_input.clone());
        if let Some(input) = focus_input {
            window.defer(cx, move |window, cx| {
                window.activate_window();
                let focus_handle = input.read(cx).focus_handle(cx);
                window.focus(&focus_handle, cx);
            });
        }
        cx.new(|cx| Root::new(view, window, cx))
    });
    if let Err(error) = opened {
        tracing::error!("failed to open connection operation window: {error:?}");
    }
}
