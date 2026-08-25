use std::{collections::HashMap, collections::HashSet, sync::Arc};

use gpui::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px, rems,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, Size, h_flex, scroll::ScrollableElement as _,
    v_flex,
};

type SelectHandler = Arc<dyn Fn(Option<String>, &mut Window, &mut App) + Send + Sync>;
type ToggleHandler = Arc<dyn Fn(String, &mut Window, &mut App) + Send + Sync>;

#[derive(Clone, Copy)]
enum PickerLayout {
    Fill,
    Popover { width: Pixels, height: Pixels },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupTreeRow {
    path: String,
    depth: usize,
    has_children: bool,
    expanded: bool,
}

/// Compact, scrollable tree picker for slash-delimited connection groups.
///
/// The picker owns presentation and hit targets while callers retain business
/// state, making the same component usable in popovers and full-size dialogs.
#[derive(IntoElement)]
pub(crate) struct GroupTreePicker {
    id: SharedString,
    groups: Vec<String>,
    expanded: HashSet<String>,
    selected: Option<String>,
    root_label: SharedString,
    root_muted: bool,
    show_selection: bool,
    layout: PickerLayout,
    on_select: Option<SelectHandler>,
    on_toggle: Option<ToggleHandler>,
}

impl GroupTreePicker {
    pub(crate) fn new(
        id: impl Into<SharedString>,
        groups: Vec<String>,
        expanded: HashSet<String>,
        root_label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            groups,
            expanded,
            selected: None,
            root_label: root_label.into(),
            root_muted: true,
            show_selection: true,
            layout: PickerLayout::Fill,
            on_select: None,
            on_toggle: None,
        }
    }

    pub(crate) fn selected(mut self, selected: Option<String>) -> Self {
        self.selected = selected;
        self
    }

    pub(crate) fn show_selection(mut self, show_selection: bool) -> Self {
        self.show_selection = show_selection;
        self
    }

    pub(crate) fn root_muted(mut self, root_muted: bool) -> Self {
        self.root_muted = root_muted;
        self
    }

    pub(crate) fn popover(mut self, width: Pixels, height: Pixels) -> Self {
        self.layout = PickerLayout::Popover { width, height };
        self
    }

    pub(crate) fn on_select(
        mut self,
        handler: impl Fn(Option<String>, &mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        self.on_select = Some(Arc::new(handler));
        self
    }

    pub(crate) fn on_toggle(
        mut self,
        handler: impl Fn(String, &mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        self.on_toggle = Some(Arc::new(handler));
        self
    }
}

impl RenderOnce for GroupTreePicker {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let rows = group_tree_rows(&self.groups, &self.expanded);
        let id_prefix = self.id.clone();
        let root_selected = self.show_selection && self.selected.is_none();
        let root_muted = self.root_muted;
        let root_handler = self.on_select.clone();
        let selected = self.selected;
        let show_selection = self.show_selection;
        let on_select = self.on_select;
        let on_toggle = self.on_toggle;

        let picker = v_flex()
            .id(self.id)
            .overflow_hidden()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .p_1()
            .child(
                h_flex()
                    .id(SharedString::from(format!("{id_prefix}-root")))
                    .h(px(32.))
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .pl(px(10.))
                    .pr_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_size(rems(0.78))
                    .when(root_muted, |this| {
                        this.text_color(cx.theme().muted_foreground)
                    })
                    .when(root_selected, |this| {
                        this.bg(cx.theme().selection.opacity(0.72))
                            .text_color(cx.theme().foreground)
                    })
                    .hover(|this| this.bg(cx.theme().secondary.opacity(0.65)))
                    .when_some(root_handler, |this, handler| {
                        this.on_click(move |_, window, cx| handler(None, window, cx))
                    })
                    .child(div().w(px(16.)).h(px(18.)).flex_none())
                    .child(folder_slot(IconName::Folder))
                    .child(div().min_w_0().flex_1().truncate().child(self.root_label)),
            )
            .child(
                div()
                    .mx_1()
                    .flex_none()
                    .border_t_1()
                    .border_color(cx.theme().border),
            )
            .child(
                div().flex_1().min_h(px(0.)).overflow_hidden().child(
                    v_flex()
                        .id(SharedString::from(format!("{id_prefix}-scroll")))
                        .size_full()
                        .overflow_y_scrollbar()
                        .children(rows.into_iter().enumerate().map(|(index, row)| {
                            let select_path = row.path.clone();
                            let toggle_path = row.path.clone();
                            let row_selected =
                                show_selection && selected.as_deref() == Some(row.path.as_str());
                            let select_handler = on_select.clone();
                            let toggle_handler = on_toggle.clone();
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
                            let row_id = 2 + index * 2;

                            h_flex()
                                .id(SharedString::from(format!("{id_prefix}-row-{row_id}")))
                                .h(px(32.))
                                .flex_none()
                                .items_center()
                                .gap_2()
                                .pl(px(10. + row.depth as f32 * 16.))
                                .pr_2()
                                .rounded_sm()
                                .cursor_pointer()
                                .text_size(rems(0.78))
                                .when(row_selected, |this| {
                                    this.bg(cx.theme().selection.opacity(0.72))
                                })
                                .hover(|this| this.bg(cx.theme().secondary.opacity(0.65)))
                                .when_some(select_handler, |this, handler| {
                                    this.on_click(move |_, window, cx| {
                                        handler(Some(select_path.clone()), window, cx)
                                    })
                                })
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "{id_prefix}-toggle-{}",
                                            row_id + 1
                                        )))
                                        .w(px(16.))
                                        .h(px(18.))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when_some(
                                            toggle_handler.filter(|_| row.has_children),
                                            |this, handler| {
                                                this.cursor_pointer().on_click(
                                                    move |_, window, cx| {
                                                        handler(toggle_path.clone(), window, cx);
                                                        cx.stop_propagation();
                                                    },
                                                )
                                            },
                                        )
                                        .when(row.has_children, |this| {
                                            this.child(
                                                Icon::new(disclosure_icon).with_size(Size::Small),
                                            )
                                        }),
                                )
                                .child(folder_slot(folder_icon))
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .truncate()
                                        .child(group_label(&row.path)),
                                )
                        })),
                ),
            );

        match self.layout {
            PickerLayout::Fill => picker.w_full().flex_1().min_h(px(0.)),
            PickerLayout::Popover { width, height } => picker.w(width).h(height).shadow_lg(),
        }
    }
}

