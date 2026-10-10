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

const HEADER_HEIGHT: f32 = 48.;
const SEARCH_HEIGHT: f32 = 38.;
const SEARCH_BOTTOM_SPACING: f32 = 12.;
const COLUMN_HEADER_HEIGHT: f32 = 28.;
const FOOTER_HEIGHT: f32 = 40.;
const HORIZONTAL_PADDING: f32 = 16.;
const LIST_VERTICAL_PADDING: f32 = 8.;
const POPOVER_CHROME_HEIGHT: f32 = HEADER_HEIGHT
    + SEARCH_HEIGHT
    + SEARCH_BOTTOM_SPACING
    + COLUMN_HEADER_HEIGHT
    + FOOTER_HEIGHT
    + 2.;
const WINDOW_BOTTOM_MARGIN: f32 = 12.;

fn row_height(_node: &ConnectionTreeNode) -> f32 {
    32.
}

fn row_spacing(node: &ConnectionTreeNode, index: usize) -> f32 {
    if index > 0 && matches!(node, ConnectionTreeNode::Group { depth: 0, .. }) {
        4.
    } else {
        0.
    }
}

fn list_height(nodes: &[ConnectionTreeNode], viewport_height: f32, popover_top: f32) -> f32 {
    let content = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| row_height(node) + row_spacing(node, index))
        .sum::<f32>()
        + LIST_VERTICAL_PADDING * 2.;
    // Grow downwards from the tab bar, reserving all fixed chrome before enabling scrolling.
    let available =
        (viewport_height - popover_top - POPOVER_CHROME_HEIGHT - WINDOW_BOTTOM_MARGIN).max(0.);
    content.max(96.).min(available)
}

fn popover_width(viewport_width: f32) -> f32 {
    (viewport_width * 0.44)
        .clamp(600., 760.)
        .min((viewport_width - 16.).max(0.))
}

struct ConnectionColumns {
    name: f32,
    address: f32,
    user: f32,
}

impl ConnectionColumns {
    fn new(popover_width: f32) -> Self {
        // Match the list/header gutters and outer border. Indentation stays in Name.
        let width = (popover_width - HORIZONTAL_PADDING * 2. - 2.).max(0.);
        Self {
            name: width * 0.47,
            address: width * 0.36,
            user: width * 0.17,
        }
    }
}

fn column_cell(value: String, width: f32) -> gpui::Div {
    div()
        .w(px(width))
        .flex_none()
        .px_1()
        .overflow_hidden()
        .text_ellipsis()
        .child(value)
}

