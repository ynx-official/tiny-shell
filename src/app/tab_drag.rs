use gpui::{Bounds, Pixels, Point, Size, px};

pub(crate) const TAB_DRAG_THRESHOLD: f32 = 10.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DockZone {
    #[default]
    Center,
    Left,
    Right,
    Up,
    Down,
}

impl DockZone {
    pub(crate) fn is_split(self) -> bool {
        !matches!(self, Self::Center)
    }
}

pub(crate) enum DropIntent {
    None,
    Cancelled,
    Reorder { group_id: String, index: usize },
    Detach { group_id: String },
}

#[derive(Default)]
pub(crate) struct TabDragState {
    pending_group: Option<String>,
    start: Option<Point<Pixels>>,
    dragging_group: Option<String>,
    reorder_index: Option<usize>,
    outside: bool,
    selected_groups: Vec<String>,
}

impl TabDragState {
    pub(crate) fn begin(&mut self, group_id: String, position: Point<Pixels>) {
        self.begin_with_selection(group_id, position, false);
    }

    pub(crate) fn begin_with_selection(
        &mut self,
        group_id: String,
        position: Point<Pixels>,
        additive: bool,
    ) {
        self.reset_drag_target();
        if additive {
            if self.selected_groups.iter().any(|id| id == &group_id) {
                // Keep an already selected group selected so it can be the drag anchor.
            } else {
                self.selected_groups.push(group_id.clone());
            }
        } else {
            self.selected_groups.clear();
            self.selected_groups.push(group_id.clone());
        }
        self.pending_group = Some(group_id);
        self.start = Some(position);
    }

    pub(crate) fn promote_if_needed(&mut self, position: Point<Pixels>, threshold: f32) -> bool {
        if self.dragging_group.is_some() {
            return false;
        }
        let (Some(start), Some(group_id)) = (self.start, self.pending_group.as_ref()) else {
            return false;
        };
        let dx: f32 = (position.x - start.x).into();
        let dy: f32 = (position.y - start.y).into();
        if (dx * dx + dy * dy).sqrt() <= threshold {
            return false;
        }
        self.dragging_group = Some(group_id.clone());
        self.pending_group = None;
        true
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.dragging_group.is_some()
    }

    pub(crate) fn dragging_group(&self) -> Option<&str> {
        self.dragging_group.as_deref()
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.pending_group.is_some()
    }

    pub(crate) fn is_selected(&self, group_id: &str) -> bool {
        self.selected_groups.iter().any(|id| id == group_id)
    }

    pub(crate) fn selected_count(&self) -> usize {
        self.selected_groups.len()
    }

    pub(crate) fn ordered_drag_groups(
        &self,
        anchor: &str,
        ordered_groups: impl IntoIterator<Item = String>,
    ) -> Vec<String> {
        let mut groups = ordered_groups
            .into_iter()
            .filter(|id| self.selected_groups.iter().any(|selected| selected == id))
            .collect::<Vec<_>>();
        if groups.is_empty() {
            groups.push(anchor.to_string());
        }
        groups
    }

    pub(crate) fn set_reorder_index(&mut self, index: Option<usize>) -> bool {
        if self.reorder_index == index {
            return false;
        }
        self.reorder_index = index;
        true
    }

    pub(crate) fn reorder_index(&self) -> Option<usize> {
        self.reorder_index
    }

    pub(crate) fn outside(&self) -> bool {
        self.outside
    }

    pub(crate) fn set_outside(&mut self, outside: bool) -> bool {
        if self.outside == outside {
            return false;
        }
        self.outside = outside;
        true
    }

    pub(crate) fn finish(&mut self) -> DropIntent {
        let Some(group_id) = self.dragging_group.take() else {
            self.reset_drag_target();
            return DropIntent::None;
        };
        let reorder_index = self.reorder_index;
        let outside = self.outside;
        self.reset_drag_target();

        if let Some(index) = reorder_index {
            return DropIntent::Reorder { group_id, index };
        }
        if outside {
            return DropIntent::Detach { group_id };
        }
        DropIntent::Cancelled
    }

    /// Cancel the current gesture while intentionally preserving multi-selection.
    pub(crate) fn cancel(&mut self) {
        self.reset_drag_target();
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selected_groups.clear();
    }

    fn reset_drag_target(&mut self) {
        self.pending_group = None;
        self.start = None;
        self.dragging_group = None;
        self.reorder_index = None;
        self.outside = false;
    }
}