fn folder_slot(icon: IconName) -> impl IntoElement {
    div()
        .w(px(16.))
        .h(px(18.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(Icon::new(icon).with_size(Size::Small))
}

fn group_label(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn group_tree_rows(groups: &[String], expanded: &HashSet<String>) -> Vec<GroupTreeRow> {
    let group_set = groups.iter().collect::<HashSet<_>>();
    let mut children = HashMap::<Option<String>, Vec<String>>::new();
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
        children: &mut HashMap<Option<String>, Vec<String>>,
        expanded: &HashSet<String>,
        rows: &mut Vec<GroupTreeRow>,
    ) {
        let key = parent.map(str::to_string);
        let Some(groups) = children.remove(&key) else {
            return;
        };
        for group in groups {
            let has_children = children.contains_key(&Some(group.clone()));
            let is_expanded = expanded.contains(&group);
            rows.push(GroupTreeRow {
                path: group.clone(),
                depth,
                has_children,
                expanded: is_expanded,
            });
            if is_expanded {
                append(Some(&group), depth + 1, children, expanded, rows);
            }
        }
    }

    let mut rows = Vec::new();
    append(None, 0, &mut children, expanded, &mut rows);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_only_top_level_groups_initially() {
        let groups = vec![
            "prod".to_string(),
            "prod/eu".to_string(),
            "prod/eu/database".to_string(),
            "shared".to_string(),
        ];

        assert_eq!(
            group_tree_rows(&groups, &HashSet::new()),
            vec![
                GroupTreeRow {
                    path: "prod".to_string(),
                    depth: 0,
                    has_children: true,
                    expanded: false,
                },
                GroupTreeRow {
                    path: "shared".to_string(),
                    depth: 0,
                    has_children: false,
                    expanded: false,
                },
            ]
        );
    }

    #[test]
    fn expands_only_requested_branches_one_level_at_a_time() {
        let groups = vec![
            "prod".to_string(),
            "prod/eu".to_string(),
            "prod/eu/database".to_string(),
            "prod/us".to_string(),
            "shared".to_string(),
            "shared/tools".to_string(),
        ];
        let expanded = HashSet::from(["prod".to_string()]);
        let rows = group_tree_rows(&groups, &expanded);

        assert_eq!(
            rows.iter()
                .map(|row| (row.path.as_str(), row.depth))
                .collect::<Vec<_>>(),
            vec![("prod", 0), ("prod/eu", 1), ("prod/us", 1), ("shared", 0)]
        );
        assert!(!rows.iter().any(|row| row.path == "prod/eu/database"));
        assert!(!rows.iter().any(|row| row.path == "shared/tools"));
    }
}
