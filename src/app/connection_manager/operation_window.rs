use std::{collections::HashSet, path::PathBuf};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, PathPromptOptions, Render, StatefulInteractiveElement as _,
    Styled, Window, WindowOptions, prelude::FluentBuilder as _, px, rems, size,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Root, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
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
    move_picker_expanded: HashSet<String>,
    _owner_subscription: gpui::Subscription,
    _input_subscriptions: Vec<gpui::Subscription>,
}

impl ConnectionOperationWindow {
    fn close_window(window: &mut Window) {
        crate::app::deregister_auxiliary_window(window.window_handle());
        window.remove_window();
    }

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
            move_picker_expanded: HashSet::new(),
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
            crate::feedback::Feedback::warning(window, cx, t!("connection_group_name"));
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
        let result = self.owner.update(cx, |owner, cx| {
            let mut staged = owner.config.clone();
            if let Some(original) = &original {
                staged.rename_connection_group(original, full_name.clone());
            } else {
                staged.add_connection_group(full_name.clone());
            }
            match crate::app::config_persistence::save_full(&owner.config_repository, &staged) {
                Ok(()) => {
                    if owner.connection_group_filter.as_deref() == original.as_deref()
                        || original.is_none()
                    {
                        owner.connection_group_filter = Some(full_name);
                    }
                    owner.config = staged;
                    cx.notify();
                    Ok(())
                }
                Err(error) => {
                    let message = t!(
                        "connection_manager_action_failed",
                        error = error.to_string()
                    )
                    .to_string();
                    owner.status = message.clone().into();
                    cx.notify();
                    Err(message)
                }
            }
        });
        match result {
            Ok(()) => {
                crate::feedback::Feedback::show_for_owner(
                    &self.owner,
                    cx,
                    crate::feedback::FeedbackKind::Success,
                    format!("{} · {}", t!("connection_group_dialog_title"), t!("save")),
                );
                Self::close_window(window);
            }
            Err(message) => crate::feedback::Feedback::error(window, cx, message),
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
            crate::feedback::Feedback::show_for_owner(
                &self.owner,
                cx,
                crate::feedback::FeedbackKind::Success,
                t!("connection_group_move_to"),
            );
            Self::close_window(window);
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
            crate::feedback::Feedback::show_for_owner(
                &self.owner,
                cx,
                crate::feedback::FeedbackKind::Success,
                t!("connection_group_move_to"),
            );
            Self::close_window(window);
        }
    }

    fn run_archive(
        &self,
        path: &PathBuf,
        password: &str,
        cx: &mut Context<Self>,
    ) -> Result<gpui::SharedString, String> {
        self.owner.update(cx, |owner, cx| {
            let result = match self.operation {
                ConnectionOperation::Archive {
                    importing: true, ..
                } => owner.import_connection_archive(path, password),
                ConnectionOperation::Archive {
                    importing: false, ..
                } => owner.export_connection_archive(path, password),
                _ => return Err(String::new()),
            };
            if let Err(error) = result {
                let message =
                    t!("connection_archive_failed", error = error.to_string()).to_string();
                owner.status = message.clone().into();
                cx.notify();
                Err(message)
            } else {
                let message = owner.status.clone();
                cx.notify();
                Ok(message)
            }
        })
    }

    fn submit_archive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(password_input) = &self.archive_password_input else {
            return;
        };
        let password = password_input.read(cx).value().to_string();
        if password.is_empty() {
            let message = t!("connection_archive_password_required").to_string();
            self.owner.update(cx, |owner, cx| {
                owner.status = message.clone().into();
                cx.notify();
            });
            crate::feedback::Feedback::warning(window, cx, message);
            return;
        }
        let ConnectionOperation::Archive { path, importing } = &self.operation else {
            return;
        };
        if !path.as_os_str().is_empty() {
            match self.run_archive(path, &password, cx) {
                Ok(message) => {
                    crate::feedback::Feedback::show_for_owner(
                        &self.owner,
                        cx,
                        crate::feedback::FeedbackKind::Success,
                        message,
                    );
                    Self::close_window(window);
                }
                Err(message) if !message.is_empty() => {
                    crate::feedback::Feedback::error(window, cx, message);
                }
                Err(_) => {}
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
                        let result =
                            this.update(cx, |this, cx| this.run_archive(&path, &password, cx))?;
                        match result {
                            Ok(message) => {
                                this.update(cx, |this, cx| {
                                    crate::feedback::Feedback::show_for_owner(
                                        &this.owner,
                                        cx,
                                        crate::feedback::FeedbackKind::Success,
                                        message,
                                    );
                                })?;
                                let _ = window_handle.update(cx, |_, window, _| {
                                    Self::close_window(window);
                                });
                            }
                            Err(message) if !message.is_empty() => {
                                let _ = window_handle.update(cx, |_, window, cx| {
                                    crate::feedback::Feedback::error(window, cx, message);
                                });
                            }
                            Err(_) => {}
                        }
                    }
                }
                Ok(Err(error)) => {
                    let message = t!(
                        "connection_archive_picker_failed",
                        error = error.to_string()
                    )
                    .to_string();
                    this.update(cx, |this, cx| {
                        this.owner.update(cx, |owner, cx| {
                            owner.status = message.clone().into();
                            cx.notify();
                        });
                    })?;
                    let _ = window_handle.update(cx, |_, window, cx| {
                        crate::feedback::Feedback::error(window, cx, message);
                    });
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
                            .on_click(|_, window, _| Self::close_window(window)),
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
                            .on_click(|_, window, _| Self::close_window(window)),
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
        &mut self,
        source_label: String,
        groups: Vec<String>,
        is_group: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let root_label = if is_group {
            t!("connection_group_move_root").to_string()
        } else {
            t!("connection_group_ungrouped").to_string()
        };
        let visible_groups = visible_move_targets(&groups, &self.move_picker_expanded);

        v_flex()
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .h(px(32.))
                    .items_center()
                    .gap_2()
                    .child(
                        gpui::div()
                            .text_size(rems(0.78))
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("connection_group_move_source")),
                    )
                    .child(
                        gpui::div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(rems(0.875))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(source_label),
                    ),
            )
            .child(
                v_flex()
                    .id("connection-operation-targets")
                    .flex_1()
                    .min_h(px(0.))
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .text_color(cx.theme().popover_foreground)
                    .p_1()
                    .child(move_target_row(
                        "connection-operation-root",
                        MoveTargetRow {
                            label: root_label,
                            depth: 0,
                            target: None,
                            has_children: false,
                            expanded: false,
                        },
                        is_group,
                        cx,
                    ))
                    .child(
                        gpui::div()
                            .mx_1()
                            .border_t_1()
                            .border_color(cx.theme().border),
                    )
                    .child(
                        v_flex()
                            .id("connection-operation-target-tree")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scrollbar()
                            .children(visible_groups.into_iter().enumerate().map(
                                |(index, (group, depth))| {
                                    let label =
                                        group.rsplit('/').next().unwrap_or(&group).to_string();
                                    let has_children = groups.iter().any(|candidate| {
                                        candidate
                                            .strip_prefix(&format!("{group}/"))
                                            .is_some_and(|rest| !rest.contains('/'))
                                    });
                                    let expanded = self.move_picker_expanded.contains(&group);
                                    move_target_row(
                                        ("connection-operation-target", index),
                                        MoveTargetRow {
                                            label,
                                            depth,
                                            target: Some(group),
                                            has_children,
                                            expanded,
                                        },
                                        is_group,
                                        cx,
                                    )
                                },
                            )),
                    ),
            )
            .child(
                h_flex().justify_end().child(
                    Button::new("connection-operation-move-cancel")
                        .secondary()
                        .label(t!("cancel").to_string())
                        .on_click(|_, window, _| Self::close_window(window)),
                ),
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
            .children(Root::render_notification_layer(window, cx))
    }
}

