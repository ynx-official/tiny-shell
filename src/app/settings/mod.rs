pub(crate) mod about;
pub(crate) mod actions;
pub(crate) mod appearance;
pub(crate) mod controls;
pub(crate) mod form;
pub(crate) mod highlighting;
pub(crate) mod keybindings;
pub(crate) mod proxy;
pub(crate) mod sync;
pub(crate) mod terminal;
pub(crate) mod update;
pub(crate) mod view;
pub(crate) mod workspace;

use crate::{
    app::constants::BOTTOM_MONITORING_HEIGHT,
    session::config::{CursorStyle, TerminalDisplayStyle, TitleBarStyle, UpdateCheckMode},
};

pub(crate) const MONITORING_POSITIONS: [MonitoringPosition; 3] = [
    MonitoringPosition::Bottom,
    MonitoringPosition::Sidebar,
    MonitoringPosition::Hidden,
];

pub(crate) const DISPLAY_LANGUAGES: [DisplayLanguage; 3] = [
    DisplayLanguage::System,
    DisplayLanguage::English,
    DisplayLanguage::Chinese,
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum MonitoringPosition {
    #[default]
    Bottom,
    Sidebar,
    Hidden,
}

impl MonitoringPosition {
    pub(crate) fn from_config(value: &str) -> Self {
        match value {
            "Sidebar" => Self::Sidebar,
            "Hidden" => Self::Hidden,
            _ => Self::Bottom,
        }
    }

    pub(crate) const fn config_value(self) -> &'static str {
        match self {
            Self::Bottom => "Bottom",
            Self::Sidebar => "Sidebar",
            Self::Hidden => "Hidden",
        }
    }

    pub(crate) const fn translation_key(self) -> &'static str {
        match self {
            Self::Bottom => "position_bottom",
            Self::Sidebar => "position_sidebar",
            Self::Hidden => "position_hidden",
        }
    }

    pub(crate) const fn lower_panel_overhead(self) -> f32 {
        match self {
            Self::Bottom => BOTTOM_MONITORING_HEIGHT,
            Self::Sidebar | Self::Hidden => 0.0,
        }
    }
}

pub(crate) fn adjusted_lower_panel_height(
    current_height: f32,
    previous: MonitoringPosition,
    next: MonitoringPosition,
) -> f32 {
    (current_height - previous.lower_panel_overhead() + next.lower_panel_overhead()).max(0.0)
}

pub(crate) fn adjusted_body_panel_sizes(
    sizes: Option<&[f32]>,
    previous: MonitoringPosition,
    next: MonitoringPosition,
) -> Option<Vec<f32>> {
    sizes.map(|sizes| {
        let mut adjusted = sizes.to_vec();
        if let Some(height) = adjusted.get_mut(1) {
            *height = adjusted_lower_panel_height(*height, previous, next);
        }
        adjusted
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DisplayLanguage {
    #[default]
    System,
    English,
    Chinese,
}

impl DisplayLanguage {
    pub(crate) fn from_config(value: &str) -> Self {
        match value {
            "en" => Self::English,
            "zh-CN" => Self::Chinese,
            _ => Self::System,
        }
    }

    pub(crate) const fn config_value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "en",
            Self::Chinese => "zh-CN",
        }
    }

    pub(crate) const fn translation_key(self) -> &'static str {
        match self {
            Self::System => "follow_system",
            Self::English => "english",
            Self::Chinese => "chinese",
        }
    }
}

pub(crate) const fn terminal_display_style_key(style: TerminalDisplayStyle) -> &'static str {
    match style {
        TerminalDisplayStyle::Standard => "terminal_display_style_standard",
        TerminalDisplayStyle::Compact => "terminal_display_style_compact",
    }
}

pub(crate) const fn cursor_style_key(style: CursorStyle) -> &'static str {
    match style {
        CursorStyle::Default => "cursor_style_default",
        CursorStyle::Blink => "cursor_style_blink",
        CursorStyle::Beam => "cursor_style_beam",
        CursorStyle::BeamBlink => "cursor_style_beam_blink",
    }
}

pub(crate) const fn title_bar_style_key(style: TitleBarStyle) -> &'static str {
    match style {
        TitleBarStyle::Native => "title_bar_native",
        TitleBarStyle::Integrated => "title_bar_integrated",
    }
}

pub(crate) const fn update_check_mode_key(mode: UpdateCheckMode) -> &'static str {
    match mode {
        UpdateCheckMode::Startup => "update_frequency_startup",
        UpdateCheckMode::Interval => "update_frequency_interval",
        UpdateCheckMode::Disabled => "update_frequency_disabled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitoring_position_normalizes_unknown_values() {
        assert_eq!(
            MonitoringPosition::from_config("Sidebar"),
            MonitoringPosition::Sidebar
        );
        assert_eq!(
            MonitoringPosition::from_config("unknown"),
            MonitoringPosition::Bottom
        );
    }

    #[test]
    fn monitoring_position_preserves_sftp_height_when_moving_the_monitor_strip() {
        assert_eq!(
            adjusted_lower_panel_height(
                328.0,
                MonitoringPosition::Bottom,
                MonitoringPosition::Sidebar,
            ),
            248.0
        );
        assert_eq!(
            adjusted_lower_panel_height(
                248.0,
                MonitoringPosition::Sidebar,
                MonitoringPosition::Bottom,
            ),
            328.0
        );
        assert_eq!(
            adjusted_lower_panel_height(
                248.0,
                MonitoringPosition::Sidebar,
                MonitoringPosition::Hidden,
            ),
            248.0
        );
    }

    #[test]
    fn monitoring_position_adjusts_saved_height_before_the_workspace_is_rendered() {
        assert_eq!(
            adjusted_body_panel_sizes(
                Some(&[520.0, 328.0]),
                MonitoringPosition::Bottom,
                MonitoringPosition::Sidebar,
            ),
            Some(vec![520.0, 248.0])
        );
        assert_eq!(
            adjusted_body_panel_sizes(
                Some(&[520.0]),
                MonitoringPosition::Bottom,
                MonitoringPosition::Sidebar,
            ),
            Some(vec![520.0])
        );
    }

    #[test]
    fn display_language_normalizes_unknown_values() {
        assert_eq!(DisplayLanguage::from_config("en"), DisplayLanguage::English);
        assert_eq!(
            DisplayLanguage::from_config("zh-CN"),
            DisplayLanguage::Chinese
        );
        assert_eq!(
            DisplayLanguage::from_config("unknown"),
            DisplayLanguage::System
        );
    }

    #[test]
    fn typed_options_round_trip_to_config_values() {
        for position in MONITORING_POSITIONS {
            assert_eq!(
                MonitoringPosition::from_config(position.config_value()),
                position
            );
        }
        for language in DISPLAY_LANGUAGES {
            assert_eq!(
                DisplayLanguage::from_config(language.config_value()),
                language
            );
        }
    }
}
