fn main() {
    println!("cargo:rerun-if-changed=native/freerdp/tiny_shell_freerdp.c");
    println!("cargo:rerun-if-changed=native/freerdp/tiny_shell_freerdp.h");
    println!("cargo:rerun-if-env-changed=TINY_SHELL_FREERDP_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=TINY_SHELL_FREERDP_LIB_DIR");
    println!("cargo:rerun-if-env-changed=TINY_SHELL_FREERDP_LIBS");

    if std::env::var_os("CARGO_FEATURE_FREERDP").is_some() {
        let mut build = cc::Build::new();
        build
            .file("native/freerdp/tiny_shell_freerdp.c")
            .include("native/freerdp")
            .warnings(true);
        if let Some(include_dir) = std::env::var_os("TINY_SHELL_FREERDP_INCLUDE_DIR") {
            build.include(include_dir);
        }
        build.compile("tiny_shell_freerdp");

        if let Some(lib_dir) = std::env::var_os("TINY_SHELL_FREERDP_LIB_DIR") {
            println!("cargo:rustc-link-search=native={}", lib_dir.to_string_lossy());
        }
        let libraries = std::env::var("TINY_SHELL_FREERDP_LIBS")
            .unwrap_or_else(|_| "freerdp-client3,freerdp3,winpr3".to_string());
        for library in libraries.split(',').map(str::trim).filter(|item| !item.is_empty()) {
            println!("cargo:rustc-link-lib=dylib={library}");
        }
    }

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icons/tiny-shell.ico");
        res.set("FileDescription", "TinyShell");
        res.set("ProductName", "TinyShell");
        res.compile().unwrap();
    }
}
