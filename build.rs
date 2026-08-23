use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

const FREERDP_PACKAGES: [&str; 3] = ["freerdp-client3", "freerdp3", "winpr3"];
const WINDOWS_CORE_LIBRARIES: [&str; 3] = ["freerdp-client3.lib", "freerdp3.lib", "winpr3.lib"];
const WINDOWS_CORE_RUNTIME: [&str; 3] = ["freerdp-client3.dll", "freerdp3.dll", "winpr3.dll"];

#[derive(Default)]
struct FreeRdpPaths {
    include_dirs: BTreeSet<PathBuf>,
    lib_dirs: BTreeSet<PathBuf>,
    runtime_dir: Option<PathBuf>,
    link_via_pkg_config: bool,
}

fn main() {
    println!("cargo:rerun-if-changed=native/freerdp/tiny_shell_freerdp.c");
    println!("cargo:rerun-if-changed=native/freerdp/tiny_shell_freerdp.h");
    println!("cargo:rerun-if-env-changed=TINY_SHELL_FREERDP_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=TINY_SHELL_FREERDP_INCLUDE_DIRS");
    println!("cargo:rerun-if-env-changed=TINY_SHELL_FREERDP_LIB_DIR");
    println!("cargo:rerun-if-env-changed=TINY_SHELL_FREERDP_LIBS");
    println!("cargo:rerun-if-env-changed=TINY_SHELL_FREERDP_RUNTIME_DIR");
    println!("cargo:rerun-if-env-changed=FREERDP_RUNTIME_DIR");
    println!("cargo:rerun-if-env-changed=VCPKG_ROOT");
    println!("cargo:rerun-if-env-changed=VCPKG_INSTALLATION_ROOT");
    println!("cargo:rerun-if-env-changed=VCPKG_INSTALLED_DIR");
    println!("cargo:rerun-if-env-changed=VCPKG_TARGET_TRIPLET");
    println!("cargo:rerun-if-env-changed=VCPKG_DEFAULT_TRIPLET");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_LIBDIR");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_SYSROOT_DIR");
    println!("cargo:rustc-check-cfg=cfg(tiny_shell_freerdp_backend)");

    let force_freerdp = env::var_os("CARGO_FEATURE_FREERDP").is_some();
    let auto_freerdp = env::var_os("CARGO_FEATURE_FREERDP_AUTO").is_some();
    // Windows uses the operating system's mstsc.exe client.  FreeRDP remains
    // a native backend for macOS and Linux only.
    if (force_freerdp || auto_freerdp) && !target_is_windows() {
        match discover_freerdp() {
            Ok(Some(paths)) => compile_freerdp_bridge(paths),
            Ok(None) if force_freerdp => {
                clear_windows_runtime_from_out_dir();
                panic!(
                    "the `freerdp` feature requires FreeRDP 3 development and runtime files; set \
                     TINY_SHELL_FREERDP_INCLUDE_DIRS, TINY_SHELL_FREERDP_LIB_DIR and, on Windows, \
                     TINY_SHELL_FREERDP_RUNTIME_DIR, or install FreeRDP 3 through pkg-config/vcpkg"
                );
            }
            Ok(None) => {
                clear_windows_runtime_from_out_dir();
                println!(
                    "cargo:warning=FreeRDP 3 was not found; building the no-backend fallback. \
                     After installing it, run `cargo clean -p tiny-shell` if Cargo reuses this result"
                );
            }
            Err(error) => {
                clear_windows_runtime_from_out_dir();
                panic!("invalid FreeRDP configuration: {error}");
            }
        }
    } else if target_is_windows() && force_freerdp {
        println!(
            "cargo:warning=the `freerdp` feature is ignored on Windows; TinyShell launches mstsc.exe"
        );
    }

    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icons/tiny-shell.ico");
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icons/tiny-shell.ico");
        res.set("FileDescription", "TinyShell");
        res.set("ProductName", "TinyShell");
        res.compile().unwrap();
    }
}

