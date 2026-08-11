use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use crate::app::{PaneLayout, SystemInfoTab, TabGroup};
use crate::terminal::TerminalTab;

#[derive(Default)]
struct WorkspaceIndexes {
    tabs: HashMap<String, usize>,
    groups: HashMap<String, usize>,
    group_by_tab: HashMap<String, String>,
}

pub(crate) struct TerminalWorkspaceState {
    tabs: Vec<TerminalTab>,
    active_tab: Option<String>,
    tab_groups: Vec<TabGroup>,
    next_tab_group_ordinal: u64,
    active_group: Option<String>,
    system_info_tabs: Vec<SystemInfoTab>,
    active_system_info_tab: Option<String>,
    pane_root: PaneLayout,
    focused_pane_path: Vec<usize>,
    indexes: RefCell<WorkspaceIndexes>,
    indexes_dirty: Cell<bool>,
}

impl Default for TerminalWorkspaceState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            tab_groups: Vec::new(),
            next_tab_group_ordinal: 0,
            active_group: None,
            system_info_tabs: Vec::new(),
            active_system_info_tab: None,
            pane_root: PaneLayout::Empty,
            focused_pane_path: Vec::new(),
            indexes: RefCell::new(WorkspaceIndexes::default()),
            indexes_dirty: Cell::new(false),
        }
    }
}

pub(crate) fn extract_system_info_transfer(
    tabs: &[SystemInfoTab],
    active_id: Option<&str>,
    valid_tab_ids: &[String],
) -> (Vec<SystemInfoTab>, Option<String>) {
    let extracted = tabs
        .iter()
        .filter(|tab| valid_tab_ids.iter().any(|id| id == &tab.source_tab_id))
        .cloned()
        .collect::<Vec<_>>();
    let active = active_id
        .filter(|id| extracted.iter().any(|tab| tab.id == *id))
        .map(str::to_owned);
    (extracted, active)
}

pub(crate) fn restore_system_info_transfer(
    destination: &mut Vec<SystemInfoTab>,
    transferred: Vec<SystemInfoTab>,
    active_id: Option<String>,
) -> Option<String> {
    destination.extend(transferred);
    active_id.filter(|id| destination.iter().any(|tab| tab.id == *id))
}

pub(crate) fn choose_transfer_active_tab(
    active_tab: Option<&str>,
    pane_root: &PaneLayout,
    valid_tabs: &[String],
) -> Option<String> {
    let is_valid =
        |tab_id: &str| pane_root.contains(tab_id) && valid_tabs.iter().any(|id| id == tab_id);
    active_tab
        .filter(|tab_id| is_valid(tab_id))
        .map(str::to_owned)
        .or_else(|| {
            pane_root
                .tab_ids()
                .into_iter()
                .find(|tab_id| is_valid(tab_id))
                .map(str::to_owned)
        })
}

impl TerminalWorkspaceState {
    fn invalidate_indexes(&self) {
        self.indexes_dirty.set(true);
    }

    fn ensure_indexes(&self) {
        if !self.indexes_dirty.replace(false) {
            return;
        }
        let mut indexes = self.indexes.borrow_mut();
        indexes.tabs.clear();
        indexes.groups.clear();
        indexes.group_by_tab.clear();
        for (index, tab) in self.tabs.iter().enumerate() {
            indexes.tabs.entry(tab.id.clone()).or_insert(index);
        }
        for (index, group) in self.tab_groups.iter().enumerate() {
            indexes.groups.entry(group.id.clone()).or_insert(index);
            for tab_id in group.pane_root.tab_ids() {
                indexes
                    .group_by_tab
                    .entry(tab_id.to_owned())
                    .or_insert_with(|| group.id.clone());
            }
        }
    }

    pub(crate) fn new() -> Self {
        Self {
            next_tab_group_ordinal: 1,
            ..Self::default()
        }
    }

    pub(crate) fn tabs(&self) -> &[TerminalTab] {
        &self.tabs
    }

    pub(crate) fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub(crate) fn active_tab_id(&self) -> Option<&str> {
        self.active_tab.as_deref()
    }

    pub(crate) fn active_tab_value(&self) -> Option<String> {
        self.active_tab.clone()
    }

    pub(crate) fn active_group_value(&self) -> Option<String> {
        self.active_group.clone()
    }

