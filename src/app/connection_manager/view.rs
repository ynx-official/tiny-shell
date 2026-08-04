#![allow(dead_code)]

use std::path::PathBuf;

use gpui::{
    AnyElement, App, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, StatefulInteractiveElement as _, Styled, Window,
    div, prelude::FluentBuilder as _, px, rems,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, Size, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    scroll::{Scrollbar, ScrollbarAxis, ScrollbarShow},
    v_flex,
};
use rust_i18n::t;

use super::{
    actions::ConnectionManagerAction,
    state::{ConnectionManagerState, ConnectionNodeId, ConnectionTreeNode},
};
use crate::{app::TinyShell, session::config::Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionContext {
    Session,
    Group,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnectionMenuItem {
    Connect,
    Edit,
    Rename,
    Copy,
    Cut,
    Paste,
    Delete,
    Restore,
    CopyAddress,
    PasteAddress,
    MoveToGroup,
    NewConnection,
    NewGroup,
    Import,
}

pub(crate) fn context_for(node: Option<&ConnectionTreeNode>) -> ConnectionContext {
    match node {
        Some(ConnectionTreeNode::Session { .. } | ConnectionTreeNode::DeletedSession { .. }) => {
            ConnectionContext::Session
        }
        Some(ConnectionTreeNode::Group { .. } | ConnectionTreeNode::DeletedGroup { .. }) => {
            ConnectionContext::Group
        }
        None => ConnectionContext::Empty,
    }
}

pub(crate) fn menu_items(context: ConnectionContext, deleted: bool) -> Vec<ConnectionMenuItem> {
    if deleted {
        return vec![ConnectionMenuItem::Restore];
    }
    match context {
        ConnectionContext::Session => vec![
            ConnectionMenuItem::Connect,
            ConnectionMenuItem::Edit,
            ConnectionMenuItem::CopyAddress,
            ConnectionMenuItem::Copy,
            ConnectionMenuItem::Cut,
            ConnectionMenuItem::MoveToGroup,
            ConnectionMenuItem::Delete,
        ],
        ConnectionContext::Group => vec![
            ConnectionMenuItem::NewConnection,
            ConnectionMenuItem::NewGroup,
            ConnectionMenuItem::Rename,
            ConnectionMenuItem::Copy,
            ConnectionMenuItem::Cut,
            ConnectionMenuItem::Paste,
            ConnectionMenuItem::MoveToGroup,
            ConnectionMenuItem::Delete,
        ],
        ConnectionContext::Empty => vec![
            ConnectionMenuItem::NewConnection,
            ConnectionMenuItem::NewGroup,
            ConnectionMenuItem::Paste,
            ConnectionMenuItem::PasteAddress,
            ConnectionMenuItem::Import,
        ],
    }
}

pub(crate) fn action_for(
    item: ConnectionMenuItem,
    node: &ConnectionNodeId,
) -> Option<ConnectionManagerAction> {
    match (item, node) {
        (ConnectionMenuItem::Copy, ConnectionNodeId::Session(id)) => {
            Some(ConnectionManagerAction::CopySession { id: id.clone() })
        }
        (ConnectionMenuItem::Cut, ConnectionNodeId::Session(id)) => {
            Some(ConnectionManagerAction::CutSession { id: id.clone() })
        }
        (ConnectionMenuItem::CopyAddress, ConnectionNodeId::Session(id)) => {
            Some(ConnectionManagerAction::CopyAddress { id: id.clone() })
        }
        (ConnectionMenuItem::Copy, ConnectionNodeId::Group(name)) => {
            Some(ConnectionManagerAction::CopyGroup { name: name.clone() })
        }
        (ConnectionMenuItem::Delete, ConnectionNodeId::Session(id)) => {
            Some(ConnectionManagerAction::DeleteSession { id: id.clone() })
        }
        (ConnectionMenuItem::Delete, ConnectionNodeId::Group(name)) => {
            Some(ConnectionManagerAction::DeleteGroup { name: name.clone() })
        }
        (ConnectionMenuItem::Restore, ConnectionNodeId::DeletedSession(id)) => {
            Some(ConnectionManagerAction::RestoreSession { id: id.clone() })
        }
        (ConnectionMenuItem::Restore, ConnectionNodeId::DeletedGroup(name)) => {
            Some(ConnectionManagerAction::RestoreGroup { name: name.clone() })
        }
        _ => None,
    }
}

pub(crate) fn render(
    view: &Entity<TinyShell>,
    search_input: &Entity<gpui_component::input::InputState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state_entity = view.read(cx).connection_manager_state.clone();
    let search = search_input.read(cx).value().trim().to_string();
    state_entity.update(cx, |state, _| state.set_query(search));
    let state = state_entity.read(cx).clone();
    let nodes = {
        let shell = view.read(cx);
        state.visible_nodes(&shell.config)
    };
    let visible_connections = nodes
        .iter()
        .filter(|node| matches!(node, ConnectionTreeNode::Session { .. }))
        .count();
    let recycle_count = {
        let shell = view.read(cx);
        shell.config.deleted_sessions().len()
            + shell
                .config
                .deleted_connection_groups()
                .iter()
                .map(|group| 1 + group.sessions.len())
                .sum::<usize>()
    };

    let toolbar = render_toolbar(view, search_input, &state, window);
    let rows = render_rows(view, &state, nodes, window, cx);
    let row_area = v_flex()
        .id("connection-manager-tree")
        .size_full()
        .track_scroll(&view.read(cx).quick_connection_scroll_handle)
        .overflow_y_scroll()
        .children(rows)
        .child(if recycle_count == 0 && visible_connections == 0 {
            div()
                .id("connection-manager-empty-context")
                .flex_1()
                .min_h(px(120.))
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .context_menu({
                    let view = view.clone();
                    move |menu, window, _| render_empty_menu(menu, &view, window)
                })
                .child(t!("quick_connection_empty").to_string())
                .into_any_element()
        } else {
            div()
                .id("connection-manager-empty-context")
                .flex_1()
                .min_h(px(1.))
                .context_menu({
                    let view = view.clone();
                    move |menu, window, _| render_empty_menu(menu, &view, window)
                })
                .into_any_element()
        });

    v_flex()
        .size_full()
        .gap_3()
        .child(toolbar)
        .child(
            v_flex()
                .relative()
                .flex_1()
                .min_h(px(0.))
                .overflow_hidden()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .child(render_header(cx))
                .child(row_area)
                .child(
                    div().absolute().top(px(32.)).bottom_0().right_0().child(
                        Scrollbar::new(&view.read(cx).quick_connection_scroll_handle)
                            .id("connection-manager-scrollbar")
                            .axis(ScrollbarAxis::Vertical)
                            .scrollbar_show(ScrollbarShow::Scrolling),
                    ),
                ),
        )
        .child(
            h_flex()
                .flex_none()
                .justify_between()
                .text_size(rems(0.72))
                .text_color(cx.theme().muted_foreground)
                .child(t!("quick_connection_status", count = visible_connections).to_string())
                .child(t!("quick_connection_recycle_status", count = recycle_count).to_string()),
        )
        .into_any_element()
}

fn render_toolbar(
    view: &Entity<TinyShell>,
    search_input: &Entity<gpui_component::input::InputState>,
    state: &ConnectionManagerState,
    window: &mut Window,
) -> AnyElement {
    let show_deleted = state.show_deleted;
    h_flex()
        .flex_none()
        .gap_2()
        .items_center()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(Input::new(search_input).small()),
        )
        .child(
            Button::new("connection-manager-trash")
                .small()
                .label(if show_deleted {
                    t!("quick_connection_hide_deleted").to_string()
                } else {
                    t!("quick_connection_show_deleted").to_string()
                })
                .on_click(window.listener_for(view, |this, _, _, cx| {
                    this.connection_manager_state
                        .update(cx, |state, _| state.toggle_deleted());
                    cx.notify();
                })),
        )
        .child(sort_button(view, state))
        .child(
            Button::new("connection-manager-import")
                .small()
                .label(t!("connection_archive_import").to_string())
                .dropdown_menu_with_anchor(gpui::Anchor::BottomRight, {
                    let view = view.clone();
                    move |mut menu, window, _| {
                        menu = menu.item(
                            PopupMenuItem::new(t!("connection_archive_import_tiny_shell").to_string())
                                .on_click(window.listener_for(&view, |this, _, window, cx| {
                                    this.open_connection_operation_window(
                                        crate::app::connection_manager::operation_window::ConnectionOperation::Archive {
                                            path: PathBuf::new(),
                                            importing: true,
                                        },
                                        window,
                                        cx,
                                    );
                                })),
                        );
                        menu.item(
                            PopupMenuItem::new(t!("finalshell_import_title").to_string())
                                .on_click(window.listener_for(&view, |this, _, window, cx| {
                                    this.open_finalshell_import_window(window, cx);
                                })),
                        )
                    }
                }),
        )
        .child(
            Button::new("connection-manager-export")
                .small()
                .label(t!("connection_archive_export").to_string())
                .on_click(window.listener_for(view, |this, _, window, cx| {
                    this.open_connection_operation_window(
                        crate::app::connection_manager::operation_window::ConnectionOperation::Archive {
                            path: PathBuf::new(),
                            importing: false,
                        },
                        window,
                        cx,
                    );
                })),
        )
        .child(
            Button::new("connection-manager-new")
                .primary()
                .small()
                .label(t!("overview_new_connection").to_string())
                .on_click(window.listener_for(view, |this, _, window, cx| {
                    this.active_dialog = None;
                    window.close_dialog(cx);
                    this.open_new_ssh_dialog(window, cx);
                })),
        )
        .into_any_element()
}

fn sort_button(view: &Entity<TinyShell>, state: &ConnectionManagerState) -> AnyElement {
    let current = state.sort;
    let descending = state.descending;
    Button::new("connection-manager-sort")
        .small()
        .label(t!("connection_manager_sort").to_string())
        .dropdown_menu_with_anchor(gpui::Anchor::BottomRight, {
            let view = view.clone();
            move |mut menu, window, _| {
                for (key, label) in [
                    (super::state::ConnectionSort::Name, t!("name").to_string()),
                    (super::state::ConnectionSort::Host, t!("host").to_string()),
                    (super::state::ConnectionSort::User, t!("user").to_string()),
                    (
                        super::state::ConnectionSort::LastUsed,
                        t!("connection_manager_last_used").to_string(),
                    ),
                ] {
                    let checked = current == key;
                    menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                        window.listener_for(&view, move |this, _, _, cx| {
                            this.connection_manager_state.update(cx, |state, _| {
                                state.sort = key;
                            });
                            cx.notify();
                        }),
                    ));
                }
                menu = menu.separator().item(
                    PopupMenuItem::new(t!("connection_manager_sort_descending").to_string())
                        .checked(descending)
                        .on_click(window.listener_for(&view, |this, _, _, cx| {
                            this.connection_manager_state.update(cx, |state, _| {
                                state.descending = !state.descending;
                            });
                            cx.notify();
                        })),
                );
                menu
            }
        })
        .into_any_element()
}