pub(crate) fn dock_zone_at(
    cursor: Point<Pixels>,
    bounds: Bounds<Pixels>,
    tab_bar_bounds: Option<Bounds<Pixels>>,
) -> Option<DockZone> {
    if tab_bar_bounds.is_some_and(|tab_bar| tab_bar.contains(&cursor)) {
        return Some(DockZone::Center);
    }
    if !bounds.contains(&cursor) {
        return None;
    }

    let x = (cursor.x - bounds.origin.x).as_f32() / bounds.size.width.as_f32().max(1.0);
    let y = (cursor.y - bounds.origin.y).as_f32() / bounds.size.height.as_f32().max(1.0);
    const EDGE: f32 = 0.28;
    if x < EDGE {
        Some(DockZone::Left)
    } else if x > 1.0 - EDGE {
        Some(DockZone::Right)
    } else if y < EDGE {
        Some(DockZone::Up)
    } else if y > 1.0 - EDGE {
        Some(DockZone::Down)
    } else {
        Some(DockZone::Center)
    }
}

pub(crate) fn should_close_empty_source<I: PartialEq>(
    merge_succeeded: bool,
    source_is_empty: bool,
    source_window: &I,
    target_window: &I,
) -> bool {
    merge_succeeded && source_is_empty && source_window != target_window
}

pub(crate) fn cursor_inside_viewport(cursor: Point<Pixels>, viewport_size: Size<Pixels>) -> bool {
    cursor.x >= px(0.)
        && cursor.y >= px(0.)
        && cursor.x < viewport_size.width
        && cursor.y < viewport_size.height
}

pub(crate) fn should_offer_detach(
    group_count: usize,
    cursor: Point<Pixels>,
    tab_bar_bounds: Option<Bounds<Pixels>>,
    has_merge_target: bool,
) -> bool {
    let inside_tab_bar = tab_bar_bounds.is_some_and(|bounds| bounds.contains(&cursor));
    group_count > 1 && !inside_tab_bar && !has_merge_target
}

