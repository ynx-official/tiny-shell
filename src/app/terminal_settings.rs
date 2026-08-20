use gpui::{AppContext as _, Context, Window, px};
use gpui_component::Theme;
use rust_i18n::t;

use crate::{
    TinyShell,
    app::{resizable::ResizableState, theme},
    session::config::{CursorStyle, TerminalDisplayStyle},
    session::terminal_preferences::{terminal_cell_width_for, terminal_line_height_for},
};

impl TinyShell {
    pub(crate) fn terminal_cell_width(&self) -> f32 {
        terminal_cell_width_for(self.terminal_font_size, self.terminal_display_style)
    }

    pub(crate) fn terminal_line_height(&self) -> f32 {
        terminal_line_height_for(self.terminal_font_size, self.terminal_display_style)
    }

    pub(crate) fn change_terminal_display_style(
        &mut self,
        style: TerminalDisplayStyle,
        cx: &mut Context<Self>,
    ) {
        self.terminal_display_style = style;
        self.config.set_terminal_display_style(style);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(crate) fn change_terminal_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.terminal_font_size = (self.terminal_font_size + delta).clamp(10.0, 24.0);
        self.config.set_terminal_font_size(self.terminal_font_size);
        self.mark_config_preferences_dirty();
        self.status = t!(
            "terminal_font_size_changed",
            size = format!("{:.0}", self.terminal_font_size)
        )
        .into();
        cx.notify();
    }

    pub(crate) fn change_ui_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.ui_font_size = (self.ui_font_size + delta).clamp(8.0, 24.0);
        self.config.set_ui_font_size(self.ui_font_size);
        self.mark_config_preferences_dirty();
        Theme::global_mut(cx).font_size = px(self.ui_font_size);
        self.status = t!(
            "ui_font_size_changed",
            size = format!("{:.0}", self.ui_font_size)
        )
        .into();
        cx.notify();
    }

    pub(crate) fn change_ui_font_family(
        &mut self,
        family: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.set_ui_font_family(family);
        let resolved = crate::app::font_preferences::resolve_ui_font(
            family,
            &cx.text_system().all_font_names(),
        );
        self.ui_font_family = resolved.family;
        self.ui_font_preference_available = resolved.preference_available;
        self.mark_config_preferences_dirty();
        theme::set_theme_font_names(
            Theme::global_mut(cx),
            &self.ui_font_family,
            &self.terminal_font_family,
        );
        cx.notify();
        window.refresh();
    }

    pub(crate) fn change_terminal_font_family(&mut self, family: &str, cx: &mut Context<Self>) {
        self.config.set_terminal_font_family(family);
        let resolved = crate::app::font_preferences::resolve_terminal_font(
            family,
            &cx.text_system().all_font_names(),
        );
        self.terminal_font_family = resolved.selection.family;
        self.terminal_font_preference_available = resolved.selection.preference_available;
        self.terminal_font_fallbacks = resolved.fallbacks;
        self.mark_config_preferences_dirty();
        Theme::global_mut(cx).mono_font_family = self.terminal_font_family.clone();
        cx.notify();
    }

    pub(crate) fn change_cursor_style(&mut self, style: CursorStyle, cx: &mut Context<Self>) {
        self.cursor_style = style;
        self.config.set_cursor_style(style);
        self.mark_config_preferences_dirty();
        cx.notify();
    }

    pub(crate) fn reset_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut staged = self.config.clone();
        staged.set_layout_state(None, None, None);
        self.commit_staged_config_in_window_async(
            staged,
            window,
            |this, _, cx| {
                this.is_layout_reset = true;
                this.workspace_panels = cx.new(|_| ResizableState::default());
                this.body_panels = cx.new(|_| ResizableState::default());
                cx.notify();
            },
            |this, error, _, cx| {
                tracing::warn!("failed to persist reset layout: {error:#}");
                this.status = t!("config_save_failed", error = format!("{error:#}")).into();
                cx.notify();
            },
            cx,
        );
    }
}
