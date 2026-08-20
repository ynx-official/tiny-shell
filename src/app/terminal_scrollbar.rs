use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{Pixels, Point, Size, point, px, size};
use gpui_component::scroll::ScrollbarHandle;

use crate::terminal;

struct TerminalScrollbarState {
    line_height: Pixels,
    total_lines: usize,
    viewport_lines: usize,
    display_offset: usize,
}

#[derive(Clone, Default)]
pub(crate) struct TerminalScrollbarHandle {
    state: Rc<RefCell<Option<TerminalScrollbarState>>>,
    pub(crate) future_display_offset: Rc<Cell<Option<usize>>>,
}

impl TerminalScrollbarHandle {
    pub(crate) fn update(&self, snapshot: &terminal::RenderSnapshot, line_height: Pixels) {
        self.state.replace(Some(TerminalScrollbarState {
            line_height,
            total_lines: snapshot.history_size + snapshot.rows,
            viewport_lines: snapshot.rows,
            display_offset: snapshot.display_offset,
        }));
    }
}

impl ScrollbarHandle for TerminalScrollbarHandle {
    fn offset(&self) -> Point<Pixels> {
        let state_ref = self.state.borrow();
        let Some(state) = state_ref.as_ref() else {
            return point(px(0.), px(0.));
        };
        let scroll_offset = scroll_offset_lines(
            state.total_lines,
            state.viewport_lines,
            state.display_offset,
        );
        point(px(0.), -(scroll_offset as f32 * state.line_height))
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        let state_ref = self.state.borrow();
        let Some(state) = state_ref.as_ref() else {
            return;
        };
        let offset_delta = (offset.y / state.line_height).round() as i32;
        self.future_display_offset
            .set(Some(display_offset_for_scroll(
                state.total_lines,
                state.viewport_lines,
                offset_delta,
            )));
    }

    fn content_size(&self) -> Size<Pixels> {
        let state_ref = self.state.borrow();
        let Some(state) = state_ref.as_ref() else {
            return size(px(0.), px(0.));
        };
        size(
            px(0.),
            state.total_lines.max(state.viewport_lines) as f32 * state.line_height,
        )
    }
}

fn scroll_offset_lines(total_lines: usize, viewport_lines: usize, display_offset: usize) -> usize {
    total_lines
        .saturating_sub(viewport_lines)
        .saturating_sub(display_offset)
}

fn display_offset_for_scroll(
    total_lines: usize,
    viewport_lines: usize,
    offset_delta: i32,
) -> usize {
    let max_offset = total_lines.saturating_sub(viewport_lines);
    let max_offset_signed = max_offset.min(i64::MAX as usize) as i64;
    (max_offset_signed + i64::from(offset_delta)).clamp(0, max_offset_signed) as usize
}

#[cfg(test)]
mod tests {
    use super::{display_offset_for_scroll, scroll_offset_lines};

    #[test]
    fn scroll_offset_saturates_for_short_content_and_large_display_offsets() {
        assert_eq!(scroll_offset_lines(5, 10, 0), 0);
        assert_eq!(scroll_offset_lines(100, 20, 30), 50);
        assert_eq!(scroll_offset_lines(100, 20, 100), 0);
    }

    #[test]
    fn display_offset_is_clamped_to_terminal_history() {
        assert_eq!(display_offset_for_scroll(100, 20, -200), 0);
        assert_eq!(display_offset_for_scroll(100, 20, -30), 50);
        assert_eq!(display_offset_for_scroll(100, 20, 20), 80);
        assert_eq!(display_offset_for_scroll(5, 10, 20), 0);
        assert_eq!(
            display_offset_for_scroll(usize::MAX, 0, 0),
            i64::MAX as usize
        );
    }
}
