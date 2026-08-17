use std::collections::{HashMap, HashSet};

use gpui::{AnyWindowHandle, App, Context, Entity, Window};
use rust_i18n::t;

use crate::{PaneLayout, TabGroup, TinyShell, terminal::TerminalTab};

use super::{
    SystemInfoTab,
    tab_drag::{DockZone, should_close_empty_source},
};

/// Live resources removed from one window while a tab group is transferred.
///
/// The source keeps this value intact until the target commits. If validation,
/// route handoff, or window creation fails, the same value is used to restore
/// the source without recreating terminal or SFTP backends.
pub(crate) struct GroupTransfer {
    pub(crate) group: TabGroup,
    pub(crate) group_index: usize,
    pub(crate) tabs: Vec<(usize, TerminalTab)>,
    pub(crate) sftp_handles: HashMap<String, crate::sftp::SftpHandle>,
    pub(crate) route_ids: Vec<String>,
    pub(crate) active_tab: Option<String>,
    pub(crate) system_info_tabs: Vec<SystemInfoTab>,
    pub(crate) active_system_info_tab: Option<String>,
    pub(crate) was_active_group: bool,
}

#[derive(Debug)]
struct TransferManifest {
    group_id: String,
    layout_tab_ids: Vec<String>,
    tab_ids: Vec<String>,
    sftp_handle_ids: Vec<String>,
    route_ids: Vec<String>,
}

struct TransferTargetManifest {
    group_ids: HashSet<String>,
    tab_ids: HashSet<String>,
    sftp_handle_ids: HashSet<String>,
}

impl GroupTransfer {
    fn manifest(&self) -> TransferManifest {
        TransferManifest {
            group_id: self.group.id.clone(),
            layout_tab_ids: self
                .group
                .pane_root
                .tab_ids()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            tab_ids: self.tabs.iter().map(|(_, tab)| tab.id.clone()).collect(),
            sftp_handle_ids: self.sftp_handles.keys().cloned().collect(),
            route_ids: self.route_ids.clone(),
        }
    }

    fn chosen_active_tab(&self) -> Option<String> {
        let valid_tab_ids = self
            .tabs
            .iter()
            .map(|(_, tab)| tab.id.clone())
            .collect::<Vec<_>>();
        super::terminal_workspace::choose_transfer_active_tab(
            self.active_tab.as_deref(),
            &self.group.pane_root,
            &valid_tab_ids,
        )
    }
}

pub(crate) fn validate_transfer_batch(transfers: &[GroupTransfer]) -> Result<(), &'static str> {
    let manifests = transfers
        .iter()
        .map(GroupTransfer::manifest)
        .collect::<Vec<_>>();
    validate_transfer_manifests(&manifests)
}

fn validate_transfer_manifests(manifests: &[TransferManifest]) -> Result<(), &'static str> {
    let mut group_ids = HashSet::new();
    let mut tab_ids = HashSet::new();
    let mut sftp_handle_ids = HashSet::new();
    let mut route_ids = HashSet::new();

    for manifest in manifests {
        validate_transfer_manifest(manifest)?;
        if !group_ids.insert(manifest.group_id.as_str()) {
            return Err("transfer batch contains duplicate groups");
        }
        if manifest
            .tab_ids
            .iter()
            .any(|id| !tab_ids.insert(id.as_str()))
        {
            return Err("transfer batch contains duplicate terminals");
        }
        if manifest
            .sftp_handle_ids
            .iter()
            .any(|id| !sftp_handle_ids.insert(id.as_str()))
        {
            return Err("transfer batch contains duplicate SFTP handles");
        }
        if manifest
            .route_ids
            .iter()
            .any(|id| !route_ids.insert(id.as_str()))
        {
            return Err("transfer batch contains duplicate backend routes");
        }
    }
    Ok(())
}

fn validate_transfer_manifest(manifest: &TransferManifest) -> Result<(), &'static str> {
    let layout = manifest
        .layout_tab_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let tabs = manifest
        .tab_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if manifest.tab_ids.is_empty()
        || layout.len() != manifest.layout_tab_ids.len()
        || tabs.len() != manifest.tab_ids.len()
        || layout != tabs
    {
        return Err("transfer payload does not match the group layout");
    }
    Ok(())
}

fn validate_transfer_target_manifest(
    transfer: &TransferManifest,
    target: &TransferTargetManifest,
    preserve_group: bool,
) -> Result<(), &'static str> {
    validate_transfer_manifest(transfer)?;
    if preserve_group && target.group_ids.contains(&transfer.group_id) {
        return Err("target already contains this group");
    }
    if transfer
        .tab_ids
        .iter()
        .any(|id| target.tab_ids.contains(id))
    {
        return Err("target already contains one of the transferred terminals");
    }
    if transfer
        .sftp_handle_ids
        .iter()
        .any(|id| target.sftp_handle_ids.contains(id))
    {
        return Err("target already contains one of the transferred SFTP handles");
    }
    Ok(())
}

fn validate_transfer_batch_for_target(
    manifests: &[TransferManifest],
    target: &TransferTargetManifest,
    preserve_groups: bool,
) -> Result<(), &'static str> {
    validate_transfer_manifests(manifests)?;
    for manifest in manifests {
        validate_transfer_target_manifest(manifest, target, preserve_groups)?;
    }
    Ok(())
}

fn effective_batch_drop_zone(transfer_count: usize, requested: DockZone) -> DockZone {
    if transfer_count == 1 {
        requested
    } else {
        DockZone::Center
    }
}

