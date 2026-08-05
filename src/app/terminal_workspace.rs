use std::ops::{Deref, DerefMut};

use crate::app::{PaneLayout, SystemInfoTab, TabGroup};
use crate::terminal::TerminalTab;

pub(crate) struct TerminalWorkspaceState {
    pub(crate) tabs: Vec<TerminalTab>,
    pub(crate) active_tab: Option<String>,
    pub(crate) tab_groups: Vec<TabGroup>,
    pub(crate) next_tab_group_ordinal: u64,
    pub(crate) active_group: Option<String>,
    pub(crate) system_info_tabs: Vec<SystemInfoTab>,
    pub(crate) active_system_info_tab: Option<String>,
    pub(crate) pane_root: PaneLayout,
    pub(crate) focused_pane_path: Vec<usize>,
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
        }
    }
}

impl TerminalWorkspaceState {
    pub(crate) fn new() -> Self {
        Self {
            next_tab_group_ordinal: 1,
            ..Self::default()
        }
    }

    pub(crate) fn terminal_tab(&self, tab_id: &str) -> Option<&TerminalTab> {
        self.tabs.iter().find(|tab| tab.id == tab_id)
    }

    pub(crate) fn terminal_tab_mut(&mut self, tab_id: &str) -> Option<&mut TerminalTab> {
        self.tabs.iter_mut().find(|tab| tab.id == tab_id)
    }

    pub(crate) fn tab_group(&self, group_id: &str) -> Option<&TabGroup> {
        self.tab_groups.iter().find(|group| group.id == group_id)
    }

    pub(crate) fn tab_group_mut(&mut self, group_id: &str) -> Option<&mut TabGroup> {
        self.tab_groups
            .iter_mut()
            .find(|group| group.id == group_id)
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
        self.active_group = self
            .tab_groups
            .iter()
            .find(|group| group.pane_root.contains(tab_id))
            .map(|group| group.id.clone());
        true
    }

    pub(crate) fn set_system_info_tab(&mut self, tab_id: Option<String>) {
        self.active_system_info_tab = tab_id;
    }

    pub(crate) fn install_terminal_tab(&mut self, tab: TerminalTab, group: TabGroup) {
        let tab_id = tab.id.clone();
        let group_id = group.id.clone();
        self.tabs.push(tab);
        self.tab_groups.push(group);
        self.active_tab = Some(tab_id.clone());
        self.active_group = Some(group_id);
        self.pane_root = PaneLayout::Single(tab_id);
        self.focused_pane_path.clear();
    }

    pub(crate) fn preferred_terminal_tab_id(&self) -> Option<String> {
        if let Some(active_id) = self.active_tab.as_deref()
            && self.terminal_tab(active_id).is_some()
        {
            return Some(active_id.to_owned());
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
    pub(crate) workspace: TerminalWorkspaceState,
    pub(crate) search_active: bool,
    pub(crate) search_epoch: u64,
    pub(crate) search_query: String,
    pub(crate) search_matches: Vec<(i32, i32)>,
    pub(crate) search_current: usize,
    pub(crate) search_target_tab: Option<String>,
    pub(crate) search_bar_bounds: Option<gpui::Bounds<gpui::Pixels>>,
    pub(crate) pending_dialog: Option<crate::app::DialogKind>,
    pub(crate) active_dialog: Option<crate::app::DialogKind>,
}

impl WindowState {
    pub(crate) fn new() -> Self {
        Self {
            workspace: TerminalWorkspaceState::new(),
            ..Self::default()
        }
    }

    pub(crate) fn request_dialog(&mut self, kind: crate::app::DialogKind) {
        if self.active_dialog.is_none() {
            self.pending_dialog = Some(kind);
        }
    }

    pub(crate) fn take_pending_dialog(&mut self) -> Option<crate::app::DialogKind> {
        self.pending_dialog.take()
    }

    pub(crate) fn mark_dialog_open(&mut self, kind: crate::app::DialogKind) {
        self.pending_dialog = None;
        self.active_dialog = Some(kind);
    }

    pub(crate) fn mark_dialog_closed(&mut self) {
        self.active_dialog = None;
    }
}

impl Deref for WindowState {
    type Target = TerminalWorkspaceState;

    fn deref(&self) -> &Self::Target {
        &self.workspace
    }
}

impl DerefMut for WindowState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.workspace
    }
}

#[cfg(test)]
mod tests {
    use super::{PaneLayout, TerminalWorkspaceState};

    fn single(id: &str) -> PaneLayout {
        PaneLayout::Single(id.to_string())
    }

    #[test]
    fn removing_missing_tab_reports_no_change() {
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
    fn workspace_state_starts_with_empty_pane_root() {
        let state = TerminalWorkspaceState::new();

        assert!(matches!(state.pane_root, PaneLayout::Empty));
        assert!(state.focused_pane_path.is_empty());
    }
}