    pub(crate) fn pane_root_mut(&mut self) -> &mut PaneLayout {
        self.ensure_indexes();
        let active_group_index = self
            .active_group
            .as_deref()
            .and_then(|group_id| self.indexes.borrow().groups.get(group_id).copied());
        if let Some(index) = active_group_index {
            self.invalidate_indexes();
            return &mut self.tab_groups[index].pane_root;
        }
        &mut self.pane_root
    }

    pub(crate) fn system_info_tabs_mut(&mut self) -> &mut Vec<SystemInfoTab> {
        &mut self.system_info_tabs
    }

    pub(crate) fn remove_group_at(&mut self, index: usize) -> Option<TabGroup> {
        self.remove_group(index)
    }

    pub(crate) fn insert_group(&mut self, index: usize, group: TabGroup) {
        self.tab_groups
            .insert(index.min(self.tab_groups.len()), group);
        self.invalidate_indexes();
    }

    pub(crate) fn insert_tab(&mut self, index: usize, tab: TerminalTab) {
        self.tabs.insert(index.min(self.tabs.len()), tab);
        self.invalidate_indexes();
    }

    pub(crate) fn tab_groups(&self) -> &[TabGroup] {
        &self.tab_groups
    }

    pub(crate) fn active_group_id(&self) -> Option<&str> {
        self.active_group.as_deref()
    }

    pub(crate) fn system_info_tabs(&self) -> &[SystemInfoTab] {
        &self.system_info_tabs
    }

    pub(crate) fn active_system_info_tab_id(&self) -> Option<&str> {
        self.active_system_info_tab.as_deref()
    }

    pub(crate) fn pane_root(&self) -> &PaneLayout {
        self.ensure_indexes();
        if let Some(index) = self
            .active_group
            .as_deref()
            .and_then(|group_id| self.indexes.borrow().groups.get(group_id).copied())
        {
            return &self.tab_groups[index].pane_root;
        }
        &self.pane_root
    }

    pub(crate) fn focused_pane_path(&self) -> &[usize] {
        &self.focused_pane_path
    }

    pub(crate) fn tabs_mut(&mut self) -> &mut Vec<TerminalTab> {
        self.invalidate_indexes();
        &mut self.tabs
    }

    pub(crate) fn terminal_tab_mut(&mut self, tab_id: &str) -> Option<&mut TerminalTab> {
        self.ensure_indexes();
        let index = self.indexes.borrow().tabs.get(tab_id).copied()?;
        self.invalidate_indexes();
        self.tabs.get_mut(index)
    }

    pub(crate) fn tab_groups_mut(&mut self) -> &mut Vec<TabGroup> {
        self.invalidate_indexes();
        &mut self.tab_groups
    }

    pub(crate) fn tab_group_mut(&mut self, group_id: &str) -> Option<&mut TabGroup> {
        self.ensure_indexes();
        let index = self.indexes.borrow().groups.get(group_id).copied()?;
        self.invalidate_indexes();
        self.tab_groups.get_mut(index)
    }

    pub(crate) fn clear(&mut self) {
        self.tabs.clear();
        self.tab_groups.clear();
        self.system_info_tabs.clear();
        self.active_tab = None;
        self.active_group = None;
        self.active_system_info_tab = None;
        self.pane_root = PaneLayout::Empty;
        self.focused_pane_path.clear();
        self.invalidate_indexes();
    }

    pub(crate) fn push_tab(&mut self, tab: TerminalTab) {
        self.tabs.push(tab);
        self.invalidate_indexes();
    }

    pub(crate) fn take_tabs(&mut self) -> Vec<TerminalTab> {
        let tabs = std::mem::take(&mut self.tabs);
        self.invalidate_indexes();
        tabs
    }

    pub(crate) fn replace_tabs(&mut self, tabs: Vec<TerminalTab>) {
        self.tabs = tabs;
        self.invalidate_indexes();
    }

    pub(crate) fn append_tabs(&mut self, mut tabs: Vec<TerminalTab>) {
        self.tabs.append(&mut tabs);
        self.invalidate_indexes();
    }

    pub(crate) fn push_group(&mut self, group: TabGroup) {
        self.tab_groups.push(group);
        self.invalidate_indexes();
    }

