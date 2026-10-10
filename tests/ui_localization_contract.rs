#[test]
fn startup_applies_language_before_constructing_localized_inputs() {
    let source = include_str!("../src/app/mod.rs").replace("\r\n", "\n");
    let constructor = source
        .split("pub(crate) fn new(\n        window: &mut Window,\n        session_store:")
        .nth(1)
        .unwrap();
    let locale = constructor.find("localization::set_display_locale(");
    let inputs = constructor.find("ConnectionFormInputs::new(").unwrap();
    assert!(
        locale.is_some_and(|position| position < inputs),
        "Apply the configured language before constructing any persistent input"
    );
}

#[test]
fn language_switch_notifies_existing_inputs_in_all_windows() {
    let source = include_str!("../src/app/theme.rs");
    let switch = source
        .split("pub(crate) fn set_display_language(")
        .nth(1)
        .unwrap()
        .split("pub(crate) fn apply_theme_preferences(")
        .next()
        .unwrap();
    assert!(
        switch.contains("localization::set_display_locale("),
        "Switching the global locale alone leaves cached input placeholders stale"
    );
    assert!(switch.contains("cx.refresh_windows()"));
}