#[cfg(test)]
fn validate_transfer_layout<'a>(
    layout: &PaneLayout,
    tab_ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), &'static str> {
    let tab_ids = tab_ids.into_iter().collect::<Vec<_>>();
    let unique_tab_ids = tab_ids.iter().copied().collect::<HashSet<_>>();
    let layout_ids = layout.tab_ids();
    let unique_layout_ids = layout_ids.iter().copied().collect::<HashSet<_>>();

    if tab_ids.is_empty()
        || unique_tab_ids.len() != tab_ids.len()
        || unique_layout_ids.len() != layout_ids.len()
        || unique_tab_ids != unique_layout_ids
    {
        return Err("transfer payload does not match the group layout");
    }
    Ok(())
}

pub(crate) fn merge_pane_layout(
    incoming: PaneLayout,
    existing: PaneLayout,
    zone: DockZone,
) -> Option<PaneLayout> {
    match zone {
        DockZone::Left => Some(PaneLayout::Vertical(vec![incoming, existing], 0.5)),
        DockZone::Right => Some(PaneLayout::Vertical(vec![existing, incoming], 0.5)),
        DockZone::Up => Some(PaneLayout::Horizontal(vec![incoming, existing], 0.5)),
        DockZone::Down => Some(PaneLayout::Horizontal(vec![existing, incoming], 0.5)),
        DockZone::Center => None,
    }
}

pub(crate) struct ReorderedTabGroups {
    pub(crate) groups: Vec<TabGroup>,
    pub(crate) active_group_id: String,
    pub(crate) insert_at: usize,
}

