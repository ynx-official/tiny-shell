use super::*;
use gpui::SharedString;
use gpui_component::{
    IndexPath,
    searchable_list::SearchableListChange,
    select::{SelectDelegate, SelectItem},
};
use std::{cell::Cell, rc::Rc};

#[derive(Clone)]
pub(super) struct DiscoveredModel {
    pub id: String,
    added: Rc<Cell<bool>>,
}

impl DiscoveredModel {
    pub fn new(id: String, added: bool) -> Self {
        Self {
            id,
            added: Rc::new(Cell::new(added)),
        }
    }

    pub fn set_added(&self, added: bool) -> bool {
        self.added.replace(added) != added
    }
}

impl SelectItem for DiscoveredModel {
    type Value = String;

    fn title(&self) -> SharedString {
        self.id.clone().into()
    }
    fn value(&self) -> &String {
        &self.id
    }
    fn disabled(&self) -> bool {
        self.added.get()
    }

    fn render(&self, window: &mut Window, _: &mut gpui::App) -> impl gpui::IntoElement {
        let added = self.added.get();
        h_flex()
            .w(select_menu_width(window) - px(56.))
            .min_w_0()
            .gap_2()
            .child(
                selectors::choice_label(&self.id, &self.id, window)
                    .flex_1()
                    .min_w_0(),
            )
            .when(added, |row| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .child(t!("ai_model_added").to_string()),
                )
            })
    }
}

pub(super) struct DiscoveredModels(SearchableVec<DiscoveredModel>);

impl DiscoveredModels {
    pub fn new(items: Vec<DiscoveredModel>) -> Self {
        Self(SearchableVec::new(items))
    }
}

impl SelectDelegate for DiscoveredModels {
    type Item = DiscoveredModel;

    fn items_count(&self, section: usize) -> usize {
        self.0.items_count(section)
    }
    fn item(&self, index: IndexPath) -> Option<&Self::Item> {
        self.0.item(index)
    }
    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Self::Item: SelectItem<Value = V>,
        V: PartialEq,
    {
        self.0.position(value)
    }
    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> gpui::Task<()> {
        self.0.perform_search(query, window, cx)
    }
    fn is_item_checked(
        &self,
        _: IndexPath,
        item: &Self::Item,
        _: &[(IndexPath, Self::Item)],
        _: &gpui::App,
    ) -> bool {
        item.disabled()
    }
    fn on_will_change(
        &mut self,
        selection: &mut Vec<(IndexPath, Self::Item)>,
        changes: &[SearchableListChange],
    ) {
        // The pinned list still confirms disabled cursor rows on Enter. Veto them
        // here as well as marking them disabled, before the Select emits a value.
        *selection = changes
            .iter()
            .rev()
            .find_map(|change| {
                let SearchableListChange::Select { index } = change else {
                    return None;
                };
                self.item(*index)
                    .filter(|item| !item.disabled())
                    .map(|item| vec![(*index, item.clone())])
            })
            .unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_rows_are_marked_and_cannot_be_committed_until_removed() {
        let item = DiscoveredModel::new("already-added".into(), true);
        let mut choices = DiscoveredModels::new(vec![item.clone()]);
        let index = IndexPath::default().row(0);
        let mut selection = Vec::new();
        assert!(choices.item(index).unwrap().disabled());
        choices.on_will_change(&mut selection, &[SearchableListChange::Select { index }]);
        assert!(selection.is_empty());
        item.set_added(false);
        assert!(!choices.item(index).unwrap().disabled());
        choices.on_will_change(&mut selection, &[SearchableListChange::Select { index }]);
        assert_eq!(selection[0].1.value(), "already-added");
    }

    #[gpui::test]
    fn added_flags_stay_current_while_results_are_filtered(cx: &mut gpui::TestAppContext) {
        cx.add_window(|window, cx| {
            let item = DiscoveredModel::new("needle".into(), false);
            let mut choices = DiscoveredModels::new(vec![
                DiscoveredModel::new("other".into(), false),
                item.clone(),
            ]);
            let _search = choices.perform_search("needle", window, cx);
            let index = IndexPath::default().row(0);
            assert_eq!(choices.items_count(0), 1);
            assert_eq!(choices.item(index).unwrap().value(), "needle");
            item.set_added(true);
            assert!(choices.item(index).unwrap().disabled());
            assert!(choices.is_item_checked(index, choices.item(index).unwrap(), &[], cx));
            item.set_added(false);
            assert!(!choices.item(index).unwrap().disabled());
            assert_eq!(choices.items_count(0), 1);
            Empty
        });
    }

    struct Empty;
    impl Render for Empty {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
            div()
        }
    }
}
