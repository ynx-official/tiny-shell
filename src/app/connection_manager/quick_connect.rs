use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EntityInputHandler as _, Focusable as _,
    FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    popover::{Popover, PopoverState},
    scroll::{Scrollbar, ScrollbarAxis, ScrollbarShow},
    v_flex,
};
use rust_i18n::t;

use super::state::{ConnectionManagerState, ConnectionNodeId, ConnectionTreeNode};
use crate::TinyShell;

fn row_height(node: &ConnectionTreeNode) -> f32 {
    if matches!(node, ConnectionTreeNode::Group { .. }) {
        32.
    } else {
        48.
    }
}

fn list_height(nodes: &[ConnectionTreeNode], viewport_height: f32) -> f32 {
    let content = nodes.iter().map(row_height).sum::<f32>() + 8.;
    // Reserve header/search/footer and room below the tab bar, including on small windows.
    content
        .max(96.)
        .min((viewport_height - 204.).clamp(96., 320.))
}

fn key_hint(key: &'static str, cx: &App) -> AnyElement {
    div()
        .flex_none()
        .px_1()
        .h(px(20.))
        .min_w(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .line_height(px(16.))
        .rounded(px(4.))
        .border_1()
        .border_color(cx.theme().border)
        .text_color(cx.theme().muted_foreground)
        .child(key)
        .into_any_element()
}

/// This picker owns only transient UI state. Connection data and opening stay with its owner.
struct QuickConnect {
    owner: Entity<TinyShell>,
    input: Entity<InputState>,
    tree: ConnectionManagerState,
    scroll: ScrollHandle,
    popover: Option<WeakEntity<PopoverState>>,
    _search_subscription: Subscription,
}

impl QuickConnect {
    fn new(owner: Entity<TinyShell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            crate::app::localization::localized_input(window, cx, || {
                t!("quick_connection_search").to_string()
            })
        });
        let search = cx.subscribe(&input, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.tree.set_query(input.read(cx).value().to_string());
                this.tree.selected = None;
                this.scroll.scroll_to_item(0);
                cx.notify();
            }
        });
        // use_keyed_state already forwards picker notifications to its owning view.
        // Observing that owner here would create an endless owner -> picker -> owner loop,
        // even while the popover is closed. This uncached child renders with its parent.
        Self {
            owner,
            input,
            tree: ConnectionManagerState::default(),
            scroll: ScrollHandle::new(),
            popover: None,
            _search_subscription: search,
        }
    }

    fn dismiss(&self, window: &mut Window, cx: &mut App) {
        if let Some(popover) = self.popover.as_ref().and_then(WeakEntity::upgrade) {
            popover.update(cx, |state, cx| state.dismiss(window, cx));
        }
    }

    fn activate(&mut self, id: ConnectionNodeId, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            ConnectionNodeId::Group(group) => {
                self.tree.toggle_group(&group);
                self.tree.selected = Some(ConnectionNodeId::Group(group));
                cx.notify();
            }
            ConnectionNodeId::Session(id) => {
                // Dismiss first so focus restoration cannot steal focus from a new tab or prompt.
                self.dismiss(window, cx);
                self.owner
                    .update(cx, |owner, cx| owner.connect_saved_session(id, window, cx));
            }
            _ => {}
        }
    }
}