pub(crate) fn reorder_tab_groups(
    groups: &[TabGroup],
    selected_group_ids: &[String],
    index: usize,
) -> Option<ReorderedTabGroups> {
    let selected = selected_group_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if selected.is_empty() {
        return None;
    }

    let moving = groups
        .iter()
        .filter(|group| selected.contains(group.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let active_group_id = moving.first()?.id.clone();
    let mut remaining = groups
        .iter()
        .filter(|group| !selected.contains(group.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let insert_at = index.min(remaining.len());
    remaining.splice(insert_at..insert_at, moving);

    Some(ReorderedTabGroups {
        groups: remaining,
        active_group_id,
        insert_at,
    })
}

pub(crate) fn dock_tab_layout(
    layout: &mut PaneLayout,
    source_tab_id: &str,
    target_tab_id: &str,
    zone: DockZone,
) -> bool {
    if source_tab_id == target_tab_id || !zone.is_split() {
        return false;
    }
    if !layout.contains(source_tab_id) || !layout.contains(target_tab_id) {
        return false;
    }

    let original = layout.clone();
    if !layout.remove_tab(source_tab_id)
        || !dock_at(
            layout,
            target_tab_id,
            PaneLayout::Single(source_tab_id.to_string()),
            zone,
        )
    {
        *layout = original;
        return false;
    }
    true
}

fn dock_at(
    layout: &mut PaneLayout,
    target_tab_id: &str,
    source: PaneLayout,
    zone: DockZone,
) -> bool {
    match layout {
        PaneLayout::Single(id) if id == target_tab_id => {
            let current = PaneLayout::Single(id.clone());
            let Some(merged) = merge_pane_layout(source, current, zone) else {
                return false;
            };
            *layout = merged;
            true
        }
        PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => children
            .iter_mut()
            .any(|child| dock_at(child, target_tab_id, source.clone(), zone)),
        PaneLayout::Empty | PaneLayout::Single(_) => false,
    }
}

impl TinyShell {
    /// Schedule the active tab group to move into a new native window after
    /// the current input callback has released its window and entity borrows.
    pub(crate) fn detach_tab_to_new_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let group_id = self
            .workspace()
            .active_group_id()
            .map(str::to_owned)
            .filter(|group_id| {
                self.workspace()
                    .tab_groups()
                    .iter()
                    .any(|group| group.id == *group_id)
            })
            .or_else(|| {
                let active_tab = self.workspace().active_tab_id().map(str::to_owned)?;
                self.workspace()
                    .tab_groups()
                    .iter()
                    .find(|group| group.pane_root.tab_ids().contains(&active_tab.as_str()))
                    .map(|group| group.id.clone())
            });
        let Some(group_id) = group_id else {
            self.status = t!("cannot_detach_tab_group").into();
            cx.notify();
            return;
        };

        self.defer_group_detach(group_id, window, cx);
    }

    pub(crate) fn defer_group_detach(
        &mut self,
        group_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = cx.entity();
        window.defer(cx, move |_window, cx| {
            Self::detach_group_to_new_window(source, group_id, cx);
        });
    }

    pub(crate) fn defer_groups_detach(
        &mut self,
        group_ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if group_ids.len() <= 1 {
            if let Some(group_id) = group_ids.into_iter().next() {
                self.defer_group_detach(group_id, window, cx);
            }
            return;
        }
        let source = cx.entity();
        window.defer(cx, move |_window, cx| {
            Self::detach_groups_to_new_window(source, group_ids, cx);
        });
    }

    fn detach_groups_to_new_window(source: Entity<Self>, group_ids: Vec<String>, cx: &mut App) {
        let prepared = source.update(cx, |this, _| {
            let mut transfers = Vec::new();
            for group_id in &group_ids {
                match this.take_group_transfer(group_id) {
                    Ok(transfer) => transfers.push(transfer),
                    Err(message) => return Err((message, transfers)),
                }
            }
            Ok((
                transfers,
                this.session_owner_id,
                this.session_store.clone(),
                this.config_repository.clone(),
            ))
        });

        let (transfers, source_owner_id, session_store, config_repository) = match prepared {
            Ok(prepared) => prepared,
            Err((message, transfers)) => {
                source.update(cx, |this, cx| {
                    for transfer in transfers.into_iter().rev() {
                        this.restore_group_transfer(transfer, cx);
                    }
                    this.status = message.into();
                    cx.notify();
                });
                return;
            }
        };
        let transferred_tab_ids = transfers
            .iter()
            .flat_map(|transfer| transfer.tabs.iter())
            .map(|(_, tab)| tab.id.clone())
            .collect::<Vec<_>>();

        match super::startup::open_new_window_with_groups(
            transfers,
            source_owner_id,
            session_store,
            config_repository,
            cx,
        ) {
            Ok(()) => {
                source.update(cx, |this, cx| {
                    this.clear_transferred_remote_desktop_surfaces(&transferred_tab_ids);
                    this.status = t!("tab_groups_detached").into();
                    cx.notify();
                });
            }
            Err((message, transfers)) => {
                source.update(cx, |this, cx| {
                    for transfer in transfers.into_iter().rev() {
                        this.restore_group_transfer(transfer, cx);
                    }
                    this.status = t!("tab_group_detach_failed", error = message).into();
                    cx.notify();
                });
            }
        }
    }

    /// Detach a complete tab group to a new window without recreating its
    /// terminal or SFTP backends. Window creation and route handoff form the
    /// prepare step; any failure restores the original group in place.
    fn detach_group_to_new_window(source: Entity<Self>, group_id: String, cx: &mut App) {
        tracing::info!(group_id, "[tab-drag] preparing detached window");
        let prepared = source.update(cx, |this, _| {
            this.take_group_transfer(&group_id).map(|transfer| {
                (
                    transfer,
                    this.session_owner_id,
                    this.session_store.clone(),
                    this.config_repository.clone(),
                )
            })
        });

        let (transfer, source_owner_id, session_store, config_repository) = match prepared {
            Ok(prepared) => prepared,
            Err(message) => {
                source.update(cx, |this, cx| {
                    this.status = message.into();
                    cx.notify();
                });
                return;
            }
        };

        let moved_search_target = transfer.tabs.iter().any(|(_, tab)| {
            source.read(cx).window_state.search_target_tab.as_deref() == Some(tab.id.as_str())
        });
        let transferred_tab_ids = transfer
            .tabs
            .iter()
            .map(|(_, tab)| tab.id.clone())
            .collect::<Vec<_>>();
        let result = super::startup::open_new_window_with_group(
            transfer,
            source_owner_id,
            session_store,
            config_repository,
            cx,
        );

        source.update(cx, |this, cx| {
            match result {
                Ok(()) => {
                    tracing::info!(group_id, "[tab-drag] detached window opened");
                    this.clear_transferred_remote_desktop_surfaces(&transferred_tab_ids);
                    if moved_search_target {
                        this.window_state.search_target_tab = None;
                        this.window_state.search_matches.clear();
                        this.window_state.search_query.clear();
                        this.window_state.search_current = 0;
                    }
                    this.status = t!("tab_group_detached").into();
                }
                Err((message, transfer)) => {
                    tracing::warn!(group_id, %message, "[tab-drag] detached window failed");
                    this.restore_group_transfer(*transfer, cx);
                    this.status = t!("tab_group_detach_failed", error = message).into();
                }
            }
            cx.notify();
        });
    }

    pub(crate) fn move_group_to_window(
        &mut self,
        group_id: String,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        source_window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        let merged =
            self.commit_groups_merge(vec![group_id], target_window, target, DockZone::Center, cx);
        if should_close_empty_source(
            merged,
            self.workspace().tab_groups().is_empty(),
            &source_window,
            &target_window,
        ) {
            let source = cx.entity();
            Self::defer_close_empty_source_window(source_window, source, cx);
        }
    }

    pub(crate) fn move_active_group_to_adjacent_window(
        &mut self,
        source_window: AnyWindowHandle,
        reverse: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(group_id) = self.workspace().active_group_id().map(str::to_owned) else {
            return;
        };
        let mut targets = super::other_main_windows(source_window);
        if reverse {
            targets.reverse();
        }
        let Some((target_window, target)) = targets.into_iter().next() else {
            self.status = t!("tab_move_no_target_window").into();
            cx.notify();
            return;
        };
        self.move_group_to_window(group_id, target_window, target, source_window, cx);
    }

    pub(crate) fn merge_window_into(
        &mut self,
        source_window: AnyWindowHandle,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        cx: &mut Context<Self>,
    ) {
        let group_ids = self
            .workspace()
            .tab_groups()
            .iter()
            .map(|group| group.id.clone())
            .collect::<Vec<_>>();
        if group_ids.is_empty() {
            return;
        }
        let merged =
            self.commit_groups_merge(group_ids, target_window, target, DockZone::Center, cx);
        if merged && self.workspace().tab_groups().is_empty() {
            let source = cx.entity();
            Self::defer_close_empty_source_window(source_window, source, cx);
        }
    }

    pub(super) fn commit_groups_merge(
        &mut self,
        group_ids: Vec<String>,
        target_window: AnyWindowHandle,
        target: Entity<TinyShell>,
        zone: DockZone,
        cx: &mut Context<Self>,
    ) -> bool {
        if group_ids.is_empty() {
            return false;
        }

        let mut transfers = Vec::with_capacity(group_ids.len());
        for group_id in group_ids {
            match self.take_group_transfer(&group_id) {
                Ok(transfer) => transfers.push(transfer),
                Err(message) => {
                    for transfer in transfers.into_iter().rev() {
                        self.restore_group_transfer(transfer, cx);
                    }
                    self.status = message.into();
                    cx.notify();
                    return false;
                }
            }
        }

        let moved_search_target = self
            .window_state
            .search_target_tab
            .as_ref()
            .is_some_and(|id| {
                transfers
                    .iter()
                    .flat_map(|transfer| transfer.tabs.iter())
                    .any(|(_, tab)| tab.id == *id)
            });
        let transferred_tab_ids = transfers
            .iter()
            .flat_map(|transfer| transfer.tabs.iter())
            .map(|(_, tab)| tab.id.clone())
            .collect::<Vec<_>>();
        let source_owner_id = self.session_owner_id;
        let result = target.update(cx, |target, cx| {
            target.incoming_tab_drag = None;
            let result = target.receive_group_transfers(transfers, source_owner_id, zone, cx);
            cx.notify();
            result
        });

        match result {
            Ok(()) => {
                self.clear_transferred_remote_desktop_surfaces(&transferred_tab_ids);
                let focus_handle = target.read(cx).focus_handle.clone();
                super::activate_window_with_retry(target_window, focus_handle, cx);
                if moved_search_target {
                    self.window_state.search_target_tab = None;
                    self.window_state.search_matches.clear();
                    self.window_state.search_query.clear();
                    self.window_state.search_current = 0;
                }
                self.status = t!("tab_group_moved").into();
                cx.notify();
                true
            }
            Err((message, transfers)) => {
                for transfer in transfers.into_iter().rev() {
                    self.restore_group_transfer(transfer, cx);
                }
                self.status = t!("tab_group_move_failed", error = message).into();
                cx.notify();
                false
            }
        }
    }

    fn handoff_route_ids(
        &mut self,
        route_ids: &[String],
        source_owner_id: crate::session::store::WindowOwnerId,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        let target_owner_id = self.session_owner_id;
        if self.session_store.update(cx, |store, _| {
            store.move_event_routes(route_ids, source_owner_id, target_owner_id)
        }) {
            Ok(())
        } else {
            Err("backend event routes changed before the move could commit")
        }
    }

    /// Receive an intact group from another window without recreating any
    /// terminal or SFTP backend. The group remains a separate top-level tab
    /// because `TabGroup` owns a single SFTP UI state.
    pub(crate) fn receive_group_transfer(
        &mut self,
        transfer: GroupTransfer,
        source_owner_id: crate::session::store::WindowOwnerId,
        cx: &mut Context<Self>,
    ) -> Result<(), (String, Box<GroupTransfer>)> {
        self.receive_single_group_transfer(transfer, source_owner_id, DockZone::Center, cx)
    }

    fn receive_single_group_transfer(
        &mut self,
        transfer: GroupTransfer,
        source_owner_id: crate::session::store::WindowOwnerId,
        zone: DockZone,
        cx: &mut Context<Self>,
    ) -> Result<(), (String, Box<GroupTransfer>)> {
        match self.receive_group_transfers(vec![transfer], source_owner_id, zone, cx) {
            Ok(()) => Ok(()),
            Err((message, transfers)) => {
                let Some(transfer) = transfers.into_iter().next() else {
                    unreachable!("a failed single-group transfer must return its payload");
                };
                Err((message, Box::new(transfer)))
            }
        }
    }

    pub(super) fn receive_group_transfers(
        &mut self,
        transfers: Vec<GroupTransfer>,
        source_owner_id: crate::session::store::WindowOwnerId,
        requested_zone: DockZone,
        cx: &mut Context<Self>,
    ) -> Result<(), (String, Vec<GroupTransfer>)> {
        if transfers.is_empty() {
            return Err((
                "cannot receive an empty transfer batch".to_string(),
                transfers,
            ));
        }
        let mut zone = effective_batch_drop_zone(transfers.len(), requested_zone);
        if self.workspace().active_group_id().is_none() {
            zone = DockZone::Center;
        }
        let manifests = transfers
            .iter()
            .map(GroupTransfer::manifest)
            .collect::<Vec<_>>();
        let target = self.transfer_target_manifest();
        if let Err(message) =
            validate_transfer_batch_for_target(&manifests, &target, !zone.is_split())
        {
            return Err((message.to_string(), transfers));
        }

        let merged_layout = if zone.is_split() {
            merge_pane_layout(
                transfers[0].group.pane_root.clone(),
                self.workspace().pane_root().clone(),
                zone,
            )
        } else {
            None
        };
        if zone.is_split() && merged_layout.is_none() {
            return Err((
                "cannot merge the transferred pane layout".to_string(),
                transfers,
            ));
        }

        let route_ids = manifests
            .iter()
            .flat_map(|manifest| manifest.route_ids.iter().cloned())
            .collect::<Vec<_>>();
        if let Err(message) = self.handoff_route_ids(&route_ids, source_owner_id, cx) {
            return Err((message.to_string(), transfers));
        }

        if let Some(merged_layout) = merged_layout {
            let Some(transfer) = transfers.into_iter().next() else {
                unreachable!("a split transfer must contain exactly one group");
            };
            self.commit_received_docked(transfer, merged_layout, cx);
        } else {
            for transfer in transfers {
                self.commit_received_group(transfer, cx);
            }
        }
        Ok(())
    }

    fn transfer_target_manifest(&self) -> TransferTargetManifest {
        TransferTargetManifest {
            group_ids: self
                .workspace()
                .tab_groups()
                .iter()
                .map(|group| group.id.clone())
                .collect(),
            tab_ids: self
                .workspace()
                .tabs()
                .iter()
                .map(|tab| tab.id.clone())
                .collect(),
            sftp_handle_ids: self.sftp_handles.keys().cloned().collect(),
        }
    }

    fn commit_received_group(&mut self, transfer: GroupTransfer, cx: &mut Context<Self>) {
        let chosen_active_tab = transfer.chosen_active_tab();
        let GroupTransfer {
            group,
            mut tabs,
            sftp_handles,
            system_info_tabs,
            active_system_info_tab,
            ..
        } = transfer;
        let pane_order = group.pane_root.tab_ids();
        tabs.sort_by_key(|(_, tab)| {
            pane_order
                .iter()
                .position(|id| *id == tab.id)
                .unwrap_or(usize::MAX)
        });
        let group_id = self.create_group_for_transfer(group, cx);
        self.adopt_transferred_tabs(
            group_id,
            tabs,
            sftp_handles,
            chosen_active_tab,
            system_info_tabs,
            active_system_info_tab,
            cx,
        );
    }

    fn commit_received_docked(
        &mut self,
        transfer: GroupTransfer,
        merged_layout: PaneLayout,
        cx: &mut Context<Self>,
    ) {
        let chosen_active_tab = transfer.chosen_active_tab();
        let GroupTransfer {
            tabs,
            sftp_handles,
            system_info_tabs,
            active_system_info_tab,
            ..
        } = transfer;
        self.workspace_state_mut()
            .set_pane_root(merged_layout.clone());
        if let Some(group_id) = self.workspace().active_group_id().map(str::to_owned)
            && let Some(target_group) = self.tab_group_mut(&group_id)
        {
            target_group.pane_root = merged_layout;
        }
        self.workspace_state_mut()
            .append_tabs(tabs.into_iter().map(|(_, tab)| tab).collect());
        self.sftp_handles.extend(sftp_handles);
        let restored_info = super::terminal_workspace::restore_system_info_transfer(
            self.workspace_state_mut().system_info_tabs_mut(),
            system_info_tabs,
            active_system_info_tab,
        );
        self.workspace_state_mut()
            .set_active_system_info_tab(restored_info);
        if let Some(active_tab) = chosen_active_tab {
            self.workspace_state_mut()
                .set_active_tab(Some(active_tab.clone()));
            self.focus_pane_with_id(active_tab);
        }
        self.home_page_open = false;
        self.reset_sftp_tree_for_active_group();
        self.sync_system_tab_to_active_group();
        cx.notify();
    }

    pub(super) fn take_group_transfer(&mut self, group_id: &str) -> Result<GroupTransfer, String> {
        let transfer = self.prepare_group_for_transfer(group_id)?;
        self.clear_transferred_group(transfer.group_index);
        Ok(transfer)
    }

    fn prepare_group_for_transfer(&mut self, group_id: &str) -> Result<GroupTransfer, String> {
        let group_index = self
            .workspace()
            .tab_groups()
            .iter()
            .position(|group| group.id == group_id)
            .ok_or_else(|| "cannot move: source group no longer exists".to_string())?;
        let group = self.workspace_state_mut().tab_groups()[group_index].clone();
        let tab_ids = group
            .pane_root
            .tab_ids()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if tab_ids.is_empty() || tab_ids.iter().any(String::is_empty) {
            return Err("cannot move: source group has no terminal panes".to_string());
        }
        let tab_id_set = tab_ids.iter().cloned().collect::<HashSet<_>>();
        if tab_id_set.len() != tab_ids.len() {
            return Err("cannot move: source group contains duplicate terminal ids".to_string());
        }
        if tab_ids.iter().any(|tab_id| {
            !self
                .workspace_state_mut()
                .tabs()
                .iter()
                .any(|tab| tab.id == *tab_id)
        }) {
            return Err("cannot move: a source terminal no longer exists".to_string());
        }

        let was_active_group =
            self.workspace_state_mut().active_group_value().as_deref() == Some(group_id);
        let active_tab = self
            .workspace()
            .active_tab_id()
            .map(str::to_owned)
            .filter(|tab_id| tab_id_set.contains(tab_id.as_str()));
        let mut tabs = Vec::with_capacity(tab_ids.len());
        let mut remaining_tabs = Vec::with_capacity(self.workspace().tabs().len() - tab_ids.len());
        for (index, tab) in self
            .workspace_state_mut()
            .take_tabs()
            .into_iter()
            .enumerate()
        {
            if tab_id_set.contains(tab.id.as_str()) {
                tabs.push((index, tab));
            } else {
                remaining_tabs.push(tab);
            }
        }
        self.workspace_state_mut().replace_tabs(remaining_tabs);

        let mut sftp_handles = HashMap::new();
        let handle_ids = self
            .sftp_handles
            .keys()
            .filter(|id| id.as_str() == group_id || tab_id_set.contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        for handle_id in handle_ids {
            if let Some(handle) = self.sftp_handles.remove(&handle_id) {
                sftp_handles.insert(handle_id, handle);
            }
        }

        let mut route_ids = tab_ids.clone();
        route_ids.extend(sftp_handles.keys().cloned());
        route_ids.sort();
        route_ids.dedup();

        let (system_info_tabs, active_system_info_tab) =
            super::terminal_workspace::extract_system_info_transfer(
                self.workspace().system_info_tabs(),
                self.workspace().active_system_info_tab_id(),
                &tab_ids,
            );

        Ok(GroupTransfer {
            group,
            group_index,
            tabs,
            sftp_handles,
            route_ids,
            active_tab,
            system_info_tabs,
            active_system_info_tab,
            was_active_group,
        })
    }

    fn clear_transferred_group(&mut self, group_index: usize) {
        let active_group_id = self.workspace().active_group_id().map(str::to_owned);
        let group_id = self.workspace().tab_groups()[group_index].id.clone();
        let was_active_group = active_group_id.as_deref() == Some(group_id.as_str());
        let transferred_tab_ids = self.workspace().tab_groups()[group_index]
            .pane_root
            .tab_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let removed_active_info =
            self.workspace()
                .active_system_info_tab_id()
                .is_some_and(|id| {
                    self.workspace().system_info_tabs().iter().any(|info| {
                        info.id == id && transferred_tab_ids.contains(&info.source_tab_id)
                    })
                });
        self.workspace_state_mut()
            .system_info_tabs_mut()
            .retain(|info| !transferred_tab_ids.contains(&info.source_tab_id));
        if removed_active_info {
            self.workspace_state_mut().clear_active_system_info_tab();
        }
        self.workspace_state_mut().remove_group_at(group_index);
        if was_active_group {
            self.activate_after_group_extraction(group_index);
        } else {
            self.sync_system_tab_to_active_group();
        }
    }

    fn clear_transferred_remote_desktop_surfaces(&mut self, tab_ids: &[String]) {
        for tab_id in tab_ids {
            self.remote_desktop_surfaces.remove(tab_id);
        }
    }

    fn activate_after_group_extraction(&mut self, removed_index: usize) {
        if self.workspace_state_mut().tab_groups().is_empty() {
            self.workspace_state_mut().set_pane_root(PaneLayout::Empty);
            self.workspace_state_mut().clear_focused_pane_path();
            self.workspace_state_mut().set_active_tab(None);
            self.workspace_state_mut().set_active_group(None);
            self.reset_sftp_tree_for_active_group();
            self.home_page_open = true;
            self.sync_system_tab_to_active_group();
            return;
        }

        let next_index = removed_index.min(self.workspace_state_mut().tab_groups().len() - 1);
        let next_group = &self.workspace_state_mut().tab_groups()[next_index];
        let next_group_id = next_group.id.clone();
        let next_layout = next_group.pane_root.clone();
        let next_tab = next_layout.tab_ids().first().copied().map(str::to_string);
        self.workspace_state_mut()
            .set_active_group(Some(next_group_id));
        self.workspace_state_mut().set_pane_root(next_layout);
        self.workspace_state_mut().clear_focused_pane_path();
        self.workspace_state_mut().set_active_tab(next_tab.clone());
        if let Some(tab_id) = next_tab {
            self.focus_pane_with_id(tab_id);
        }
        self.reset_sftp_tree_for_active_group();
        self.sync_system_tab_to_active_group();
    }

    pub(super) fn restore_group_transfer(
        &mut self,
        mut transfer: GroupTransfer,
        cx: &mut Context<Self>,
    ) {
        let owner_id = self.session_owner_id;
        self.session_store.update(cx, |store, _| {
            if !store.move_event_routes(&transfer.route_ids, owner_id, owner_id) {
                for route_id in &transfer.route_ids {
                    store.register_event_route(route_id.clone(), owner_id);
                }
            }
        });

        let group_index = transfer
            .group_index
            .min(self.workspace_state_mut().tab_groups().len());
        let group_id = transfer.group.id.clone();
        let group_layout = transfer.group.pane_root.clone();
        let system_info_tabs = transfer.system_info_tabs.clone();
        let active_system_info_tab = transfer.active_system_info_tab.clone();
        self.workspace_state_mut()
            .insert_group(group_index, transfer.group);
        transfer.tabs.sort_by_key(|(index, _)| *index);
        for (index, tab) in transfer.tabs {
            let insert_at = index.min(self.workspace().tabs().len());
            self.workspace_state_mut().insert_tab(insert_at, tab);
        }
        self.sftp_handles.extend(transfer.sftp_handles);

        let restored_active_info = super::terminal_workspace::restore_system_info_transfer(
            self.workspace_state_mut().system_info_tabs_mut(),
            system_info_tabs,
            active_system_info_tab,
        );

        if transfer.was_active_group {
            self.workspace_state_mut().set_active_group(Some(group_id));
            self.workspace_state_mut().set_pane_root(group_layout);
            self.workspace_state_mut().clear_focused_pane_path();
            self.workspace_state_mut()
                .set_active_system_info_tab(restored_active_info);
            let active_tab = transfer.active_tab.or_else(|| {
                self.workspace()
                    .pane_root()
                    .tab_ids()
                    .first()
                    .copied()
                    .map(str::to_string)
            });
            self.workspace_state_mut().set_active_tab(active_tab);
            if let Some(tab_id) = self.workspace_state_mut().active_tab_value() {
                self.focus_pane_with_id(tab_id);
            }
            self.reset_sftp_tree_for_active_group();
        }
        self.sync_system_tab_to_active_group();
        cx.notify();
    }

    pub(super) fn create_group_for_transfer(
        &mut self,
        group: TabGroup,
        _cx: &mut Context<Self>,
    ) -> String {
        let group_id = group.id.clone();
        let pane_root = group.pane_root.clone();
        let ordinal = group.ordinal;
        let next_ordinal = self.workspace().next_tab_group_ordinal().max(ordinal + 1);
        self.workspace_state_mut()
            .set_next_tab_group_ordinal(next_ordinal);
        self.workspace_state_mut().push_group(group);
        self.home_page_open = false;
        self.workspace_state_mut().clear_active_system_info_tab();
        self.workspace_state_mut()
            .set_active_group(Some(group_id.clone()));
        self.workspace_state_mut().set_pane_root(pane_root);
        self.workspace_state_mut().clear_focused_pane_path();
        self.reset_sftp_tree_for_active_group();
        group_id
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn adopt_transferred_tabs(
        &mut self,
        _group_id: String,
        tabs: Vec<(usize, TerminalTab)>,
        sftp_handles: HashMap<String, crate::sftp::SftpHandle>,
        active_tab: Option<String>,
        system_info_tabs: Vec<SystemInfoTab>,
        active_system_info_tab: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.workspace_state_mut()
            .append_tabs(tabs.into_iter().map(|(_, tab)| tab).collect());
        self.sftp_handles.extend(sftp_handles);
        let restored_active_info = super::terminal_workspace::restore_system_info_transfer(
            self.workspace_state_mut().system_info_tabs_mut(),
            system_info_tabs,
            active_system_info_tab,
        );
        self.workspace_state_mut()
            .set_active_system_info_tab(restored_active_info);
        self.workspace_state_mut().set_active_tab(active_tab);
        if let Some(tab_id) = self.workspace_state_mut().active_tab_value() {
            self.focus_pane_with_id(tab_id);
        }
        self.sync_system_tab_to_active_group();
        self.status = t!("tab_group_received").into();
        cx.notify();
    }

    pub(super) fn reorder_tab_groups(
        &mut self,
        group_ids: &[String],
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(reordered) = reorder_tab_groups(self.workspace().tab_groups(), group_ids, index)
        else {
            self.status = t!("cannot_reorder_tab_group").into();
            cx.notify();
            return;
        };
        *self.workspace_state_mut().tab_groups_mut() = reordered.groups;
        self.activate_group(reordered.active_group_id, window, cx);
        self.tabs_scroll_handle.scroll_to_item(reordered.insert_at);
        self.status = t!("tab_group_reordered").into();
        window.activate_window();
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    pub(crate) fn dock_pane(
        &mut self,
        group_id: &str,
        source_tab_id: &str,
        target_tab_id: &str,
        zone: DockZone,
        cx: &mut Context<Self>,
    ) {
        if self.workspace().active_group_id() != Some(group_id) {
            return;
        }
        let mut layout = self.workspace().pane_root().clone();
        if dock_tab_layout(&mut layout, source_tab_id, target_tab_id, zone) {
            self.workspace_state_mut().set_pane_root(layout.clone());
            if let Some(group) = self.tab_group_mut(group_id) {
                group.pane_root = layout;
            }
            self.workspace_state_mut()
                .set_active_tab(Some(source_tab_id.to_string()));
            self.focus_pane_with_id(source_tab_id.to_string());
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        TransferManifest, TransferTargetManifest, dock_tab_layout, effective_batch_drop_zone,
        merge_pane_layout, reorder_tab_groups, validate_transfer_batch_for_target,
        validate_transfer_layout, validate_transfer_manifests, validate_transfer_target_manifest,
    };
    use crate::{PaneLayout, TabGroup, app::tab_drag::DockZone};

    fn leaf(id: &str) -> PaneLayout {
        PaneLayout::Single(id.to_string())
    }

    fn group(id: &str) -> TabGroup {
        TabGroup {
            id: id.to_string(),
            drag_id: 0,
            ordinal: 0,
            title: id.to_string(),
            pane_root: leaf(id),
            sftp: None,
        }
    }

    fn manifest(group_id: &str, tab_ids: &[&str]) -> TransferManifest {
        TransferManifest {
            group_id: group_id.to_string(),
            layout_tab_ids: tab_ids.iter().map(|id| (*id).to_string()).collect(),
            tab_ids: tab_ids.iter().map(|id| (*id).to_string()).collect(),
            sftp_handle_ids: Vec::new(),
            route_ids: tab_ids.iter().map(|id| (*id).to_string()).collect(),
        }
    }

    #[test]
    fn transfer_layout_requires_an_exact_unique_tab_set() {
        let layout = PaneLayout::Horizontal(vec![leaf("a"), leaf("b")], 0.5);

        assert_eq!(validate_transfer_layout(&layout, ["a", "b"]), Ok(()));
        assert!(validate_transfer_layout(&layout, ["a"]).is_err());
        assert!(validate_transfer_layout(&layout, ["a", "b", "c"]).is_err());
        assert!(validate_transfer_layout(&layout, ["a", "a"]).is_err());
    }

    #[test]
    fn transfer_layout_rejects_duplicate_and_empty_layouts() {
        let duplicate = PaneLayout::Vertical(vec![leaf("a"), leaf("a")], 0.5);

        assert!(validate_transfer_layout(&duplicate, ["a"]).is_err());
        assert!(validate_transfer_layout(&PaneLayout::Empty, []).is_err());
    }

    #[test]
    fn batch_validation_rejects_cross_group_identity_collisions() {
        let valid = [manifest("g1", &["a"]), manifest("g2", &["b"])];
        assert_eq!(validate_transfer_manifests(&valid), Ok(()));

        let duplicate_group = [manifest("g1", &["a"]), manifest("g1", &["b"])];
        assert_eq!(
            validate_transfer_manifests(&duplicate_group),
            Err("transfer batch contains duplicate groups")
        );

        let duplicate_tab = [manifest("g1", &["a"]), manifest("g2", &["a"])];
        assert_eq!(
            validate_transfer_manifests(&duplicate_tab),
            Err("transfer batch contains duplicate terminals")
        );
    }

    #[test]
    fn batch_validation_checks_layout_handles_and_routes_before_window_creation() {
        let mut invalid_layout = manifest("g1", &["a"]);
        invalid_layout.layout_tab_ids.push("extra".to_string());
        assert_eq!(
            validate_transfer_manifests(&[invalid_layout]),
            Err("transfer payload does not match the group layout")
        );

        let mut first = manifest("g1", &["a"]);
        let mut second = manifest("g2", &["b"]);
        first.sftp_handle_ids.push("shared".to_string());
        second.sftp_handle_ids.push("shared".to_string());
        assert_eq!(
            validate_transfer_manifests(&[first, second]),
            Err("transfer batch contains duplicate SFTP handles")
        );

        let first = manifest("g1", &["a"]);
        let mut second = manifest("g2", &["b"]);
        second.route_ids.push("a".to_string());
        assert_eq!(
            validate_transfer_manifests(&[first, second]),
            Err("transfer batch contains duplicate backend routes")
        );
    }

    #[test]
    fn target_validation_distinguishes_group_merge_from_pane_docking() {
        let transfer = manifest("group", &["incoming"]);
        let target = TransferTargetManifest {
            group_ids: HashSet::from(["group".to_string()]),
            tab_ids: HashSet::new(),
            sftp_handle_ids: HashSet::new(),
        };

        assert_eq!(
            validate_transfer_target_manifest(&transfer, &target, true),
            Err("target already contains this group")
        );
        assert_eq!(
            validate_transfer_target_manifest(&transfer, &target, false),
            Ok(())
        );
    }

    #[test]
    fn target_validation_rejects_terminal_and_sftp_collisions() {
        let mut transfer = manifest("group", &["incoming"]);
        transfer.sftp_handle_ids.push("sftp".to_string());

        let terminal_collision = TransferTargetManifest {
            group_ids: HashSet::new(),
            tab_ids: HashSet::from(["incoming".to_string()]),
            sftp_handle_ids: HashSet::new(),
        };
        assert_eq!(
            validate_transfer_target_manifest(&transfer, &terminal_collision, true),
            Err("target already contains one of the transferred terminals")
        );

        let sftp_collision = TransferTargetManifest {
            group_ids: HashSet::new(),
            tab_ids: HashSet::new(),
            sftp_handle_ids: HashSet::from(["sftp".to_string()]),
        };
        assert_eq!(
            validate_transfer_target_manifest(&transfer, &sftp_collision, true),
            Err("target already contains one of the transferred SFTP handles")
        );
    }

    #[test]
    fn batch_target_validation_rejects_every_conflict_before_commit() {
        let transfers = [manifest("first", &["a"]), manifest("second", &["b"])];
        let target = TransferTargetManifest {
            group_ids: HashSet::from(["second".to_string()]),
            tab_ids: HashSet::new(),
            sftp_handle_ids: HashSet::new(),
        };

        assert_eq!(
            validate_transfer_batch_for_target(&transfers, &target, true),
            Err("target already contains this group")
        );
        assert_eq!(
            validate_transfer_batch_for_target(&transfers, &target, false),
            Ok(())
        );
    }

    #[test]
    fn multiple_groups_ignore_split_zones_to_preserve_group_state() {
        assert_eq!(effective_batch_drop_zone(1, DockZone::Left), DockZone::Left);
        assert_eq!(
            effective_batch_drop_zone(2, DockZone::Left),
            DockZone::Center
        );
        assert_eq!(
            effective_batch_drop_zone(3, DockZone::Down),
            DockZone::Center
        );
    }

    #[test]
    fn merge_pane_layout_preserves_zone_order() {
        let left = merge_pane_layout(leaf("incoming"), leaf("existing"), DockZone::Left)
            .expect("left is a split zone");
        let down = merge_pane_layout(leaf("incoming"), leaf("existing"), DockZone::Down)
            .expect("down is a split zone");

        assert_eq!(left.tab_ids(), vec!["incoming", "existing"]);
        assert!(matches!(left, PaneLayout::Vertical(_, _)));
        assert_eq!(down.tab_ids(), vec!["existing", "incoming"]);
        assert!(matches!(down, PaneLayout::Horizontal(_, _)));
        assert!(merge_pane_layout(leaf("a"), leaf("b"), DockZone::Center).is_none());
    }

    #[test]
    fn docking_moves_a_source_leaf_without_losing_nested_panes() {
        let mut layout = PaneLayout::Horizontal(
            vec![
                leaf("source"),
                PaneLayout::Vertical(vec![leaf("a"), leaf("target")], 0.5),
            ],
            0.5,
        );

        assert!(dock_tab_layout(
            &mut layout,
            "source",
            "target",
            DockZone::Right
        ));
        assert_eq!(layout.tab_ids(), vec!["a", "target", "source"]);
    }

    #[test]
    fn failed_docking_leaves_the_original_layout_unchanged() {
        let mut layout = PaneLayout::Horizontal(vec![leaf("source"), leaf("target")], 0.5);
        let original_ids = layout
            .tab_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();

        assert!(!dock_tab_layout(
            &mut layout,
            "source",
            "missing",
            DockZone::Left
        ));
        assert_eq!(layout.tab_ids(), original_ids);
        assert!(!dock_tab_layout(
            &mut layout,
            "source",
            "target",
            DockZone::Center
        ));
        assert_eq!(layout.tab_ids(), original_ids);
    }

    #[test]
    fn reorder_keeps_selected_groups_in_workspace_order() {
        let groups = [group("a"), group("b"), group("c"), group("d")];
        let selected = vec!["d".to_string(), "b".to_string()];

        let reordered = reorder_tab_groups(&groups, &selected, 0).expect("groups are selected");
        let ids = reordered
            .groups
            .iter()
            .map(|group| group.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["b", "d", "a", "c"]);
        assert_eq!(reordered.active_group_id, "b");
        assert_eq!(reordered.insert_at, 0);
    }

    #[test]
    fn reorder_clamps_the_insertion_index_and_rejects_unknown_selection() {
        let groups = [group("a"), group("b"), group("c")];
        let reordered =
            reorder_tab_groups(&groups, &["b".to_string()], usize::MAX).expect("b exists");
        let ids = reordered
            .groups
            .iter()
            .map(|group| group.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["a", "c", "b"]);
        assert_eq!(reordered.insert_at, 2);
        assert!(reorder_tab_groups(&groups, &["missing".to_string()], 0).is_none());
        assert!(reorder_tab_groups(&groups, &[], 0).is_none());
    }
}