struct MoveTargetRow {
    label: String,
    depth: usize,
    target: Option<String>,
    has_children: bool,
    expanded: bool,
}

fn move_target_row(
    id: impl Into<gpui::ElementId>,
    row: MoveTargetRow,
    is_group: bool,
    cx: &mut Context<ConnectionOperationWindow>,
) -> gpui::AnyElement {
    let toggle_target = row.target.clone();
    let toggle_id = row.target.clone().unwrap_or_else(|| "__root__".to_string());
    let disclosure_icon = if row.expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    let folder_icon = if row.expanded {
        IconName::FolderOpen
    } else {
        IconName::Folder
    };
    let target = row.target;

    h_flex()
        .id(id)
        .w_full()
        .h(px(32.))
        .items_center()
        .gap_2()
        .pl(px(10. + row.depth as f32 * 16.))
        .pr_2()
        .cursor_pointer()
        .rounded_sm()
        .text_size(rems(0.78))
        .hover(|this| this.bg(cx.theme().secondary.opacity(0.65)))
        .on_click(cx.listener(move |this, _, window, cx| {
            if is_group {
                this.move_group(target.clone(), window, cx);
            } else {
                this.move_session(target.clone(), window, cx);
            }
        }))
        .child(
            gpui::div()
                .id(gpui::SharedString::from(format!(
                    "connection-operation-target-toggle-{toggle_id}"
                )))
                .w(px(16.))
                .h(px(18.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .when(row.has_children, |this| {
                    this.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(target) = toggle_target.as_deref()
                                && !this.move_picker_expanded.remove(target)
                            {
                                this.move_picker_expanded.insert(target.to_string());
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }))
                })
                .when(row.has_children, |this| {
                    this.child(Icon::new(disclosure_icon).with_size(Size::Small))
                }),
        )
        .child(
            gpui::div()
                .w(px(16.))
                .h(px(18.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(folder_icon).with_size(Size::Small)),
        )
        .child(gpui::div().min_w_0().flex_1().truncate().child(row.label))
        .into_any_element()
}

fn visible_move_targets(groups: &[String], expanded: &HashSet<String>) -> Vec<(String, usize)> {
    let group_set = groups.iter().collect::<HashSet<_>>();
    let mut children = std::collections::HashMap::<Option<String>, Vec<String>>::new();
    for group in groups {
        let parent = group
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_string())
            .filter(|parent| group_set.contains(parent));
        children.entry(parent).or_default().push(group.clone());
    }

    fn append(
        parent: Option<&str>,
        depth: usize,
        children: &mut std::collections::HashMap<Option<String>, Vec<String>>,
        expanded: &HashSet<String>,
        result: &mut Vec<(String, usize)>,
    ) {
        let key = parent.map(str::to_string);
        let Some(groups) = children.remove(&key) else {
            return;
        };
        for group in groups {
            let is_expanded = expanded.contains(&group);
            result.push((group.clone(), depth));
            if is_expanded {
                append(Some(&group), depth + 1, children, expanded, result);
            }
        }
    }

    let mut result = Vec::new();
    append(None, 0, &mut children, expanded, &mut result);
    result
}

