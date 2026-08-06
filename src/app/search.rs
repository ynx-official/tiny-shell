use std::{collections::HashMap, time::Duration};

use gpui::{
    Animation, AnimationExt as _, Context, ElementId, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Styled as _, Window, div, ease_out_quint,
    prelude::FluentBuilder as _, px, rems,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, ElementExt as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
};
use rust_i18n::t;

use crate::TinyShell;

impl TinyShell {
    pub(crate) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.window_state.search_active {
            self.close_search(window, cx);
        } else {
            self.open_search(window, cx);
        }
    }

    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.window_state_mut().search_active = true;
        self.window_state_mut().search_epoch = self.window_state.search_epoch.wrapping_add(1);
        // Focus the search input on the next frame so it happens after the
        // current render cycle completes, avoiding focus being stolen back
        // by the terminal panel's track_focus.
        let search_input = self.search_input.clone();
        crate::app::input_focus::defer_focus_input_at_end(search_input, window, cx);
        cx.notify();
    }

    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.window_state_mut().search_active = false;
        self.window_state_mut().search_query.clear();
        self.window_state_mut().search_matches.clear();
        self.window_state_mut().search_current = 0;
        self.window_state_mut().search_target_tab = None;
        self.window_state_mut().search_bar_bounds = None;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    /// Move keyboard focus back to the search input so the user can keep typing.
    /// Deferred to the next frame so it happens after the current render cycle,
    /// preventing the terminal panel's track_focus from stealing focus back.
    fn refocus_search_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        let search_input = self.search_input.clone();
        crate::app::input_focus::defer_focus_input_at_end(search_input, window, cx);
    }

    pub(crate) fn perform_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).text().to_string();
        if query.is_empty() {
            self.window_state_mut().search_query.clear();
            self.window_state_mut().search_matches.clear();
            self.window_state_mut().search_current = 0;
            self.refocus_search_input(window, cx);
            cx.notify();
            return;
        }

        let tab = self
            .preferred_terminal_tab_id()
            .as_deref()
            .and_then(|id| self.terminal_tab(id));

        let Some(tab) = tab else {
            self.status = t!("no_results").into();
            self.refocus_search_input(window, cx);
            cx.notify();
            return;
        };

        // Remember which tab was searched so highlights only appear in that pane.
        // Store the id before mutating the nested search state so the immutable
        // tab borrow does not overlap the state update.
        let tab_id = tab.id.clone();
        self.window_state_mut().search_target_tab = Some(tab_id.clone());
        let Some(tab) = self.terminal_tab(&tab_id) else {
            return;
        };

        let query_lower = query.to_lowercase();
        let query_byte_len = query.len();

        // Search the ENTIRE terminal buffer (scrollback + visible screen).
        // full_grid_rows returns grid line indices; the first row is at
        // `grid_start` (typically -history_size).
        let (grid_start, all_rows) = tab.full_grid_rows();

        let mut matches: Vec<(i32, i32)> = Vec::new();

        for (row_idx, row) in all_rows.iter().enumerate() {
            if row.is_empty() {
                continue;
            }

            // Build text string and byte→column index mapping.
            let mut text = String::with_capacity(row.len());
            let mut byte_to_col: Vec<i32> = Vec::new();
            for &(col, c) in row {
                text.push(c);
                while byte_to_col.len() < text.len() {
                    byte_to_col.push(col);
                }
            }
            let text_lower = text.to_lowercase();

            // Grid line index for this row.
            let abs_row = grid_start + row_idx as i32;

            let mut search_start = 0;
            while let Some(pos) = text_lower[search_start..].find(&query_lower) {
                let abs = search_start + pos;
                let start_col = byte_to_col[abs];
                let end_byte = (abs + query_byte_len).min(byte_to_col.len());
                let end_col = byte_to_col[end_byte - 1];
                for c in start_col..=end_col {
                    matches.push((abs_row, c));
                }
                search_start = abs + query_byte_len;
            }
        }

        let match_count = count_match_groups(&matches);

        self.window_state_mut().search_query = query;
        self.window_state_mut().search_matches = matches;

        if match_count > 0 {
            self.window_state_mut().search_current = 0;
            self.jump_to_current_match(cx);
        }

        self.status = format!(
            "{}: {} ({})",
            t!("search"),
            self.window_state.search_query,
            if match_count == 0 {
                t!("no_results").to_string()
            } else {
                format!("{}/{}", self.window_state.search_current + 1, match_count)
            }
        )
        .into();

        // Keep focus on the search input so the user can continue typing.
        self.refocus_search_input(window, cx);
        cx.notify();
    }

    pub(crate) fn search_goto_next(&mut self, cx: &mut Context<Self>) {
        let match_count = count_match_groups(&self.window_state.search_matches);
        if match_count == 0 {
            return;
        }
        self.window_state_mut().search_current =
            (self.window_state.search_current + 1) % match_count;
        self.jump_to_current_match(cx);
        cx.notify();
    }

    pub(crate) fn search_goto_prev(&mut self, cx: &mut Context<Self>) {
        let match_count = count_match_groups(&self.window_state.search_matches);
        if match_count == 0 {
            return;
        }
        self.window_state_mut().search_current =
            (self.window_state.search_current + match_count - 1) % match_count;
        self.jump_to_current_match(cx);
        cx.notify();
    }

    fn jump_to_current_match(&mut self, _cx: &mut Context<Self>) {
        // target_grid_line is the grid line index (negative = history).
        let Some((target_grid_line, _)) = find_nth_match_start(
            &self.window_state.search_matches,
            self.window_state.search_current,
        ) else {
            return;
        };

        let tab_ids = self
            .workspace()
            .tabs()
            .iter()
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        let tab_id = search_target_tab_id(self.window_state.search_target_tab.as_deref(), &tab_ids)
            .map(str::to_owned);

        let Some(tab_id) = tab_id else {
            return;
        };
        let tab = self.terminal_tab_mut(&tab_id);

        if let Some(tab) = tab {
            let snapshot = tab.render_snapshot(false);
            let display_offset = snapshot.display_offset as i32;
            let rows = snapshot.rows as i32;

            // viewport row = grid_line + display_offset
            let vp_row = target_grid_line + display_offset;
            let visible = vp_row >= 0 && vp_row < rows;

            if !visible {
                // Scroll so the target grid line appears near the top.
                // display_offset = -grid_line puts it at viewport row 0.
                let new_offset = (-target_grid_line).max(0) as usize;
                if new_offset > snapshot.display_offset {
                    tab.scroll_up_by(new_offset - snapshot.display_offset);
                } else if new_offset < snapshot.display_offset {
                    tab.scroll_down_by(snapshot.display_offset - new_offset);
                }
            }
        }
    }

    /// Build a highlight map for search matches, converting grid line indices
    /// to the current viewport coordinates. Only returns highlights for the
    /// pane that was actually searched (`search_target_tab`).
    pub(crate) fn search_highlight_map(
        &self,
        tab_id: &str,
        match_color: Hsla,
        current_color: Hsla,
    ) -> Option<HashMap<(i32, i32), Hsla>> {
        if self.window_state.search_matches.is_empty()
            || self.window_state.search_query.is_empty()
            || self.window_state.search_target_tab.as_deref() != Some(tab_id)
        {
            return None;
        }

        // Get current display_offset to convert grid line → viewport row.
        let tab_ids = self
            .workspace()
            .tabs()
            .iter()
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        let target_id =
            search_target_tab_id(self.window_state.search_target_tab.as_deref(), &tab_ids)?;
        let tab = self.terminal_tab(target_id)?;
        let snapshot = tab.render_snapshot(false);
        let display_offset = snapshot.display_offset as i32;
        let rows = snapshot.rows as i32;

        let mut map = HashMap::new();

        let mut sorted: Vec<(i32, i32)> = self.window_state.search_matches.clone();
        sorted.sort();

        let mut group_idx = 0;
        let mut i = 0;
        while i < sorted.len() {
            let is_current = group_idx == self.window_state.search_current;
            let color = if is_current {
                current_color
            } else {
                match_color
            };

            // grid_line → viewport row:  vp_row = grid_line + display_offset
            let (grid_line, _) = sorted[i];
            let vp_row = grid_line + display_offset;
            let mut j = i;
            if vp_row >= 0 && vp_row < rows {
                while j < sorted.len() && sorted[j].0 == grid_line {
                    if j > i && sorted[j].1 != sorted[j - 1].1 + 1 {
                        break;
                    }
                    map.insert((vp_row, sorted[j].1), color);
                    j += 1;
                }
            } else {
                // Outside current viewport — skip.
                while j < sorted.len() && sorted[j].0 == grid_line {
                    if j > i && sorted[j].1 != sorted[j - 1].1 + 1 {
                        break;
                    }
                    j += 1;
                }
            }

            group_idx += 1;
            i = j;
        }

        Some(map)
    }

    /// Render the expanded search bar overlay (when search is active).
    pub(crate) fn render_search_bar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement {
        let match_count = count_match_groups(&self.window_state.search_matches);
        let has_query = !self.window_state.search_query.is_empty();
        let has_matches = match_count > 0;
        let current_display = if has_matches {
            format!("{}/{}", self.window_state.search_current + 1, match_count)
        } else if has_query {
            "0".to_string()
        } else {
            String::new()
        };
        let search_epoch = self.window_state.search_epoch;

        let view = cx.entity();
        div()
            .absolute()
            .top(px(8.))
            .right(px(24.))
            .on_prepaint(move |bounds, _window, cx| {
                view.update(cx, |this, _| {
                    this.window_state_mut().search_bar_bounds = Some(bounds);
                });
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.refocus_search_input(window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .p_1()
                    .rounded(px(6.))
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .w(px(200.))
                            .on_key_down(cx.listener(
                                |this, event: &gpui::KeyDownEvent, window, cx| {
                                    if event.keystroke.key.as_str() == "escape" {
                                        this.close_search(window, cx);
                                        window.prevent_default();
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .child(Input::new(&self.search_input).small()),
                    )
                    .when(!current_display.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(rems(0.75))
                                .text_color(cx.theme().muted_foreground)
                                .min_w(px(36.))
                                .text_center()
                                .child(current_display),
                        )
                    })
                    .child(
                        Button::new("search-prev")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronUp)
                            .disabled(!has_matches)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.search_goto_prev(cx);
                            })),
                    )
                    .child(
                        Button::new("search-next")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronDown)
                            .disabled(!has_matches)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.search_goto_next(cx);
                            })),
                    )
                    .child(
                        Button::new("search-close")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_search(window, cx);
                            })),
                    ),
            )
            .with_animation(
                ElementId::NamedInteger("search-bar-fade".into(), search_epoch),
                Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()),
                |this, delta| this.opacity(delta * delta),
            )
            .into_any_element()
    }
}

