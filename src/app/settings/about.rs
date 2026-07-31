use gpui_component::setting::SettingPage;

use crate::TinyShell;

pub(crate) fn page() -> SettingPage {
    TinyShell::render_settings_about_page()
}