pub(crate) fn reorder_index_at_x(
    dragged_group_id: &str,
    cursor_x: Pixels,
    ordered_bounds: &[(String, Bounds<Pixels>)],
) -> Option<usize> {
    if !ordered_bounds
        .iter()
        .any(|(group_id, _)| group_id == dragged_group_id)
    {
        return None;
    }

    let remaining = ordered_bounds
        .iter()
        .filter(|(group_id, _)| group_id != dragged_group_id)
        .collect::<Vec<_>>();
    Some(
        remaining
            .iter()
            .position(|(_, bounds)| cursor_x < bounds.origin.x + bounds.size.width / 2.0)
            .unwrap_or(remaining.len()),
    )
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::{
        DockZone, DropIntent, TabDragState, cursor_inside_viewport, dock_zone_at,
        reorder_index_at_x, should_close_empty_source, should_offer_detach,
    };

    #[test]
    fn drag_starts_only_after_threshold() {
        let mut state = TabDragState::default();
        state.begin("group-a".into(), point(px(10.), px(10.)));
        assert!(!state.promote_if_needed(point(px(13.), px(14.)), 5.0));
        assert!(state.promote_if_needed(point(px(16.), px(10.)), 5.0));
        assert!(state.is_dragging());
    }

    #[test]
    fn additive_selection_is_preserved_in_visual_order() {
        let mut state = TabDragState::default();
        state.begin_with_selection("group-b".into(), point(px(0.), px(0.)), false);
        state.begin_with_selection("group-a".into(), point(px(0.), px(0.)), true);
        assert_eq!(
            state.ordered_drag_groups(
                "group-a",
                ["group-a", "group-b", "group-c"].map(str::to_string),
            ),
            vec!["group-a".to_string(), "group-b".to_string()]
        );
    }

    #[test]
    fn cancelling_drag_keeps_multi_selection() {
        let mut state = TabDragState::default();
        state.begin_with_selection("a".into(), point(px(0.), px(0.)), false);
        state.begin_with_selection("b".into(), point(px(0.), px(0.)), true);
        state.cancel();
        assert_eq!(state.selected_count(), 2);
    }

    #[test]
    fn invalid_release_cancels_without_detaching() {
        let mut state = TabDragState::default();
        state.begin("group-a".into(), point(px(0.), px(0.)));
        state.promote_if_needed(point(px(10.), px(0.)), 5.0);
        assert!(matches!(state.finish(), DropIntent::Cancelled));
    }

    #[test]
    fn tab_bar_release_reorders_group() {
        let mut state = TabDragState::default();
        state.begin("group-c".into(), point(px(0.), px(0.)));
        state.promote_if_needed(point(px(10.), px(0.)), 5.0);
        state.set_reorder_index(Some(0));
        state.set_outside(true);
        assert!(matches!(
            state.finish(),
            DropIntent::Reorder { group_id, index: 0 } if group_id == "group-c"
        ));
    }

    #[test]
    fn docking_zone_prefers_edges_and_tab_bar() {
        let bounds = Bounds::new(point(px(0.), px(40.)), size(px(1000.), px(600.)));
        let tab_bar = Bounds::new(point(px(0.), px(0.)), size(px(1000.), px(40.)));
        assert_eq!(
            dock_zone_at(point(px(10.), px(300.)), bounds, Some(tab_bar)),
            Some(DockZone::Left)
        );
        assert_eq!(
            dock_zone_at(point(px(990.), px(300.)), bounds, Some(tab_bar)),
            Some(DockZone::Right)
        );
        assert_eq!(
            dock_zone_at(point(px(500.), px(60.)), bounds, Some(tab_bar)),
            Some(DockZone::Up)
        );
        assert_eq!(
            dock_zone_at(point(px(500.), px(630.)), bounds, Some(tab_bar)),
            Some(DockZone::Down)
        );
        assert_eq!(
            dock_zone_at(point(px(500.), px(300.)), bounds, Some(tab_bar)),
            Some(DockZone::Center)
        );
        assert_eq!(
            dock_zone_at(point(px(500.), px(20.)), bounds, Some(tab_bar)),
            Some(DockZone::Center)
        );
    }

    #[test]
    fn cursor_position_computes_left_and_right_reorder_indices() {
        let bounds = vec![
            (
                "group-a".to_string(),
                Bounds::new(point(px(0.), px(0.)), size(px(100.), px(32.))),
            ),
            (
                "group-b".to_string(),
                Bounds::new(point(px(100.), px(0.)), size(px(100.), px(32.))),
            ),
            (
                "group-c".to_string(),
                Bounds::new(point(px(200.), px(0.)), size(px(100.), px(32.))),
            ),
        ];
        assert_eq!(reorder_index_at_x("group-c", px(20.), &bounds), Some(0));
        assert_eq!(reorder_index_at_x("group-a", px(280.), &bounds), Some(2));
        assert_eq!(reorder_index_at_x("missing", px(20.), &bounds), None);
    }

    #[test]
    fn completed_drag_cannot_commit_twice() {
        let mut state = TabDragState::default();
        state.begin("group-a".into(), point(px(0.), px(0.)));
        state.promote_if_needed(point(px(10.), px(0.)), 5.0);
        state.set_outside(true);
        assert!(matches!(state.finish(), DropIntent::Detach { .. }));
        assert!(matches!(state.finish(), DropIntent::None));
    }

    #[test]
    fn cancelling_reorder_keeps_finish_inert() {
        let mut state = TabDragState::default();
        state.begin("group-b".into(), point(px(0.), px(0.)));
        state.promote_if_needed(point(px(10.), px(0.)), 5.0);
        state.set_reorder_index(Some(0));
        state.cancel();
        assert!(matches!(state.finish(), DropIntent::None));
    }

    #[test]
    fn empty_source_closes_only_after_successful_cross_window_merge() {
        assert!(should_close_empty_source(true, true, &1_u8, &2_u8));
        assert!(!should_close_empty_source(false, true, &1_u8, &2_u8));
        assert!(!should_close_empty_source(true, false, &1_u8, &2_u8));
        assert!(!should_close_empty_source(true, true, &1_u8, &1_u8));
    }

    #[test]
    fn viewport_hit_test_rejects_positions_outside_source_window() {
        let viewport = size(px(800.), px(600.));
        assert!(cursor_inside_viewport(point(px(400.), px(300.)), viewport));
        assert!(!cursor_inside_viewport(point(px(801.), px(300.)), viewport));
        assert!(!cursor_inside_viewport(point(px(-1.), px(300.)), viewport));
    }

    #[test]
    fn detach_is_offered_away_from_the_source_tab_bar_with_multiple_groups() {
        let tab_bar = Bounds::new(point(px(0.), px(0.)), size(px(800.), px(40.)));
        assert!(should_offer_detach(
            2,
            point(px(300.), px(300.)),
            Some(tab_bar),
            false
        ));
        assert!(!should_offer_detach(
            1,
            point(px(300.), px(300.)),
            Some(tab_bar),
            false
        ));
        assert!(!should_offer_detach(
            2,
            point(px(300.), px(20.)),
            Some(tab_bar),
            false
        ));
        assert!(!should_offer_detach(
            2,
            point(px(300.), px(300.)),
            Some(tab_bar),
            true
        ));
    }

    #[test]
    fn detach_hint_state_commits_detach_on_release() {
        let mut state = TabDragState::default();
        state.begin("group-a".into(), point(px(0.), px(0.)));
        state.promote_if_needed(point(px(10.), px(0.)), 5.0);
        state.set_outside(true);
        assert!(matches!(
            state.finish(),
            DropIntent::Detach { group_id } if group_id == "group-a"
        ));
    }
}
