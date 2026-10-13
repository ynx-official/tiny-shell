//! Each submenu selects a section; launcher entries dispatch without replacing it.
use std::rc::Rc;

use gpui::{
    App, AppContext as _, Axis, ElementId, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, RenderOnce, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Collapsible, IconName, Size,
    group_box::GroupBox,
    input::{Input, InputEvent, InputState},
    resizable::{h_resizable, resizable_panel},
    scroll::ScrollableElement as _,
    setting::RenderOptions,
    sidebar::{Sidebar, SidebarItem, SidebarMenu, SidebarMenuItem},
    v_flex,
};
use rust_i18n::t;

use super::page::{SettingGroup, SettingPage, contains_query};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Selection {
    page_ix: usize,
    group_ix: usize,
}

fn resolve_selection(pages: &[SettingPage], selected: Selection, query: &str) -> Option<Selection> {
    if let Some(page) = pages.get(selected.page_ix) {
        if page.groups.get(selected.group_ix).is_some_and(|group| {
            group.action.is_none() && page.group_matches(selected.group_ix, query)
        }) {
            return Some(selected);
        }
        if let Some(group_ix) = page.first_content_group(query) {
            return Some(Selection {
                group_ix,
                ..selected
            });
        }
    }
    pages.iter().enumerate().find_map(|(page_ix, page)| {
        page.first_content_group(query)
            .map(|group_ix| Selection { page_ix, group_ix })
    })
}

struct NavigationState {
    selected: Selection,
    request: u64,
    search: Entity<InputState>,
    search_scroll: ScrollHandle,
    _search_subscription: Subscription,
}

// Filtered positions must not become the identity of a category's open state.
#[derive(Clone)]
struct CategoryMenu {
    id: SharedString,
    menu: SidebarMenu,
}

impl Collapsible for CategoryMenu {
    fn is_collapsed(&self) -> bool {
        self.menu.is_collapsed()
    }

    fn collapsed(mut self, collapsed: bool) -> Self {
        self.menu = self.menu.collapsed(collapsed);
        self
    }
}

impl SidebarItem for CategoryMenu {
    fn render(
        self,
        _: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        self.menu.render(self.id, window, cx).into_any_element()
    }
}

#[derive(IntoElement)]
pub(super) struct Settings {
    id: SharedString,
    pages: Vec<SettingPage>,
    initial_page: usize,
    request: u64,
    on_navigate: Rc<dyn Fn(&mut App)>,
}