fn compile_freerdp_bridge(paths: FreeRdpPaths) {
    println!("cargo:rustc-cfg=tiny_shell_freerdp_backend");
    let mut build = cc::Build::new();
    build
        .file("native/freerdp/tiny_shell_freerdp.c")
        .include("native/freerdp")
        .warnings(true);
    for include_dir in &paths.include_dirs {
        build.include(include_dir);
    }
    build.compile("tiny_shell_freerdp");

    let custom_libraries = env::var_os("TINY_SHELL_FREERDP_LIBS").is_some();
    if paths.link_via_pkg_config && !custom_libraries {
        emit_pkg_config_link_metadata();
    } else {
        for lib_dir in &paths.lib_dirs {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
        }
        for library in configured_libraries() {
            println!("cargo:rustc-link-lib=dylib={library}");
        }
    }

    if target_is_windows() {
        copy_windows_runtime_to_out_dir(
            paths
                .runtime_dir
                .as_deref()
                .expect("validated Windows FreeRDP runtime directory"),
        );
    }
}

fn discover_freerdp() -> Result<Option<FreeRdpPaths>, String> {
    let mut paths = FreeRdpPaths::default();
    if let Some(include_dir) = env::var_os("TINY_SHELL_FREERDP_INCLUDE_DIR") {
        paths.include_dirs.insert(PathBuf::from(include_dir));
    }
    if let Some(include_dirs) = env::var_os("TINY_SHELL_FREERDP_INCLUDE_DIRS") {
        paths.include_dirs.extend(env::split_paths(&include_dirs));
    }
    if let Some(lib_dir) = env::var_os("TINY_SHELL_FREERDP_LIB_DIR") {
        paths.lib_dirs.insert(PathBuf::from(lib_dir));
    }
    if target_is_windows() {
        paths.runtime_dir = env::var_os("TINY_SHELL_FREERDP_RUNTIME_DIR")
            .or_else(|| env::var_os("FREERDP_RUNTIME_DIR"))
            .map(PathBuf::from);
    }

    let explicit_build_paths = !paths.include_dirs.is_empty() || !paths.lib_dirs.is_empty();
    let explicitly_configured = explicit_build_paths || paths.runtime_dir.is_some();

    if target_is_windows() {
        if !explicit_build_paths {
            if let Some(prefix) = discover_windows_vcpkg_prefix() {
                paths.include_dirs.insert(prefix.join("include"));
                paths.include_dirs.insert(prefix.join("include/freerdp3"));
                paths.include_dirs.insert(prefix.join("include/winpr3"));
                paths.lib_dirs.insert(prefix.join("lib"));
                if paths.runtime_dir.is_none() {
                    paths.runtime_dir = Some(prefix.join("bin"));
                }
            }
        }
        if paths.runtime_dir.is_none() {
            paths.runtime_dir = infer_windows_runtime_dir(&paths.lib_dirs);
        }
    } else if paths.include_dirs.is_empty() || paths.lib_dirs.is_empty() {
        if let Some(pkg_paths) = discover_with_pkg_config() {
            if paths.include_dirs.is_empty() {
                paths.include_dirs = pkg_paths.include_dirs;
            }
            if paths.lib_dirs.is_empty() {
                paths.lib_dirs = pkg_paths.lib_dirs;
                paths.link_via_pkg_config = true;
            }
        }
    }

    if paths.include_dirs.is_empty()
        && paths.lib_dirs.is_empty()
        && paths.runtime_dir.is_none()
        && !paths.link_via_pkg_config
    {
        return Ok(None);
    }
    if !freerdp_headers_exist(&paths.include_dirs) {
        let message = format!(
            "FreeRDP/WinPR headers were not found in: {}",
            display_paths(&paths.include_dirs)
        );
        return if explicitly_configured {
            Err(message)
        } else {
            Ok(None)
        };
    }
    if env::var_os("TINY_SHELL_FREERDP_LIBS").is_some() && configured_libraries().is_empty() {
        return Err("TINY_SHELL_FREERDP_LIBS must contain at least one library name".into());
    }
    if paths.lib_dirs.is_empty() && !paths.link_via_pkg_config {
        return Err(
            "no FreeRDP library directory was found; set TINY_SHELL_FREERDP_LIB_DIR".into(),
        );
    }
    if target_is_windows() {
        validate_windows_paths(&paths)?;
    }

    track_freerdp_inputs(&paths);
    Ok(Some(paths))
}