    pub(crate) fn remove_group(&mut self, index: usize) -> Option<TabGroup> {
        let removed = (index < self.tab_groups.len()).then(|| self.tab_groups.remove(index));
        if removed.is_some() {
            self.invalidate_indexes();
        }
        removed
    }

    pub(crate) fn push_system_info_tab(&mut self, tab: SystemInfoTab) {
        self.system_info_tabs.push(tab);
    }

    pub(crate) fn remove_system_info_tab(&mut self, id: &str) -> Option<SystemInfoTab> {
        let index = self.system_info_tabs.iter().position(|tab| tab.id == id)?;
        Some(self.system_info_tabs.remove(index))
    }

    pub(crate) fn set_active_tab(&mut self, tab_id: Option<String>) {
        self.active_tab = tab_id;
    }

    pub(crate) fn set_active_group(&mut self, group_id: Option<String>) {
        self.active_group = group_id;
    }

    pub(crate) fn set_active_system_info_tab(&mut self, tab_id: Option<String>) {
        self.active_system_info_tab = tab_id;
    }

    pub(crate) fn clear_active_system_info_tab(&mut self) {
        self.active_system_info_tab = None;
    }

    pub(crate) fn set_pane_root(&mut self, pane_root: PaneLayout) {
        self.ensure_indexes();
        if let Some(index) = self
            .active_group
            .as_deref()
            .and_then(|group_id| self.indexes.borrow().groups.get(group_id).copied())
        {
            self.tab_groups[index].pane_root = pane_root;
            self.invalidate_indexes();
        } else {
            self.pane_root = pane_root;
        }
    }

    pub(crate) fn set_focused_pane_path(&mut self, path: Vec<usize>) {
        self.focused_pane_path = path;
    }

    pub(crate) fn clear_focused_pane_path(&mut self) {
        self.focused_pane_path.clear();
    }

    pub(crate) fn reserve_tab_group_ordinal(&mut self) -> u64 {
        self.allocate_tab_group_ordinal()
    }

    pub(crate) fn next_tab_group_ordinal(&self) -> u64 {
        self.next_tab_group_ordinal
    }

    pub(crate) fn set_next_tab_group_ordinal(&mut self, ordinal: u64) {
        self.next_tab_group_ordinal = ordinal;
    }

    pub(crate) fn terminal_tab(&self, tab_id: &str) -> Option<&TerminalTab> {
        self.ensure_indexes();
        let index = self.indexes.borrow().tabs.get(tab_id).copied()?;
        self.tabs.get(index)
    }

    pub(crate) fn tab_group(&self, group_id: &str) -> Option<&TabGroup> {
        self.ensure_indexes();
        let index = self.indexes.borrow().groups.get(group_id).copied()?;
        self.tab_groups.get(index)
    }

    pub(crate) fn group_id_for_tab(&self, tab_id: &str) -> Option<String> {
        self.ensure_indexes();
        self.indexes.borrow().group_by_tab.get(tab_id).cloned()
    }

    pub(crate) fn allocate_tab_group_ordinal(&mut self) -> u64 {
        let ordinal = self.next_tab_group_ordinal;
        self.next_tab_group_ordinal = self.next_tab_group_ordinal.saturating_add(1);
        ordinal
    }

    pub(crate) fn activate_terminal_tab(&mut self, tab_id: &str) -> bool {
        if self.terminal_tab(tab_id).is_none() {
            return false;
        }
        self.active_tab = Some(tab_id.to_owned());
        self.active_group = self.group_id_for_tab(tab_id);
        true
    }

    pub(crate) fn install_terminal_tab(&mut self, tab: TerminalTab, group: TabGroup) {
        let tab_id = tab.id.clone();
        let group_id = group.id.clone();
        self.tabs.push(tab);
        self.tab_groups.push(group);
        self.active_tab = Some(tab_id.clone());
        self.active_group = Some(group_id);
        self.pane_root = PaneLayout::Empty;
        self.focused_pane_path.clear();
        self.invalidate_indexes();
    }

    pub(crate) fn preferred_terminal_tab_id(&self) -> Option<String> {
        if let Some(active_id) = self.active_tab.as_deref() {
            if self.terminal_tab(active_id).is_some() {
                return Some(active_id.to_owned());
            }
        }

        self.active_group
            .as_deref()
            .and_then(|group_id| self.tab_group(group_id))
            .and_then(|group| group.pane_root.tab_ids().into_iter().next())
            .map(str::to_owned)
            .or_else(|| self.tabs.first().map(|tab| tab.id.clone()))
    }
}

