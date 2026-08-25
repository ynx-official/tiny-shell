use gpui::{Pixels, Window, px};

const DIALOG_EDGE_GUTTER: Pixels = px(16.);
// The pinned gpui-component Dialog adds this offset for every stacked layer.
const DIALOG_LAYER_OFFSET: Pixels = px(16.);
// Keep layout calculations aligned with Dialog's internal `.min_h_24()` constraint.
const DIALOG_MINIMUM_HEIGHT: Pixels = px(96.);

pub(crate) const MAIN_WINDOW_CLOSE_DIALOG_BASE_HEIGHT: Pixels = px(160.);
pub(crate) const UPDATE_RESTART_DIALOG_BASE_HEIGHT: Pixels = px(160.);
pub(crate) const UPDATE_DIALOG_HEIGHT: Pixels = px(520.);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CenteredDialogLayout {
    pub(crate) height: Pixels,
    pub(crate) margin_top: Pixels,
}

pub(crate) fn centered_dialog_layout(
    window: &Window,
    preferred_height: Pixels,
    layer_ix: usize,
) -> CenteredDialogLayout {
    let paddings = gpui_component::window_paddings(window);
    centered_dialog_layout_for_viewport(
        window.viewport_size().height,
        paddings.top,
        paddings.bottom,
        preferred_height,
        layer_ix,
    )
}

pub(crate) fn confirmation_dialog_height(window: &Window, base_height: Pixels) -> Pixels {
    confirmation_dialog_height_for_rem(base_height, window.rem_size())
}

fn confirmation_dialog_height_for_rem(base_height: Pixels, rem_size: Pixels) -> Pixels {
    let default_ui_font_size = px(crate::session::config_file::default_ui_font_size());
    base_height * (rem_size / default_ui_font_size).max(1.)
}

fn centered_dialog_layout_for_viewport(
    viewport_height: Pixels,
    padding_top: Pixels,
    padding_bottom: Pixels,
    preferred_height: Pixels,
    layer_ix: usize,
) -> CenteredDialogLayout {
    let available_viewport_height = (viewport_height - padding_top - padding_bottom).max(px(0.));
    let maximum_dialog_height =
        (available_viewport_height - DIALOG_EDGE_GUTTER - DIALOG_EDGE_GUTTER)
            .max(DIALOG_MINIMUM_HEIGHT);
    let height = preferred_height
        .max(DIALOG_MINIMUM_HEIGHT)
        .min(maximum_dialog_height);
    let centered_margin_top = ((available_viewport_height - height) / 2.).max(px(0.));

    CenteredDialogLayout {
        height,
        margin_top: (centered_margin_top - DIALOG_LAYER_OFFSET * layer_ix).max(px(0.)),
    }
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::{
        MAIN_WINDOW_CLOSE_DIALOG_BASE_HEIGHT, UPDATE_DIALOG_HEIGHT,
        UPDATE_RESTART_DIALOG_BASE_HEIGHT, centered_dialog_layout_for_viewport,
        confirmation_dialog_height_for_rem,
    };

    #[test]
    fn centered_dialog_layout_centers_the_three_supported_dialogs() {
        let main_window_close = centered_dialog_layout_for_viewport(
            px(800.),
            px(0.),
            px(0.),
            MAIN_WINDOW_CLOSE_DIALOG_BASE_HEIGHT,
            0,
        );
        assert_eq!(main_window_close.height, px(160.));
        assert_eq!(main_window_close.margin_top, px(320.));

        let update_restart = centered_dialog_layout_for_viewport(
            px(800.),
            px(0.),
            px(0.),
            UPDATE_RESTART_DIALOG_BASE_HEIGHT,
            0,
        );
        assert_eq!(update_restart.height, px(160.));
        assert_eq!(update_restart.margin_top, px(320.));

        let update =
            centered_dialog_layout_for_viewport(px(700.), px(0.), px(0.), UPDATE_DIALOG_HEIGHT, 0);
        assert_eq!(update.height, px(520.));
        assert_eq!(update.margin_top, px(90.));
    }

    #[test]
    fn centered_dialog_layout_accounts_for_window_paddings() {
        let layout = centered_dialog_layout_for_viewport(
            px(800.),
            px(8.),
            px(24.),
            MAIN_WINDOW_CLOSE_DIALOG_BASE_HEIGHT,
            0,
        );

        assert_eq!(layout.height, px(160.));
        assert_eq!(layout.margin_top, px(304.));
    }

    #[test]
    fn centered_dialog_layout_preserves_edge_gutters_in_short_viewports() {
        let layout =
            centered_dialog_layout_for_viewport(px(500.), px(0.), px(0.), UPDATE_DIALOG_HEIGHT, 0);

        assert_eq!(layout.height, px(468.));
        assert_eq!(layout.margin_top, px(16.));
    }

    #[test]
    fn confirmation_dialog_height_scales_with_larger_ui_font() {
        assert_eq!(
            confirmation_dialog_height_for_rem(MAIN_WINDOW_CLOSE_DIALOG_BASE_HEIGHT, px(8.)),
            px(160.)
        );
        assert_eq!(
            confirmation_dialog_height_for_rem(MAIN_WINDOW_CLOSE_DIALOG_BASE_HEIGHT, px(14.)),
            px(160.)
        );
        assert_eq!(
            confirmation_dialog_height_for_rem(MAIN_WINDOW_CLOSE_DIALOG_BASE_HEIGHT, px(21.)),
            px(240.)
        );
    }

    #[test]
    fn centered_dialog_layout_compensates_for_component_layer_offset() {
        let layout = centered_dialog_layout_for_viewport(
            px(800.),
            px(0.),
            px(0.),
            MAIN_WINDOW_CLOSE_DIALOG_BASE_HEIGHT,
            1,
        );

        assert_eq!(layout.height, px(160.));
        assert_eq!(layout.margin_top, px(304.));
    }

    #[test]
    fn centered_dialog_layout_honors_the_component_minimum_height() {
        let constrained = centered_dialog_layout_for_viewport(px(100.), px(0.), px(0.), px(10.), 0);
        assert_eq!(constrained.height, px(96.));
        assert_eq!(constrained.margin_top, px(2.));

        let overflowing = centered_dialog_layout_for_viewport(px(50.), px(0.), px(0.), px(10.), 0);
        assert_eq!(overflowing.height, px(96.));
        assert_eq!(overflowing.margin_top, px(0.));
    }
}