fn freerdp_headers_exist(include_dirs: &BTreeSet<PathBuf>) -> bool {
    let has_freerdp = include_dirs
        .iter()
        .any(|dir| dir.join("freerdp/freerdp.h").is_file());
    let has_winpr = include_dirs
        .iter()
        .any(|dir| dir.join("winpr/wtypes.h").is_file());
    has_freerdp && has_winpr
}

fn discover_with_pkg_config() -> Option<FreeRdpPaths> {
    let mut paths = FreeRdpPaths::default();
    for package in FREERDP_PACKAGES {
        let library = pkg_config::Config::new()
            .atleast_version("3")
            .cargo_metadata(false)
            .probe(package)
            .ok()?;
        paths.include_dirs.extend(library.include_paths);
        paths.lib_dirs.extend(library.link_paths);
    }
    Some(paths)
}

fn discover_windows_vcpkg_prefix() -> Option<PathBuf> {
    let triplet = env::var("VCPKG_TARGET_TRIPLET")
        .or_else(|_| env::var("VCPKG_DEFAULT_TRIPLET"))
        .unwrap_or_else(|_| match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("aarch64") => "arm64-windows".to_string(),
            Ok("x86") => "x86-windows".to_string(),
            _ => "x64-windows".to_string(),
        });
    let mut candidates = Vec::new();
    if let Some(installed) = env::var_os("VCPKG_INSTALLED_DIR") {
        let installed = PathBuf::from(installed);
        candidates.push(installed.join(&triplet));
        candidates.push(installed);
    }
    for variable in ["VCPKG_ROOT", "VCPKG_INSTALLATION_ROOT"] {
        if let Some(root) = env::var_os(variable) {
            candidates.push(PathBuf::from(root).join("installed").join(&triplet));
        }
    }
    if let Some(manifest_dir) = env::var_os("CARGO_MANIFEST_DIR") {
        let manifest_dir = PathBuf::from(manifest_dir);
        candidates.push(manifest_dir.join("vcpkg_installed").join(&triplet));
        let target_dir = manifest_dir.join("target");
        candidates.push(target_dir.join("vcpkg_installed").join(&triplet));
        if let Ok(entries) = fs::read_dir(&target_dir) {
            let mut nested_candidates = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path().join("vcpkg_installed").join(&triplet))
                .collect::<Vec<_>>();
            nested_candidates.sort();
            candidates.extend(nested_candidates);
        }
    }
    candidates.into_iter().find(|prefix| {
        prefix.join("include/freerdp3/freerdp/freerdp.h").is_file()
            && prefix.join("include/winpr3/winpr/wtypes.h").is_file()
            && WINDOWS_CORE_LIBRARIES
                .iter()
                .all(|library| prefix.join("lib").join(library).is_file())
            && WINDOWS_CORE_RUNTIME
                .iter()
                .all(|library| prefix.join("bin").join(library).is_file())
    })
}

fn target_is_windows() -> bool {
    env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
}

fn configured_libraries() -> Vec<String> {
    env::var("TINY_SHELL_FREERDP_LIBS")
        .unwrap_or_else(|_| FREERDP_PACKAGES.join(","))
        .split(',')
        .map(str::trim)
        .filter(|library| !library.is_empty())
        .map(str::to_string)
        .collect()
}

fn emit_pkg_config_link_metadata() {
    for package in FREERDP_PACKAGES {
        pkg_config::Config::new()
            .atleast_version("3")
            .probe(package)
            .unwrap_or_else(|error| {
                panic!("FreeRDP pkg-config metadata disappeared while building {package}: {error}")
            });
    }
}