fn render_header(cx: &App) -> impl IntoElement {
    h_flex()
        .flex_none()
        .h(px(32.))
        .px_3()
        .items_center()
        .bg(cx.theme().tab_bar)
        .text_size(rems(0.68))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(div().flex_1().child(t!("name").to_string()))
        .child(div().w(px(190.)).child(t!("host").to_string()))
        .child(div().w(px(64.)).text_center().child(t!("port").to_string()))
        .child(div().w(px(100.)).child(t!("user").to_string()))
}

fn render_rows(
    view: &Entity<TinyShell>,
    state: &ConnectionManagerState,
    nodes: Vec<ConnectionTreeNode>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    nodes
        .into_iter()
        .enumerate()
        .map(|(index, node)| match node {
            ConnectionTreeNode::Group {
                id,
                name,
                depth,
                expanded,
            } => render_group(view, state, id, name, depth, expanded, index, window, cx),
            ConnectionTreeNode::Session {
                id,
                session_id,
                depth,
            } => render_session(view, state, id, session_id, depth, index, window, cx),
            ConnectionTreeNode::DeletedGroup { id, name, depth } => {
                render_deleted_group(view, id, name, depth, index, cx)
            }
            ConnectionTreeNode::DeletedSession { id, session, depth } => {
                render_deleted_session(view, id, *session, depth, index, cx)
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn render_group(
    view: &Entity<TinyShell>,
    state: &ConnectionManagerState,
    id: ConnectionNodeId,
    name: String,
    depth: usize,
    expanded: bool,
    index: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let group_name = match &id {
        ConnectionNodeId::Group(name) => name.clone(),
        _ => return div().into_any_element(),
    };
    let selected = state.selected.as_ref() == Some(&id);
    let node_id = id.clone();
    h_flex()
        .id(("connection-manager-group", index))
        .min_h(px(32.))
        .pl(px(10. + depth as f32 * 16.))
        .pr_3()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .when(selected, |this| this.bg(cx.theme().selection))
        .hover(|this| this.bg(cx.theme().secondary.opacity(0.65)))
        .on_mouse_down(
            MouseButton::Left,
            window.listener_for(view, move |this, _event: &MouseDownEvent, _, cx| {
                this.connection_manager_state.update(cx, |state, _| {
                    state.select(node_id.clone());
                });
                cx.notify();
            }),
        )
        .context_menu(group_context_menu(view, &group_name))
        .child(
            div()
                .id(("connection-manager-group-toggle", index))
                .w(px(18.))
                .h(px(22.))
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    window.listener_for(view, {
                        let group_name = group_name.clone();
                        move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.connection_manager_state.update(cx, |state, _| {
                                state.toggle_group(&group_name);
                            });
                            cx.notify();
                        }
                    }),
                )
                .child(if expanded {
                    Icon::new(IconName::ChevronDown).with_size(Size::Small)
                } else {
                    Icon::new(IconName::ChevronRight).with_size(Size::Small)
                }),
        )
        .child(
            h_flex()
                .flex_1()
                .items_center()
                .gap_2()
                .child(Icon::new(IconName::Folder).with_size(Size::Small))
                .child(
                    div()
                        .flex_1()
                        .text_size(rems(0.7))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name),
                ),
        )
        .child(
            div()
                .text_size(rems(0.68))
                .text_color(cx.theme().muted_foreground)
                .child(if expanded { "−" } else { "+" }),
        )
        .into_any_element()
}

fn group_context_menu(
    view: &Entity<TinyShell>,
    group_name: &str,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + use<> {
    let view = view.clone();
    let group_name = group_name.to_string();
    move |mut menu, window, _| {
        menu = menu
            .item(
                PopupMenuItem::new(t!("overview_new_connection").to_string()).on_click(
                    window.listener_for(&view, {
                        let group_name = group_name.clone();
                        move |this, _, window, cx| {
                            this.connection_group_parent = Some(group_name.clone());
                            dismiss_manager_dialog(this, window, cx);
                            this.open_new_ssh_dialog(window, cx);
                        }
                    }),
                ),
            )
            .item(
                PopupMenuItem::new(t!("connection_group_new_child").to_string()).on_click(
                    window.listener_for(&view, {
                        let group_name = group_name.clone();
                        move |_this, _, window, cx| {
                            open_connection_operation(
                                crate::app::connection_manager::operation_window::ConnectionOperation::EditGroup {
                                    original: None,
                                    parent: Some(group_name.clone()),
                                },
                                window,
                                cx,
                            );
                        }
                    }),
                ),
            )
            .separator()
            .item(PopupMenuItem::new(t!("rename").to_string()).on_click(
                window.listener_for(&view, {
                    let group_name = group_name.clone();
                    move |_this, _, window, cx| {
                        let parent = group_name
                            .rsplit_once('/')
                            .map(|(parent, _)| parent.to_string());
                        open_connection_operation(
                            crate::app::connection_manager::operation_window::ConnectionOperation::EditGroup {
                                original: Some(group_name.clone()),
                                parent,
                            },
                            window,
                            cx,
                        );
                    }
                }),
            ))
            .separator()
            .item(
                PopupMenuItem::new(t!("connection_manager_copy").to_string()).on_click(
                    window.listener_for(&view, {
                        let group_name = group_name.clone();
                        move |this, _, _, cx| {
                            run_manager_action(
                                this,
                                ConnectionManagerAction::CopyGroup {
                                    name: group_name.clone(),
                                },
                                cx,
                            );
                        }
                    }),
                ),
            )
            .item(
                PopupMenuItem::new(t!("connection_manager_cut").to_string()).on_click(
                    window.listener_for(&view, {
                        let group_name = group_name.clone();
                        move |this, _, _, cx| {
                            this.connection_manager_actions
                                .cut_group(group_name.clone());
                            cx.notify();
                        }
                    }),
                ),
            )
            .item(
                PopupMenuItem::new(t!("connection_manager_paste").to_string()).on_click(
                    window.listener_for(&view, {
                        let group_name = group_name.clone();
                        move |this, _, _, cx| {
                            run_manager_action(
                                this,
                                ConnectionManagerAction::Paste {
                                    group: Some(group_name.clone()),
                                },
                                cx,
                            );
                        }
                    }),
                ),
            )
            .item(
                PopupMenuItem::new(t!("connection_group_move_to").to_string()).on_click(
                    window.listener_for(&view, {
                        let group_name = group_name.clone();
                        move |_this, _, window, cx| {
                            open_connection_operation(
                                crate::app::connection_manager::operation_window::ConnectionOperation::MoveGroup {
                                    group: group_name.clone(),
                                },
                                window,
                                cx,
                            );
                        }
                    }),
                ),
            )
            .separator()
            .item(PopupMenuItem::new(t!("delete").to_string()).on_click(
                window.listener_for(&view, {
                    let group_name = group_name.clone();
                    move |this, _, _, cx| {
                        run_manager_action(
                            this,
                            ConnectionManagerAction::DeleteGroup {
                                name: group_name.clone(),
                            },
                            cx,
                        );
                    }
                }),
            ));
        menu
    }
}

#[allow(clippy::too_many_arguments)]
fn render_session(
    view: &Entity<TinyShell>,
    state: &ConnectionManagerState,
    id: ConnectionNodeId,
    session_id: String,
    depth: usize,
    index: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(session) = view.read(cx).config.get(&session_id).cloned() else {
        return div().into_any_element();
    };
    let selected = state.selected.as_ref() == Some(&id);
    let session_for_menu = session.clone();
    render_session_row(
        view,
        id,
        session_for_menu,
        depth,
        index,
        selected,
        window,
        cx,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_session_row(
    view: &Entity<TinyShell>,
    id: ConnectionNodeId,
    session: Session,
    depth: usize,
    index: usize,
    selected: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let session_id = session.id.clone();
    let needs_prompt = session.requires_credential_prompt();
    let session_for_menu = session.clone();
    let node_id = id.clone();
    h_flex()
        .id(("connection-manager-session", index))
        .min_h(px(34.))
        .pl(px(14. + depth as f32 * 16.))
        .pr_3()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .when(selected, |this| this.bg(cx.theme().selection))
        .hover(|this| this.bg(cx.theme().secondary.opacity(0.65)))
        .on_mouse_down(
            MouseButton::Left,
            window.listener_for(view, move |this, event: &MouseDownEvent, window, cx| {
                if event.click_count >= 2 {
                    this.active_dialog = None;
                    this.connect_saved_session(session_id.clone(), window, cx);
                    if needs_prompt {
                        window.defer(cx, |window, _| window.remove_window());
                    } else {
                        window.remove_window();
                    }
                } else {
                    this.connection_manager_state
                        .update(cx, |state, _| state.select(node_id.clone()));
                    cx.notify();
                }
            }),
        )
        .context_menu(session_menu(view, session_for_menu))
        .child(Icon::new(IconName::SquareTerminal).with_size(Size::Small))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(rems(0.74))
                .font_weight(FontWeight::MEDIUM)
                .child(session.name),
        )
        .child(
            div()
                .w(px(190.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(rems(0.7))
                .child(session.host),
        )
        .child(
            div()
                .w(px(64.))
                .text_center()
                .text_size(rems(0.7))
                .child(session.port.to_string()),
        )
        .child(
            div()
                .w(px(100.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(rems(0.7))
                .child(session.user),
        )
        .into_any_element()
}

fn session_menu(
    view: &Entity<TinyShell>,
    session: Session,
) -> impl Fn(PopupMenu, &mut Window, &mut gpui::Context<PopupMenu>) -> PopupMenu + 'static {
    let session_id = session.id.clone();
    let needs_prompt = session.requires_credential_prompt();
    let session_for_edit = session.clone();
    let delete_id = session_id.clone();
    let view = view.clone();
    move |mut menu, window, _| {
        let delete_id = delete_id.clone();
        menu = menu
            .item(
                PopupMenuItem::new(t!("connect").to_string()).on_click(window.listener_for(
                    &view,
                    {
                        let id = session_id.clone();
                        move |this, _, window, cx| {
                            this.active_dialog = None;
                            this.connect_saved_session(id.clone(), window, cx);
                            if needs_prompt {
                                window.defer(cx, |window, _| window.remove_window());
                            } else {
                                window.remove_window();
                            }
                        }
                    },
                )),
            )
            .item(
                PopupMenuItem::new(t!("edit").to_string()).on_click(window.listener_for(&view, {
                    let id = session_for_edit.id.clone();
                    move |this, _, window, cx| {
                        dismiss_manager_dialog(this, window, cx);
                        this.edit_saved_session(id.clone(), window, cx);
                    }
                })),
            )
            .separator()
            .item(
                PopupMenuItem::new(t!("connection_manager_copy").to_string()).on_click(
                    window.listener_for(&view, {
                        let id = session_id.clone();
                        move |this, _, _, cx| {
                            run_manager_action(
                                this,
                                ConnectionManagerAction::CopySession { id: id.clone() },
                                cx,
                            )
                        }
                    }),
                ),
            )
            .item(
                PopupMenuItem::new(t!("connection_manager_cut").to_string()).on_click(
                    window.listener_for(&view, {
                        let id = session_id.clone();
                        move |this, _, _, cx| {
                            this.connection_manager_actions.cut_session(id.clone());
                            cx.notify();
                        }
                    }),
                ),
            )
            .item(
                PopupMenuItem::new(t!("connection_copy_address").to_string()).on_click(
                    window.listener_for(&view, {
                        let id = session_id.clone();
                        move |this, _, _, cx| {
                            if let Some(session) = this.config.get(&id) {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    crate::session::connection_catalog::session_address(session),
                                ));
                            }
                        }
                    }),
                ),
            )
            .item(
                PopupMenuItem::new(t!("connection_group_move_to").to_string()).on_click(
                    window.listener_for(&view, {
                        let id = session_id.clone();
                        let session_name = session_for_edit.name.clone();
                        move |_this, _, window, cx| {
                            open_connection_operation(
                                crate::app::connection_manager::operation_window::ConnectionOperation::MoveSession {
                                    session_id: id.clone(),
                                    session_name: session_name.clone(),
                                },
                                window,
                                cx,
                            );
                        }
                    }),
                ),
            )
            .separator()
            .item(
                PopupMenuItem::new(t!("delete").to_string()).on_click(window.listener_for(
                    &view,
                    move |this, _, _, cx| {
                        run_manager_action(
                            this,
                            ConnectionManagerAction::DeleteSession {
                                id: delete_id.clone(),
                            },
                            cx,
                        )
                    },
                )),
            );
        menu
    }
}

fn render_deleted_group(
    view: &Entity<TinyShell>,
    id: ConnectionNodeId,
    name: String,
    depth: usize,
    index: usize,
    cx: &mut App,
) -> AnyElement {
    let ConnectionNodeId::DeletedGroup(group_name) = id else {
        return div().into_any_element();
    };
    h_flex()
        .id(("connection-manager-deleted-group", index))
        .min_h(px(28.))
        .pl(px(10. + depth as f32 * 16.))
        .pr_3()
        .items_center()
        .gap_2()
        .bg(cx.theme().danger.opacity(0.08))
        .context_menu({
            let view = view.clone();
            move |mut menu, window, _| {
                let restore_name = group_name.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!("connection_restore").to_string()).on_click(
                        window.listener_for(&view, move |this, _, _, cx| {
                            run_manager_action(
                                this,
                                ConnectionManagerAction::RestoreGroup {
                                    name: restore_name.clone(),
                                },
                                cx,
                            )
                        }),
                    ),
                );
                menu
            }
        })
        .child(Icon::new(IconName::Delete).with_size(Size::Small))
        .child(
            div()
                .text_size(rems(0.72))
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("{} ({})", name, t!("quick_connection_deleted"))),
        )
        .into_any_element()
}

fn render_deleted_session(
    view: &Entity<TinyShell>,
    id: ConnectionNodeId,
    session: Session,
    depth: usize,
    index: usize,
    cx: &mut App,
) -> AnyElement {
    let ConnectionNodeId::DeletedSession(id) = id else {
        return div().into_any_element();
    };
    h_flex()
        .id(("connection-manager-deleted-session", index))
        .min_h(px(30.))
        .pl(px(14. + depth as f32 * 16.))
        .pr_3()
        .items_center()
        .gap_2()
        .text_color(cx.theme().muted_foreground)
        .context_menu({
            let view = view.clone();
            move |mut menu, window, _| {
                let restore_id = id.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!("connection_restore").to_string()).on_click(
                        window.listener_for(&view, move |this, _, _, cx| {
                            run_manager_action(
                                this,
                                ConnectionManagerAction::RestoreSession {
                                    id: restore_id.clone(),
                                },
                                cx,
                            )
                        }),
                    ),
                );
                menu
            }
        })
        .child(Icon::new(IconName::Delete).with_size(Size::Small))
        .child(div().flex_1().child(session.name))
        .child(div().w(px(190.)).child(session.host))
        .child(
            div()
                .w(px(64.))
                .text_center()
                .child(session.port.to_string()),
        )
        .child(div().w(px(100.)).child(session.user))
        .into_any_element()
}

