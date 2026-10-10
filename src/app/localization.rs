use gpui::{App, Context, Global, Window};
use gpui_component::input::InputState;

struct DisplayLocale;
impl Global for DisplayLocale {}

fn resolve_locale<'a>(configured: &'a str, system: Option<&str>) -> &'a str {
    if configured != "system" {
        return configured;
    }
    if system.is_some_and(|locale| locale.to_ascii_lowercase().starts_with("zh")) {
        "zh-CN"
    } else {
        "en"
    }
}

/// Apply before constructing inputs, and notify existing inputs on runtime changes.
pub(crate) fn set_display_locale(configured: &str, cx: &mut App) -> String {
    let system = sys_locale::get_locale();
    let locale = resolve_locale(configured, system.as_deref());
    rust_i18n::set_locale(locale);
    gpui_component::set_locale(locale);
    cx.set_global(DisplayLocale);
    locale.to_string()
}

/// Keep the translation live without replacing the entity, its value, selection or focus.
pub(crate) fn localized_input(
    window: &mut Window,
    cx: &mut Context<InputState>,
    placeholder: impl Fn() -> String + 'static,
) -> InputState {
    let initial = placeholder();
    cx.observe_global_in::<DisplayLocale>(window, move |input, window, cx| {
        input.set_placeholder(placeholder(), window, cx);
    })
    // GPUI holds a weak entity and removes this observer after the input is dropped.
    .detach();
    InputState::new(window, cx).placeholder(initial)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_language_takes_priority_over_system_language() {
        assert_eq!(resolve_locale("en", Some("zh-CN")), "en");
        assert_eq!(resolve_locale("zh-CN", Some("en-US")), "zh-CN");
    }

    #[test]
    fn system_language_maps_to_supported_locales() {
        for system in [Some("zh-CN"), Some("zh_TW"), Some("ZH-HK")] {
            assert_eq!(resolve_locale("system", system), "zh-CN");
        }
        for system in [None, Some("en-US"), Some("fr-FR")] {
            assert_eq!(resolve_locale("system", system), "en");
        }
    }
}