impl Render for QuickConnect {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let nodes = self.tree.visible_nodes(&self.owner.read(cx).config);
        let count = self.owner.read(cx).config.sessions().len();
        let width = (f32::from(window.viewport_size().width) - 40.).clamp(240., 400.);
        let height = list_height(&nodes, f32::from(window.viewport_size().height));
        let search_focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let rows = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let id = node.id().clone();
                let selected = self.tree.selected.as_ref() == Some(&id);
                let is_group = matches!(node, ConnectionTreeNode::Group { .. });
                let (icon, title, detail) = match node {
                    ConnectionTreeNode::Group { name, expanded, .. } => (
                        if *expanded {
                            IconName::FolderOpen
                        } else {
                            IconName::Folder
                        },
                        name.clone(),
                        None,
                    ),
                    ConnectionTreeNode::Session { session_id, .. } => {
                        let session = self.owner.read(cx).config.get(session_id)?;
                        (
                            IconName::SquareTerminal,
                            session.name.clone(),
                            Some(format!(
                                "{}@{}:{}",
                                session.user, session.host, session.port
                            )),
                        )
                    }
                    _ => return None,
                };
                Some(
                    h_flex()
                        .id(("quick-connect-row", index))
                        .w_full()
                        .h(px(row_height(node)))
                        .flex_none()
                        .gap_2()
                        .pl(px(4. + node.depth().min(8) as f32 * 16.))
                        .pr_2()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .when(selected, |row| row.bg(cx.theme().selection))
                        .hover(|row| row.bg(cx.theme().secondary))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.activate(id.clone(), window, cx)
                        }))
                        .child(div().w(px(12.)).flex_none().when(is_group, |slot| {
                            let expanded =
                                matches!(node, ConnectionTreeNode::Group { expanded: true, .. });
                            slot.child(
                                Icon::new(if expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(px(12.))
                                .text_color(cx.theme().muted_foreground),
                            )
                        }))
                        .child(
                            Icon::new(icon)
                                .size(px(16.))
                                .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_ellipsis()
                                        .child(title),
                                )
                                .when_some(detail, |row, detail| {
                                    row.child(
                                        div()
                                            .text_size(px(11.))
                                            .line_height(px(16.))
                                            .text_color(cx.theme().muted_foreground)
                                            .text_ellipsis()
                                            .child(detail),
                                    )
                                }),
                        ),
                )
            })
            .collect::<Vec<_>>();

        v_flex()
            .w(px(width))
            .overflow_hidden()
            .text_size(px(13.))
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(px(8.))
            .shadow_md()
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if !matches!(key, "up" | "down" | "enter" | "escape") {
                    return;
                }
                // Leave IME confirmation to the input instead of opening a connection.
                if this.input.update(cx, |input, cx| {
                    input.marked_text_range(window, cx).is_some()
                }) {
                    return;
                }
                window.prevent_default();
                cx.stop_propagation();
                match key {
                    "escape" => this.dismiss(window, cx),
                    "up" | "down" => {
                        let nodes = this.tree.visible_nodes(&this.owner.read(cx).config);
                        if let Some(index) = this.tree.select_relative(&nodes, key == "up") {
                            this.scroll.scroll_to_item(index);
                        }
                        cx.notify();
                    }
                    "enter" => {
                        let nodes = this.tree.visible_nodes(&this.owner.read(cx).config);
                        let target = nodes
                            .iter()
                            .find(|node| Some(node.id()) == this.tree.selected.as_ref())
                            .or_else(|| {
                                nodes
                                    .iter()
                                    .find(|node| matches!(node, ConnectionTreeNode::Session { .. }))
                            });
                        if let Some(node) = target {
                            this.activate(node.id().clone(), window, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .child(
                h_flex()
                    .h(px(44.))
                    .px_3()
                    .gap_2()
                    .justify_between()
                    .flex_none()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("quick_connection_title").to_string()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("quick_connection_total", count = count).to_string()),
                            ),
                    )
                    .child(key_hint("Esc", cx)),
            )
            .child(
                div().px_3().pb_2().flex_none().child(
                    Input::new(&self.input)
                        // Input::h only affects multiline inputs; constrain the single-line box.
                        .min_h(px(36.))
                        .max_h(px(36.))
                        .focus_bordered(false)
                        .border_color(if search_focused {
                            cx.theme().primary.opacity(0.5)
                        } else {
                            cx.theme().border
                        })
                        .bg(cx.theme().muted.opacity(0.25))
                        .text_size(px(13.))
                        .rounded(px(6.))
                        .prefix(
                            Icon::new(IconName::Search)
                                .size(px(16.))
                                .text_color(cx.theme().muted_foreground),
                        )
                        .cleanable(true),
                ),
            )
            .child(
                div()
                    .relative()
                    .h(px(height))
                    .flex_none()
                    .child(
                        v_flex()
                            .id("quick-connect-list")
                            .size_full()
                            .px_2()
                            .py_1()
                            .track_scroll(&self.scroll)
                            .overflow_y_scroll()
                            .children(rows)
                            .when(nodes.is_empty(), |list| {
                                list.child(
                                    v_flex()
                                        .size_full()
                                        .items_center()
                                        .justify_center()
                                        .gap_2()
                                        .text_size(px(12.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(Icon::new(IconName::Search).size(px(20.)))
                                        .child(t!("quick_connection_empty").to_string()),
                                )
                            }),
                    )
                    .child(
                        div().absolute().top_0().right_0().bottom_0().child(
                            Scrollbar::new(&self.scroll)
                                .axis(ScrollbarAxis::Vertical)
                                .scrollbar_show(ScrollbarShow::Scrolling),
                        ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .h(px(40.))
                    .px_3()
                    .justify_between()
                    .gap_2()
                    .bg(cx.theme().muted.opacity(0.35))
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .gap_1()
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(key_hint("↑↓", cx))
                            .child(t!("quick_connection_navigate").to_string())
                            .child(div().w(px(4.)))
                            .child(key_hint("↵", cx))
                            .child(t!("connect").to_string()),
                    )
                    .child(
                        Button::new("quick-connect-manage")
                            .ghost()
                            .small()
                            .text_size(px(12.))
                            .label(t!("quick_connection_manage").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss(window, cx);
                                this.owner.update(cx, |owner, cx| {
                                    owner.show_quick_connection_manager_dialog(window, cx);
                                });
                            })),
                    ),
            )
    }
}