#[derive(Default)]
pub(crate) struct WindowState {
    workspace: TerminalWorkspaceState,
    pub(crate) search_active: bool,
    pub(crate) search_epoch: u64,
    pub(crate) search_query: String,
    pub(crate) search_matches: Vec<(i32, i32)>,
    pub(crate) search_current: usize,
    pub(crate) search_target_tab: Option<String>,
    pub(crate) search_bar_bounds: Option<gpui::Bounds<gpui::Pixels>>,
    dialog: DialogState,
}

use crate::app::runtime_state::{DialogActivation, DialogCoordinator};

#[derive(Default)]
struct DialogState {
    coordinator: DialogCoordinator,
}

impl WindowState {
    pub(crate) fn new() -> Self {
        Self {
            workspace: TerminalWorkspaceState::new(),
            ..Self::default()
        }
    }

    pub(crate) fn workspace(&self) -> &TerminalWorkspaceState {
        &self.workspace
    }

    pub(crate) fn workspace_state_mut(&mut self) -> &mut TerminalWorkspaceState {
        &mut self.workspace
    }

    pub(crate) fn request_dialog(
        &mut self,
        kind: crate::app::DialogKind,
    ) -> crate::app::runtime_state::DialogToken {
        self.dialog.coordinator.request(kind)
    }

    pub(crate) fn is_same_active_dialog(&self, kind: crate::app::DialogKind) -> bool {
        self.dialog.coordinator.is_same_active(kind)
    }

    pub(crate) fn active_request(&self) -> Option<crate::app::runtime_state::DialogRequest> {
        self.dialog.coordinator.active()
    }

    pub(crate) fn pending_token(&self) -> Option<crate::app::runtime_state::DialogToken> {
        self.dialog
            .coordinator
            .pending()
            .map(|request| request.token)
    }

    pub(crate) fn activate_dialog(
        &mut self,
        token: crate::app::runtime_state::DialogToken,
    ) -> DialogActivation {
        self.dialog.coordinator.activate(token)
    }

    pub(crate) fn dialog_closed(&mut self, token: crate::app::runtime_state::DialogToken) -> bool {
        self.dialog.coordinator.close(token)
    }