fn infer_windows_runtime_dir(lib_dirs: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    lib_dirs
        .iter()
        .filter_map(|lib_dir| lib_dir.parent().map(|prefix| prefix.join("bin")))
        .find(|runtime_dir| windows_runtime_is_usable(runtime_dir))
}

fn validate_windows_paths(paths: &FreeRdpPaths) -> Result<(), String> {
    let libraries = configured_libraries();
    if libraries.is_empty() {
        return Err("TINY_SHELL_FREERDP_LIBS must contain at least one library name".into());
    }
    for library in &libraries {
        let import_library = format!("{library}.lib");
        if !paths
            .lib_dirs
            .iter()
            .any(|lib_dir| lib_dir.join(&import_library).is_file())
        {
            return Err(format!(
                "Windows import library {import_library} was not found in: {}",
                display_paths(&paths.lib_dirs)
            ));
        }
    }

    let runtime_dir = paths.runtime_dir.as_ref().ok_or_else(|| {
        "no FreeRDP runtime directory was found; set TINY_SHELL_FREERDP_RUNTIME_DIR".to_string()
    })?;
    if !windows_runtime_is_usable(runtime_dir) {
        let expected = configured_libraries()
            .into_iter()
            .map(|library| format!("{library}.dll"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "FreeRDP runtime directory {} does not contain all required DLLs: {expected}",
            runtime_dir.display()
        ));
    }
    Ok(())
}

fn windows_runtime_is_usable(runtime_dir: &Path) -> bool {
    configured_libraries()
        .into_iter()
        .all(|library| runtime_dir.join(format!("{library}.dll")).is_file())
}

fn display_paths(paths: &BTreeSet<PathBuf>) -> String {
    if paths.is_empty() {
        return "<none>".to_string();
    }
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn track_freerdp_inputs(paths: &FreeRdpPaths) {
    for include_dir in &paths.include_dirs {
        for header in ["freerdp/freerdp.h", "winpr/wtypes.h"] {
            let path = include_dir.join(header);
            if path.is_file() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    if target_is_windows() {
        for lib_dir in &paths.lib_dirs {
            for library in configured_libraries() {
                let path = lib_dir.join(format!("{library}.lib"));
                if path.is_file() {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
        }
    }
}

fn copy_windows_runtime_to_out_dir(runtime_dir: &Path) {
    clear_windows_runtime_from_out_dir();
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR"));
    let entries = fs::read_dir(runtime_dir).unwrap_or_else(|error| {
        panic!(
            "failed to read FreeRDP runtime directory {}: {error}",
            runtime_dir.display()
        )
    });
    let mut copied = 0usize;
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "failed to enumerate FreeRDP runtime directory {}: {error}",
                runtime_dir.display()
            )
        });
        let source = entry.path();
        if !source
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
        {
            continue;
        }
        let destination = out_dir.join(entry.file_name());
        fs::copy(&source, &destination).unwrap_or_else(|error| {
            panic!(
                "failed to copy FreeRDP runtime {} to {}: {error}",
                source.display(),
                destination.display()
            )
        });
        println!("cargo:rerun-if-changed={}", source.display());
        copied += 1;
    }
    assert!(
        copied > 0,
        "FreeRDP runtime directory {} does not contain DLL files",
        runtime_dir.display()
    );
    println!("cargo:rustc-link-search=native={}", out_dir.display());
}

fn clear_windows_runtime_from_out_dir() {
    if !target_is_windows() {
        return;
    }
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR"));
    let entries = match fs::read_dir(&out_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!(
            "failed to inspect Cargo output directory {}: {error}",
            out_dir.display()
        ),
    };
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "failed to enumerate Cargo output directory {}: {error}",
                out_dir.display()
            )
        });
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
        {
            fs::remove_file(&path).unwrap_or_else(|error| {
                panic!(
                    "failed to remove stale FreeRDP runtime {}: {error}",
                    path.display()
                )
            });
        }
    }
}
