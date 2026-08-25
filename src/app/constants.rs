pub(crate) const DEFAULT_COLS: u16 = 100;
pub(crate) const DEFAULT_ROWS: u16 = 30;
/// Default width for the global navigation and connected-host sidebar.
pub(crate) const SIDEBAR_WIDTH: f32 = 232.0;
pub(crate) const SIDEBAR_MIN_WIDTH: f32 = 216.0;
pub(crate) const SIDEBAR_MAX_WIDTH: f32 = 320.0;
pub(crate) const COLLAPSED_SIDEBAR_WIDTH: f32 = 52.0;
pub(crate) const SFTP_PANEL_MIN_HEIGHT: f32 = 180.0;
pub(crate) const SFTP_PANEL_DEFAULT_HEIGHT: f32 = 248.0;
pub(crate) const BOTTOM_MONITORING_HEIGHT: f32 = 80.0;

pub(crate) const TERMINAL_KEY_CONTEXT: &str = "TinyShellTerminal";

pub(crate) fn resolve_sidebar_width(saved_width: Option<f32>) -> f32 {
    saved_width
        .unwrap_or(SIDEBAR_WIDTH)
        .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::{SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, SIDEBAR_WIDTH, resolve_sidebar_width};

    #[test]
    fn sidebar_width_uses_readable_default_and_clamps_saved_values() {
        assert_eq!(resolve_sidebar_width(None), SIDEBAR_WIDTH);
        assert_eq!(resolve_sidebar_width(Some(80.0)), SIDEBAR_MIN_WIDTH);
        assert_eq!(resolve_sidebar_width(Some(248.0)), 248.0);
        assert_eq!(resolve_sidebar_width(Some(900.0)), SIDEBAR_MAX_WIDTH);
    }
}