    pub(crate) fn dialog_kind(&self) -> Option<crate::app::DialogKind> {
        self.dialog.coordinator.active().map(|request| request.kind)
    }
    #[cfg(test)]
    pub(crate) fn pending_dialog(&self) -> Option<crate::app::DialogKind> {
        self.dialog
            .coordinator
            .pending()
            .map(|request| request.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::{PaneLayout, TerminalWorkspaceState, WindowState};
    use crate::app::{DialogKind, SystemInfoTab};

    fn single(id: &str) -> PaneLayout {
        PaneLayout::Single(id.to_string())
    }

    #[test]
    fn system_info_transfer_extracts_and_restores_only_related_tabs() {
        let tabs = vec![
            SystemInfoTab {
                id: "info-a".into(),
                source_tab_id: "a".into(),
                title: "A".into(),
            },
            SystemInfoTab {
                id: "info-b".into(),
                source_tab_id: "b".into(),
                title: "B".into(),
            },
        ];
        let valid = vec!["a".to_string()];
        let (moved, active) = super::extract_system_info_transfer(&tabs, Some("info-a"), &valid);
        assert_eq!(moved.len(), 1);
        assert_eq!(active.as_deref(), Some("info-a"));
        let mut destination = vec![tabs[1].clone()];
        assert_eq!(
            super::restore_system_info_transfer(&mut destination, moved, active),
            Some("info-a".to_string())
        );
        assert_eq!(destination.len(), 2);
    }

    #[test]
    fn transfer_active_tab_only_uses_valid_pane_tabs() {
        let layout = PaneLayout::Horizontal(vec![single("a"), single("b")], 0.5);
        let valid = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            super::choose_transfer_active_tab(Some("b"), &layout, &valid),
            Some("b".to_string())
        );
        assert_eq!(
            super::choose_transfer_active_tab(Some("old-target"), &layout, &valid),
            Some("a".to_string())
        );
        assert_eq!(
            super::choose_transfer_active_tab(None, &PaneLayout::Empty, &valid),
            None
        );
    }

    #[test]
    fn removing_missing_tab_preserves_layout() {
        let mut layout = PaneLayout::Horizontal(vec![single("a"), single("b")], 0.5);

        assert!(!layout.remove_tab("missing"));
        assert_eq!(layout.tab_ids(), vec!["a", "b"]);
    }

    #[test]
    fn removing_leaf_collapses_single_child_parent() {
        let mut layout = PaneLayout::Horizontal(
            vec![
                single("a"),
                PaneLayout::Vertical(vec![single("b"), single("c")], 0.5),
            ],
            0.5,
        );

        assert!(layout.remove_tab("b"));
        assert_eq!(layout.tab_ids(), vec!["a", "c"]);
        assert!(matches!(layout, PaneLayout::Horizontal(_, _)));
    }

    #[test]
    fn removing_last_leaf_returns_empty_without_empty_id() {
        let mut layout = single("only");

        assert!(layout.remove_tab("only"));
        assert!(matches!(layout, PaneLayout::Empty));
        assert!(layout.tab_ids().is_empty());
    }

    #[test]
    fn same_active_dialog_kind_is_detectable_without_mutating_pending_state() {
        let mut state = WindowState::new();
        let token = state.request_dialog(DialogKind::Updater);
        assert_eq!(state.activate_dialog(token).token(), Some(token));
        assert!(state.is_same_active_dialog(DialogKind::Updater));
        assert!(!state.is_same_active_dialog(DialogKind::Transfers));
        assert!(state.pending_dialog().is_none());
    }

    #[test]
    fn dialog_requests_are_single_slot_and_close_clears_state() {
        let mut state = WindowState::new();

        let transfers = state.request_dialog(DialogKind::Transfers);
        assert_eq!(state.activate_dialog(transfers).token(), Some(transfers));
        let updater = state.request_dialog(DialogKind::Updater);
        assert_eq!(state.pending_dialog(), Some(DialogKind::Updater));
        assert!(!state.activate_dialog(updater));
        assert!(state.dialog_closed(transfers));
        assert!(state.dialog_kind().is_none());
    }

    #[test]
    fn dialog_requests_replace_pending_after_active_dialog() {
        let mut state = WindowState::new();

        let transfers = state.request_dialog(DialogKind::Transfers);
        assert_eq!(state.activate_dialog(transfers).token(), Some(transfers));
        let updater = state.request_dialog(DialogKind::Updater);
        assert!(state.dialog_closed(transfers));
        assert_eq!(state.pending_dialog(), Some(DialogKind::Updater));
        assert!(!state.dialog_closed(transfers));
        assert_eq!(state.activate_dialog(updater).token(), Some(updater));
        assert_eq!(state.dialog_kind(), Some(DialogKind::Updater));
    }

    #[test]
    fn workspace_queries_and_clear_preserve_invariants() {
        let mut state = TerminalWorkspaceState::new();

        assert_eq!(state.tab_count(), 0);
        assert!(state.active_tab_id().is_none());
        assert!(state.active_group_id().is_none());
        assert!(state.system_info_tabs().is_empty());
        assert!(matches!(state.pane_root(), PaneLayout::Empty));
        assert!(state.focused_pane_path().is_empty());

        state.set_active_tab(Some("tab".to_string()));
        state.set_active_group(Some("group".to_string()));
        state.set_active_system_info_tab(Some("info".to_string()));
        state.set_pane_root(single("tab"));
        state.set_focused_pane_path(vec![1, 0]);
        state.clear();

        assert_eq!(state.tab_count(), 0);
        assert!(state.active_tab_id().is_none());
        assert!(state.active_group_id().is_none());
        assert!(state.active_system_info_tab_id().is_none());
        assert!(matches!(state.pane_root(), PaneLayout::Empty));
        assert!(state.focused_pane_path().is_empty());
    }

    #[test]
    fn ordinal_reservation_is_monotonic() {
        let mut state = TerminalWorkspaceState::new();

        assert_eq!(state.reserve_tab_group_ordinal(), 1);
        assert_eq!(state.reserve_tab_group_ordinal(), 2);
        state.set_next_tab_group_ordinal(10);
        assert_eq!(state.reserve_tab_group_ordinal(), 10);
    }
}
