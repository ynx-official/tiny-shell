use gpui::{Entity, FocusHandle};
use gpui_component::{IconName, setting::SettingPage};
use rust_i18n::t;

use crate::TinyShell;

pub(crate) fn page(
    shell: &TinyShell,
    view: &Entity<TinyShell>,
    focus_handle: &FocusHandle,
) -> SettingPage {
    let mut page =
        SettingPage::new(t!("settings_key_bindings").to_string()).icon(IconName::SquareTerminal);
    for group in
        crate::app::keybinding_recorder::KeybindingsPage::render_groups(shell, view, focus_handle)
    {
        page = page.group(group);
    }
    page
}