pub(crate) fn trigger(owner: Entity<TinyShell>, window: &mut Window, cx: &mut App) -> Popover {
    let picker = window.use_keyed_state("quick-connect-picker", cx, |window, cx| {
        QuickConnect::new(owner, window, cx)
    });
    let focus = picker.read(cx).input.read(cx).focus_handle(cx);
    Popover::new("tab-quick-connect-popover")
        .appearance(false)
        .anchor(gpui::Anchor::TopLeft)
        .mt(px(8.))
        .track_focus(&focus)
        .trigger(
            Button::new("tab-quick-connections")
                .ghost()
                .small()
                .rounded(px(6.))
                .icon(IconName::FolderOpen)
                .tooltip(t!("quick_connection_title").to_string()),
        )
        .on_open_change({
            let picker = picker.clone();
            move |open, window, cx| {
                if *open {
                    picker.update(cx, |this, cx| {
                        this.tree.set_query(String::new());
                        this.tree.selected = None;
                        this.input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        this.scroll.scroll_to_item(0);
                        cx.notify();
                    });
                }
            }
        })
        .content(move |_, _, cx| {
            let popover = cx.entity().downgrade();
            picker.update(cx, |this, _| this.popover = Some(popover));
            picker.clone()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> ConnectionTreeNode {
        ConnectionTreeNode::Session {
            id: ConnectionNodeId::Session("example".into()),
            session_id: "example".into(),
            depth: 0,
        }
    }

    #[test]
    fn short_results_shrink_but_long_results_scroll() {
        assert_eq!(list_height(&[], 700.), 96.);
        assert_eq!(list_height(&[connection(), connection()], 700.), 104.);
        assert_eq!(list_height(&vec![connection(); 30], 700.), 320.);
        assert!(list_height(&vec![connection(); 30], 360.) < 200.);
    }

    #[test]
    fn groups_are_more_compact_than_connection_details() {
        let group = ConnectionTreeNode::Group {
            id: ConnectionNodeId::Group("example".into()),
            name: "example".into(),
            depth: 0,
            expanded: false,
        };
        assert!(row_height(&group) < row_height(&connection()));
    }
}