impl Settings {
    pub(super) fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            pages: Vec::new(),
            initial_page: 0,
            request: 0,
            on_navigate: Rc::new(|_| {}),
        }
    }

    pub(super) fn requested_page(mut self, page: usize, request: u64) -> Self {
        self.initial_page = page;
        self.request = request;
        self
    }

    pub(super) fn on_navigate(mut self, callback: impl Fn(&mut App) + 'static) -> Self {
        self.on_navigate = Rc::new(callback);
        self
    }

    pub(super) fn page(mut self, page: SettingPage) -> Self {
        self.pages.push(page);
        self
    }

    fn render_sidebar(
        &self,
        state: &Entity<NavigationState>,
        selected: Option<Selection>,
        query: &str,
        cx: &App,
    ) -> Sidebar<CategoryMenu> {
        Sidebar::new(format!("{}-sidebar", self.id))
            .w_full()
            .min_w_0()
            .bg(cx.theme().background)
            .collapsible(false)
            .header(Input::new(&state.read(cx).search).prefix(IconName::Search))
            .children(self.pages.iter().enumerate().filter_map(|(page_ix, page)| {
                if !page
                    .groups
                    .iter()
                    .enumerate()
                    .any(|(ix, _)| page.group_matches(ix, query))
                {
                    return None;
                }
                let default_group = page.first_content_group(query);
                let active = selected.is_some_and(|index| index.page_ix == page_ix);
                let state_for_page = state.clone();
                let on_navigate = self.on_navigate.clone();
                let mut item = SidebarMenuItem::new(page.title.clone())
                    .click_to_open(true)
                    .default_open(page.default_open || active || !query.is_empty())
                    .active(active && page.groups.len() == 1)
                    .on_click(move |_, _, cx| {
                        if let Some(group_ix) = default_group {
                            on_navigate(cx);
                            state_for_page.update(cx, |state, cx| {
                                state.selected = Selection { page_ix, group_ix };
                                cx.notify();
                            });
                        }
                    });
                if let Some(icon) = &page.icon {
                    item = item.icon(icon.clone());
                }
                if page.groups.len() > 1 {
                    item = item.children(page.groups.iter().enumerate().filter_map(
                        |(group_ix, group)| {
                            let title = group.title.as_ref()?;
                            if !page.group_matches(group_ix, query) {
                                return None;
                            }
                            let index = Selection { page_ix, group_ix };
                            let state = state.clone();
                            let action = group.action.clone();
                            let on_navigate = self.on_navigate.clone();
                            Some(
                                SidebarMenuItem::new(title.clone())
                                    .active(selected == Some(index))
                                    .on_click(move |_, window, cx| {
                                        on_navigate(cx);
                                        if let Some(action) = &action {
                                            action(window, cx);
                                        } else {
                                            state.update(cx, |state, cx| {
                                                state.selected = index;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                        },
                    ));
                }
                Some(CategoryMenu {
                    id: format!("{}-category-{page_ix}-{}", self.id, query.is_empty()).into(),
                    menu: SidebarMenu::new().child(item),
                })
            }))
    }

    fn render_content(
        &self,
        selected: Option<Selection>,
        query: &str,
        search_scroll: &ScrollHandle,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::AnyElement {
        let Some(index) = selected else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(t!("no_results").to_string())
                .into_any_element();
        };
        let page = &self.pages[index.page_ix];
        let group = &page.groups[index.group_ix];
        let scroll_id = format!("{}-section-{}-{}", self.id, index.page_ix, index.group_ix);
        let scroll = if query.is_empty() {
            window
                .use_keyed_state(SharedString::from(scroll_id.clone()), cx, |_, _| {
                    ScrollHandle::new()
                })
                .read(cx)
                .clone()
        } else {
            search_scroll.clone()
        };
        let item_query = if contains_query(&page.title, query) {
            ""
        } else {
            query
        };
        let groups = page
            .content_groups(index.group_ix, query)
            .into_iter()
            .map(|group_ix| {
                let group = &page.groups[group_ix];
                render_group(group, index.page_ix, group_ix, item_query, window, cx)
            })
            .collect::<Vec<_>>();
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(
                v_flex()
                    .p_4()
                    .gap_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .when(
                        group
                            .title
                            .as_ref()
                            .is_some_and(|title| title != &page.title),
                        |this| {
                            this.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(page.title.clone()),
                            )
                        },
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(group.title.clone().unwrap_or_else(|| page.title.clone())),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        v_flex()
                            .id(SharedString::from(scroll_id))
                            .size_full()
                            .min_w_0()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .p_4()
                            .gap_4()
                            .children(groups),
                    )
                    .vertical_scrollbar(&scroll),
            )
            .into_any_element()
    }
}

fn render_group(
    group: &SettingGroup,
    page_ix: usize,
    group_ix: usize,
    query: &str,
    window: &mut Window,
    cx: &mut App,
) -> GroupBox {
    GroupBox::new()
        .id(SharedString::from(format!(
            "settings-group-{page_ix}-{group_ix}"
        )))
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .gap_4()
        .children(group.visible_items(query).map(|(item_ix, item)| {
            item.render_item(
                &RenderOptions {
                    page_ix,
                    group_ix,
                    item_ix,
                    size: Size::default(),
                    group_variant: Default::default(),
                    layout: Axis::Horizontal,
                    disabled: false,
                },
                window,
                cx,
            )
        }))
}

impl RenderOnce for Settings {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let initial_group = self
            .pages
            .get(self.initial_page)
            .and_then(|page| page.first_content_group(""))
            .unwrap_or(0);
        let initial = resolve_selection(
            &self.pages,
            Selection {
                page_ix: self.initial_page,
                group_ix: initial_group,
            },
            "",
        )
        .unwrap_or_default();
        let state = window.use_keyed_state(self.id.clone(), cx, |window, cx| {
            let search = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("settings_search_placeholder").to_string())
            });
            let subscription = cx.subscribe(
                &search,
                |state: &mut NavigationState, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        state.search_scroll.set_offset(gpui::point(px(0.), px(0.)));
                        cx.notify();
                    }
                },
            );
            NavigationState {
                selected: initial,
                request: self.request,
                search,
                search_scroll: ScrollHandle::new(),
                _search_subscription: subscription,
            }
        });
        // Explicit requests must work even when the destination is requested twice.
        if state.read(cx).request != self.request {
            let search = state.read(cx).search.clone();
            search.update(cx, |search, cx| search.set_value("", window, cx));
            state.update(cx, |state, _| {
                state.selected = initial;
                state.request = self.request;
            });
        }
        let query = state.read(cx).search.read(cx).value().trim().to_string();
        let selected = resolve_selection(&self.pages, state.read(cx).selected, &query);
        let search_scroll = state.read(cx).search_scroll.clone();
        h_resizable(format!("{}-layout", self.id))
            .child(
                resizable_panel()
                    .size(px(180.))
                    .child(self.render_sidebar(&state, selected, &query, cx)),
            )
            .child(resizable_panel().child(self.render_content(
                selected,
                &query,
                &search_scroll,
                window,
                cx,
            )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages() -> Vec<SettingPage> {
        vec![
            SettingPage::new("Terminal")
                .group(SettingGroup::new().title("Font"))
                .group(SettingGroup::new().title("Behavior")),
            SettingPage::new("AI")
                .group(SettingGroup::new().title("Providers"))
                .group(SettingGroup::new().title("Budget"))
                .group(SettingGroup::new().title("Audit").on_open(|_, _| {}))
                .group(SettingGroup::new().title("Follow-ups")),
        ]
    }

    #[test]
    fn selecting_a_submenu_displays_only_that_group() {
        let pages = pages();
        let selected = Selection {
            page_ix: 0,
            group_ix: 1,
        };
        assert_eq!(pages[0].content_groups(selected.group_ix, ""), vec![1]);
        assert_eq!(pages[1].content_groups(1, ""), vec![1]);
    }

    #[test]
    fn category_entry_opens_the_first_content_group() {
        let pages = pages();
        assert_eq!(pages[1].first_content_group(""), Some(0));
        assert_eq!(
            resolve_selection(
                &pages,
                Selection {
                    page_ix: 1,
                    group_ix: 99
                },
                ""
            ),
            Some(Selection {
                page_ix: 1,
                group_ix: 0
            }),
        );
    }

    #[test]
    fn search_keeps_original_page_and_group_indices() {
        let pages = pages();
        assert_eq!(
            resolve_selection(&pages, Selection::default(), "follow"),
            Some(Selection {
                page_ix: 1,
                group_ix: 3
            }),
        );
        assert_eq!(
            resolve_selection(&pages, Selection::default(), "missing"),
            None,
        );
    }

    #[test]
    fn audit_is_an_action_and_cannot_become_inline_content() {
        let pages = pages();
        assert!(pages[1].groups[2].action.is_some());
        assert!(pages[1].content_groups(2, "").is_empty());
        assert_eq!(
            resolve_selection(&pages, Selection::default(), "audit"),
            None
        );
    }

    #[test]
    fn untitled_about_context_remains_with_its_selected_section() {
        let page = SettingPage::new("About")
            .group(SettingGroup::new())
            .group(SettingGroup::new().title("Version"))
            .group(SettingGroup::new());
        assert_eq!(page.first_content_group(""), Some(1));
        assert_eq!(page.content_groups(1, ""), vec![0, 1, 2]);
    }

    #[test]
    fn search_by_item_label_shows_only_matching_controls() {
        use super::super::page::{SettingField, SettingItem};
        let page = SettingPage::new("Terminal")
            .group(
                SettingGroup::new()
                    .title("Font")
                    .item(SettingItem::new(
                        "Font size",
                        SettingField::render(|_, _, _| div()),
                    ))
                    .item(SettingItem::new(
                        "Line height",
                        SettingField::render(|_, _, _| div()),
                    )),
            )
            .group(SettingGroup::new().title("Behavior"));
        assert_eq!(page.first_content_group("LINE"), Some(0));
        assert_eq!(
            page.groups[0]
                .visible_items("LINE")
                .map(|(ix, _)| ix)
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(page.groups[0].visible_items("font").count(), 2);
    }
}