fn render_empty_menu(
    mut menu: gpui_component::menu::PopupMenu,
    view: &Entity<TinyShell>,
    window: &mut Window,
) -> gpui_component::menu::PopupMenu {
    menu = menu
        .item(
            PopupMenuItem::new(t!("overview_new_connection").to_string()).on_click(
                window.listener_for(view, |this, _, window, cx| {
                    dismiss_manager_dialog(this, window, cx);
                    this.open_new_ssh_dialog(window, cx);
                }),
            ),
        )
        .item(
            PopupMenuItem::new(t!("connection_group_new").to_string()).on_click(
                window.listener_for(view, |_this, _, window, cx| {
                    open_connection_operation(
                        crate::app::connection_manager::operation_window::ConnectionOperation::EditGroup {
                            original: None,
                            parent: None,
                        },
                        window,
                        cx,
                    );
                }),
            ),
        )
        .separator()
        .item(
            PopupMenuItem::new(t!("connection_manager_paste").to_string()).on_click(
                window.listener_for(view, |this, _, _, cx| {
                    run_manager_action(this, ConnectionManagerAction::Paste { group: None }, cx)
                }),
            ),
        )
        .item(
            PopupMenuItem::new(t!("connection_paste_address").to_string()).on_click(
                window.listener_for(view, |this, _, window, cx| {
                    let Some(address) = cx.read_from_clipboard().and_then(|item| item.text())
                    else {
                        return;
                    };
                    dismiss_manager_dialog(this, window, cx);
                    this.connection_group_parent = None;
                    if let Err(error) = this.open_ssh_address_dialog(&address, window, cx) {
                        this.status = t!(
                            "connection_manager_action_failed",
                            error = error.to_string()
                        )
                        .to_string()
                        .into();
                        cx.notify();
                    }
                }),
            ),
        )
        .separator()
        .item(
            PopupMenuItem::new(t!("connection_archive_import_tiny_shell").to_string()).on_click(
                window.listener_for(view, |this, _, window, cx| {
                    this.open_connection_operation_window(
                        crate::app::connection_manager::operation_window::ConnectionOperation::Archive {
                            path: PathBuf::new(),
                            importing: true,
                        },
                        window,
                        cx,
                    );
                }),
            ),
        )
        .item(
            PopupMenuItem::new(t!("finalshell_import_title").to_string()).on_click(
                window.listener_for(view, |this, _, window, cx| {
                    this.open_finalshell_import_window(window, cx);
                }),
            ),
        );
    menu
}

