//! App-owned settings sections keep navigation separate from form rendering.
use std::rc::Rc;

use gpui::{
    AnyElement, App, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex, setting::RenderOptions, v_flex};

type FieldRenderer = Rc<dyn Fn(&RenderOptions, &mut Window, &mut App) -> AnyElement>;
pub(super) type OpenAction = Rc<dyn Fn(&mut Window, &mut App)>;

pub(crate) struct SettingField(FieldRenderer);

impl SettingField {
    pub(crate) fn render<E: IntoElement + 'static>(
        render: impl Fn(&RenderOptions, &mut Window, &mut App) -> E + 'static,
    ) -> Self {
        Self(Rc::new(move |options, window, cx| {
            render(options, window, cx).into_any_element()
        }))
    }
}

pub(crate) struct SettingItem {
    title: Option<SharedString>,
    description: Option<SharedString>,
    render: FieldRenderer,
}

impl SettingItem {
    pub(crate) fn new(title: impl Into<SharedString>, field: SettingField) -> Self {
        Self {
            title: Some(title.into()),
            description: None,
            render: field.0,
        }
    }

    pub(crate) fn render<E: IntoElement + 'static>(
        render: impl Fn(&RenderOptions, &mut Window, &mut App) -> E + 'static,
    ) -> Self {
        Self {
            title: None,
            description: None,
            render: SettingField::render(render).0,
        }
    }

    pub(crate) fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    fn matches(&self, query: &str) -> bool {
        self.title
            .as_ref()
            .is_some_and(|title| contains_query(title, query))
            || self
                .description
                .as_ref()
                .is_some_and(|text| contains_query(text, query))
    }

    pub(super) fn render_item(
        &self,
        options: &RenderOptions,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let field = (self.render)(options, window, cx);
        let Some(title) = &self.title else {
            return div().w_full().child(field).into_any_element();
        };
        h_flex()
            .w_full()
            .items_start()
            .justify_between()
            .gap_3()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .max_w_3_5()
                    .gap_1()
                    .child(div().text_sm().child(title.clone()))
                    .when_some(self.description.clone(), |this, description| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        )
                    }),
            )
            .child(div().min_w_0().child(field))
            .into_any_element()
    }
}

pub(crate) struct SettingGroup {
    pub(super) title: Option<SharedString>,
    pub(super) items: Vec<SettingItem>,
    pub(super) action: Option<OpenAction>,
}

impl SettingGroup {
    pub(crate) fn new() -> Self {
        Self {
            title: None,
            items: Vec::new(),
            action: None,
        }
    }

    pub(crate) fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub(crate) fn item(mut self, item: SettingItem) -> Self {
        self.items.push(item);
        self
    }

    pub(crate) fn on_open(mut self, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.action = Some(Rc::new(action));
        self
    }

    pub(super) fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self
                .title
                .as_ref()
                .is_some_and(|title| contains_query(title, query))
            || self.items.iter().any(|item| item.matches(query))
    }

    pub(super) fn visible_items<'a>(
        &'a self,
        query: &'a str,
    ) -> impl Iterator<Item = (usize, &'a SettingItem)> {
        let whole_group = query.is_empty()
            || self
                .title
                .as_ref()
                .is_some_and(|title| contains_query(title, query));
        self.items
            .iter()
            .enumerate()
            .filter(move |(_, item)| whole_group || item.matches(query))
    }
}

pub(crate) struct SettingPage {
    pub(super) title: SharedString,
    pub(super) icon: Option<Icon>,
    pub(super) default_open: bool,
    pub(super) groups: Vec<SettingGroup>,
}

impl SettingPage {
    pub(crate) fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            icon: None,
            default_open: false,
            groups: Vec::new(),
        }
    }

    pub(crate) fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub(crate) fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub(crate) fn group(mut self, group: SettingGroup) -> Self {
        self.groups.push(group);
        self
    }

    pub(super) fn group_matches(&self, group_ix: usize, query: &str) -> bool {
        self.groups
            .get(group_ix)
            .is_some_and(|group| contains_query(&self.title, query) || group.matches(query))
    }

    pub(super) fn first_content_group(&self, query: &str) -> Option<usize> {
        // Untitled groups are shared context (e.g. the About header/footer).
        self.groups
            .iter()
            .enumerate()
            .filter(|(ix, group)| group.action.is_none() && self.group_matches(*ix, query))
            .min_by_key(|(_, group)| group.title.is_none())
            .map(|(ix, _)| ix)
    }

    pub(super) fn content_groups(&self, selected: usize, query: &str) -> Vec<usize> {
        if self
            .groups
            .get(selected)
            .is_none_or(|group| group.action.is_some())
        {
            return Vec::new();
        }
        self.groups
            .iter()
            .enumerate()
            .filter(|(ix, group)| {
                group.action.is_none()
                    && (*ix == selected || group.title.is_none())
                    && self.group_matches(*ix, query)
            })
            .map(|(ix, _)| ix)
            .collect()
    }
}

pub(super) fn contains_query(text: &str, query: &str) -> bool {
    text.to_lowercase().contains(&query.to_lowercase())
}