fn connection_address(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
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
                t!("quick_connection_search_hint").to_string()
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
        let width = popover_width(f32::from(window.viewport_size().width));
        let popover_top = self
            .owner
            .read(cx)
            .tab_bar_bounds
            .map(|bounds| f32::from(bounds.bottom()) + 8.)
            .unwrap_or(48.);
        let height = list_height(
            &nodes,
            f32::from(window.viewport_size().height),
            popover_top,
        );
        let columns = ConnectionColumns::new(width);
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
                            Some((
                                connection_address(&session.host, session.port),
                                session.user.clone(),
                            )),
                        )
                    }
                    _ => return None,
                };
                Some(
                    h_flex()
                        .id(("quick-connect-row", index))
                        .w(px(columns.name + columns.address + columns.user))
                        .h(px(row_height(node)))
                        .mt(px(row_spacing(node, index)))
                        .flex_none()
                        .rounded(px(4.))
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .cursor_pointer()
                        .when(selected, |row| row.bg(cx.theme().selection))
                        .hover(|row| {
                            row.bg(if selected {
                                cx.theme().selection
                            } else {
                                cx.theme().secondary
                            })
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.activate(id.clone(), window, cx)
                        }))
                        .tooltip({
                            let label = if let Some((address, user)) = &detail {
                                format!("{title} — {user}@{address}")
                            } else {
                                title.clone()
                            };
                            move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(label.clone())
                                    .build(window, cx)
                            }
                        })
                        .child(
                            h_flex()
                                .w(px(columns.name))
                                .flex_none()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .gap(px(6.))
                                .pl(px(4. + node.depth().min(8) as f32 * 16.))
                                .pr(px(8.))
                                .child(div().w(px(12.)).flex_none().when(is_group, |slot| {
                                    let expanded = matches!(
                                        node,
                                        ConnectionTreeNode::Group { expanded: true, .. }
                                    );
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
                                .child(Icon::new(icon).size(px(16.)).text_color(
                                    if selected && !is_group {
                                        cx.theme().link
                                    } else {
                                        cx.theme().muted_foreground
                                    },
                                ))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .overflow_hidden()
                                        .when(is_group, |label| {
                                            label.font_weight(FontWeight::MEDIUM)
                                        })
                                        .text_ellipsis()
                                        .child(title),
                                ),
                        )
                        .when_some(detail, |row, (address, user)| {
                            row.child(
                                column_cell(address, columns.address)
                                    .text_size(px(12.))
                                    .font_family(cx.theme().mono_font_family.clone()),
                            )
                            .child(column_cell(user, columns.user))
                        }),
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
                    .h(px(HEADER_HEIGHT))
                    .px(px(HORIZONTAL_PADDING))
                    .gap_2()
                    .justify_between()
                    .flex_none()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("quick_connection_title").to_string()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("quick_connection_total", count = count).to_string()),
                            ),
                    )
                    .child(key_hint("Esc", cx)),
            )
            .child(
                div()
                    .px(px(HORIZONTAL_PADDING))
                    .pb(px(SEARCH_BOTTOM_SPACING))
                    .flex_none()
                    .child(
                        Input::new(&self.input)
                            // Input::h only affects multiline inputs; constrain the single-line box.
                            .min_h(px(SEARCH_HEIGHT))
                            .max_h(px(SEARCH_HEIGHT))
                            .focus_bordered(false)
                            .border_color(if search_focused {
                                cx.theme().primary.opacity(0.5)
                            } else {
                                cx.theme().border
                            })
                            .bg(cx.theme().popover)
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
                h_flex()
                    .h(px(COLUMN_HEADER_HEIGHT))
                    .mx(px(HORIZONTAL_PADDING))
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .bg(cx.theme().muted.opacity(0.35))
                    .rounded(px(4.))
                    .child(column_cell(
                        t!("quick_connection_name").to_string(),
                        columns.name,
                    ))
                    .child(column_cell(
                        t!("quick_connection_address").to_string(),
                        columns.address,
                    ))
                    .child(column_cell(
                        t!("quick_connection_user").to_string(),
                        columns.user,
                    )),
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
                            .px(px(HORIZONTAL_PADDING))
                            .py(px(LIST_VERTICAL_PADDING))
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
                        div().absolute().top_0().left_0().size_full().child(
                            Scrollbar::new(&self.scroll)
                                .axis(ScrollbarAxis::Vertical)
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .h(px(FOOTER_HEIGHT))
                    .px(px(HORIZONTAL_PADDING))
                    .justify_between()
                    .gap_2()
                    .bg(cx.theme().muted.opacity(0.35))
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .gap_1()
                            .text_size(px(12.))
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
        assert_eq!(list_height(&[], 700., 40.), 96.);
        assert_eq!(list_height(&[connection(), connection()], 700., 40.), 96.);
        assert_eq!(list_height(&vec![connection(); 20], 900., 40.), 656.);
        assert!(list_height(&vec![connection(); 30], 700., 40.) > 320.);
        assert!(list_height(&vec![connection(); 30], 360., 40.) < 200.);
    }

    #[test]
    fn groups_and_connections_use_the_same_comfortable_row_height() {
        let group = ConnectionTreeNode::Group {
            id: ConnectionNodeId::Group("example".into()),
            name: "example".into(),
            depth: 0,
            expanded: false,
        };
        assert_eq!(row_height(&group), 32.);
        assert_eq!(row_height(&connection()), 32.);
    }

    #[test]
    fn top_level_group_spacing_counts_toward_list_height() {
        let group = |depth| ConnectionTreeNode::Group {
            id: ConnectionNodeId::Group("example".into()),
            name: "example".into(),
            depth,
            expanded: true,
        };
        let nodes = [group(0), group(1), connection(), group(0)];
        assert_eq!(list_height(&nodes, 900., 40.), 148.);
    }

    #[test]
    fn expanded_list_reserves_footer_and_tracks_window_resize() {
        let nodes = vec![connection(); 100];
        for (viewport, top) in [(900., 40.), (700., 80.), (360., 40.), (240., 60.)] {
            assert_eq!(
                list_height(&nodes, viewport, top)
                    + top
                    + POPOVER_CHROME_HEIGHT
                    + WINDOW_BOTTOM_MARGIN,
                viewport
            );
        }
        assert_eq!(list_height(&nodes, 180., 60.), 0.);
        assert!(list_height(&nodes[..8], 900., 40.) < list_height(&nodes[..20], 900., 40.));
    }

    #[test]
    fn table_width_matches_reference_proportion_and_fits_small_windows() {
        assert!((popover_width(1527.) / 1527. - 0.44).abs() < 0.01);
        assert!(popover_width(1246.) > 520.);
        assert!(popover_width(480.) <= 464.);
        let columns = ConnectionColumns::new(660.);
        assert!((columns.name + columns.address + columns.user + 34. - 660.).abs() < 0.01);
    }

    #[test]
    fn combined_address_keeps_ports_unambiguous() {
        assert_eq!(
            connection_address("server.example", 2200),
            "server.example:2200"
        );
        assert_eq!(connection_address("192.0.2.1", 22), "192.0.2.1:22");
        assert_eq!(connection_address("2001:db8::1", 22), "[2001:db8::1]:22");
        assert_eq!(connection_address("[2001:db8::1]", 22), "[2001:db8::1]:22");
    }
}
