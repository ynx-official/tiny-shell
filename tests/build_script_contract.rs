const WINDOWS_ICON_RERUN_DIRECTIVE: &str =
    r#"println!("cargo:rerun-if-changed=assets/icons/tiny-shell.ico");"#;

#[test]
fn windows_icon_changes_trigger_resource_rebuild() {
    let build_script = include_str!("../build.rs");

    assert!(
        build_script
            .lines()
            .map(str::trim)
            .any(|line| line == WINDOWS_ICON_RERUN_DIRECTIVE),
        "build.rs must tell Cargo to rebuild Windows resources when the application icon changes"
    );
}
