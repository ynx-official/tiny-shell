use super::config::TerminalDisplayStyle;

pub(super) fn terminal_cell_width_for(font_size: f32, style: TerminalDisplayStyle) -> f32 {
    let width_ratio = match style {
        TerminalDisplayStyle::Standard => 0.646,
        TerminalDisplayStyle::Compact => 0.58,
    };
    (font_size * width_ratio).max(6.0)
}

pub(super) fn terminal_line_height_for(font_size: f32, style: TerminalDisplayStyle) -> f32 {
    match style {
        TerminalDisplayStyle::Standard => (font_size * 1.385).max(font_size + 2.0),
        TerminalDisplayStyle::Compact => (font_size * 1.2).max(font_size + 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::{terminal_cell_width_for, terminal_line_height_for};
    use crate::session::config::TerminalDisplayStyle;

    #[test]
    fn standard_terminal_metrics_preserve_existing_dimensions() {
        assert_eq!(
            terminal_cell_width_for(14.0, TerminalDisplayStyle::Standard),
            14.0 * 0.646
        );
        assert_eq!(
            terminal_line_height_for(14.0, TerminalDisplayStyle::Standard),
            14.0 * 1.385
        );
    }

    #[test]
    fn compact_terminal_metrics_increase_visible_grid_density() {
        let standard_width = terminal_cell_width_for(14.0, TerminalDisplayStyle::Standard);
        let compact_width = terminal_cell_width_for(14.0, TerminalDisplayStyle::Compact);
        let standard_height = terminal_line_height_for(14.0, TerminalDisplayStyle::Standard);
        let compact_height = terminal_line_height_for(14.0, TerminalDisplayStyle::Compact);

        assert!(compact_width < standard_width);
        assert!(compact_height < standard_height);
    }
}