fn search_target_tab_id<'a>(target: Option<&'a str>, tab_ids: &'a [String]) -> Option<&'a str> {
    target.filter(|id| tab_ids.iter().any(|candidate| candidate == *id))
}

/// Count distinct match groups in a sorted list of (row, col) positions.
/// A group is a run of consecutive columns in the same row.
fn count_match_groups(matches: &[(i32, i32)]) -> usize {
    if matches.is_empty() {
        return 0;
    }
    let mut sorted: Vec<(i32, i32)> = matches.to_vec();
    sorted.sort();
    let mut count = 0;
    let mut i = 0;
    while i < sorted.len() {
        count += 1;
        let (r, _) = sorted[i];
        i += 1;
        // Skip consecutive columns in the same row.
        while i < sorted.len() && sorted[i].0 == r && sorted[i].1 == sorted[i - 1].1 + 1 {
            i += 1;
        }
    }
    count
}

/// Find the (row, col) start of the Nth distinct match group.
fn find_nth_match_start(matches: &[(i32, i32)], n: usize) -> Option<(i32, i32)> {
    if matches.is_empty() {
        return None;
    }
    let mut sorted: Vec<(i32, i32)> = matches.to_vec();
    sorted.sort();
    let mut group_idx = 0;
    let mut i = 0;
    while i < sorted.len() {
        if group_idx == n {
            return Some(sorted[i]);
        }
        group_idx += 1;
        let (r, _) = sorted[i];
        i += 1;
        while i < sorted.len() && sorted[i].0 == r && sorted[i].1 == sorted[i - 1].1 + 1 {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::search_target_tab_id;

    #[test]
    fn search_target_requires_existing_tab() {
        let tabs = vec!["tab-a".to_string(), "tab-b".to_string()];
        assert_eq!(search_target_tab_id(Some("tab-a"), &tabs), Some("tab-a"));
        assert_eq!(search_target_tab_id(Some("missing"), &tabs), None);
        assert_eq!(search_target_tab_id(None, &tabs), None);
    }
}