fn open_connection_operation(
    operation: crate::app::connection_manager::operation_window::ConnectionOperation,
    window: &mut Window,
    cx: &mut Context<TinyShell>,
) {
    let owner = cx.entity();
    window.defer(cx, move |_, cx| {
        crate::app::connection_manager::operation_window::open(owner, operation, cx);
    });
}

fn dismiss_manager_dialog(this: &mut TinyShell, window: &mut Window, cx: &mut Context<TinyShell>) {
    this.active_dialog = None;
    window.close_dialog(cx);
}

fn run_manager_action(
    this: &mut TinyShell,
    action: ConnectionManagerAction,
    cx: &mut Context<TinyShell>,
) {
    let mut staged_config = this.config.clone();
    let mut staged_actions = this.connection_manager_actions.clone();
    let result = staged_actions
        .execute(&mut staged_config, action)
        .and_then(|_| crate::app::config_persistence::save_full(&staged_config));
    match result {
        Ok(()) => {
            this.config = staged_config;
            this.connection_manager_actions = staged_actions;
        }
        Err(error) => {
            tracing::warn!("connection manager action failed: {error:#}");
            this.status = t!(
                "connection_manager_action_failed",
                error = error.to_string()
            )
            .to_string()
            .into();
        }
    }
    cx.notify();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_menus_only_expose_operations_for_the_clicked_scope() {
        assert_eq!(
            menu_items(ConnectionContext::Session, false),
            vec![
                ConnectionMenuItem::Connect,
                ConnectionMenuItem::Edit,
                ConnectionMenuItem::CopyAddress,
                ConnectionMenuItem::Copy,
                ConnectionMenuItem::Cut,
                ConnectionMenuItem::MoveToGroup,
                ConnectionMenuItem::Delete,
            ]
        );
        assert_eq!(
            menu_items(ConnectionContext::Group, false),
            vec![
                ConnectionMenuItem::NewConnection,
                ConnectionMenuItem::NewGroup,
                ConnectionMenuItem::Rename,
                ConnectionMenuItem::Copy,
                ConnectionMenuItem::Cut,
                ConnectionMenuItem::Paste,
                ConnectionMenuItem::MoveToGroup,
                ConnectionMenuItem::Delete,
            ]
        );
        assert_eq!(
            menu_items(ConnectionContext::Empty, false),
            vec![
                ConnectionMenuItem::NewConnection,
                ConnectionMenuItem::NewGroup,
                ConnectionMenuItem::Paste,
                ConnectionMenuItem::PasteAddress,
                ConnectionMenuItem::Import,
            ]
        );
        assert_eq!(
            menu_items(ConnectionContext::Session, true),
            vec![ConnectionMenuItem::Restore]
        );
    }
}
