use gpui::Entity;
use gpui_component::setting::SettingPage;

use crate::TinyShell;

pub(crate) fn page(view: &Entity<TinyShell>) -> SettingPage {
    TinyShell::render_settings_terminal_page(view)
}