fn commit_catalog_change(
    owner: &Entity<TinyShell>,
    window: &mut Window,
    cx: &mut Context<ConnectionOperationWindow>,
    change: impl FnOnce(&mut crate::session::config::ConfigStore) -> anyhow::Result<()>,
) -> bool {
    let result = owner.update(cx, |owner, cx| {
        let mut staged = owner.config.clone();
        match change(&mut staged).and_then(|_| {
            crate::app::config_persistence::save_full(&owner.config_repository, &staged)
        }) {
            Ok(()) => {
                owner.config = staged;
                cx.notify();
                Ok(())
            }
            Err(error) => {
                let message = t!(
                    "connection_manager_action_failed",
                    error = error.to_string()
                )
                .to_string();
                owner.status = message.clone().into();
                cx.notify();
                Err(message)
            }
        }
    });
    match result {
        Ok(()) => true,
        Err(message) => {
            crate::feedback::Feedback::error(window, cx, message);
            false
        }
    }
}

fn window_options(cx: &mut App, compact: bool) -> WindowOptions {
    let (preferred_size, min_size) = if compact {
        (size(px(420.), px(220.)), size(px(380.), px(180.)))
    } else {
        (size(px(440.), px(440.)), size(px(400.), px(340.)))
    };
    crate::app::platform::auxiliary_window_options(
        cx,
        crate::app::platform::AuxiliaryWindowSpec::new(preferred_size)
            .with_min_size(min_size)
            .with_max_ratio(0.9, 0.9)
            .resizable(!compact),
    )
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
    let owner_id = owner.read(cx).session_owner_id;
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
    let owner_for_window = owner.clone();
    let options = window_options(cx, compact);
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(&title);
        let window_handle = window.window_handle();
        crate::app::register_auxiliary_window(window_handle, owner_id);
        let view = cx.new(|cx| {
            ConnectionOperationWindow::new(owner_for_window.clone(), operation, window, cx)
        });
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
        window.on_window_should_close(cx, move |_, _| {
            crate::app::deregister_auxiliary_window(window_handle);
            true
        });
        cx.new(|cx| Root::new(view, window, cx))
    });
    if let Err(error) = opened {
        tracing::error!("failed to open connection operation window: {error:?}");
        crate::feedback::Feedback::show_for_owner(
            &owner,
            cx,
            crate::feedback::FeedbackKind::Error,
            t!(
                "connection_manager_action_failed",
                error = format!("{error:?}")
            )
            .to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::visible_move_targets;
    use std::collections::HashSet;

    #[test]
    fn move_picker_shows_only_root_groups_by_default() {
        let groups = vec![
            "prod".to_string(),
            "prod/eu".to_string(),
            "prod/eu/database".to_string(),
            "shared".to_string(),
        ];

        assert_eq!(
            visible_move_targets(&groups, &HashSet::new()),
            vec![("prod".to_string(), 0), ("shared".to_string(), 0)]
        );
    }

    #[test]
    fn move_picker_expands_only_the_requested_branch() {
        let groups = vec![
            "prod".to_string(),
            "prod/eu".to_string(),
            "prod/us".to_string(),
            "shared".to_string(),
            "shared/tools".to_string(),
        ];
        let expanded = HashSet::from(["prod".to_string()]);

        assert_eq!(
            visible_move_targets(&groups, &expanded),
            vec![
                ("prod".to_string(), 0),
                ("prod/eu".to_string(), 1),
                ("prod/us".to_string(), 1),
                ("shared".to_string(), 0),
            ]
        );
    }
}
