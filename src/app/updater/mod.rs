use std::path::PathBuf;

use anyhow::Context;
use serde::Deserialize;

const REPO_OWNER: &str = "ynx-official";
const REPO_NAME: &str = "tiny-shell";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Maps the cargo target triple to the release asset naming convention used in
/// release.yml.
fn platform_name() -> &'static str {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x86_64"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "windows-x86_64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-aarch64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macos-x86_64"
    } else {
        "unknown"
    }
}

fn archive_extension() -> &'static str {
    if cfg!(target_os = "linux") {
        "tar.gz"
    } else {
        "zip"
    }
}

fn bin_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "tiny-shell.exe"
    } else {
        "tiny-shell"
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[allow(dead_code)]
    name: String,
    body: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub version: String,
    #[allow(dead_code)]
    pub notes: String,
    pub download_url: String,
    #[allow(dead_code)]
    pub size: u64,
}

#[derive(Debug, Clone)]
pub enum UpdateStatus {
    Checking,
    UpToDate,
    UpdateAvailable(UpdateInfo),
    Downloading,
    #[allow(dead_code)]
    Installing,
    InstallComplete,
    Error(String),
}

/// Check GitHub Releases for a newer version.
pub async fn check_for_update() -> anyhow::Result<Option<UpdateInfo>> {
    let client = reqwest::Client::builder()
        .user_agent(format!("{}/{}", REPO_NAME, CURRENT_VERSION))
        .build()
        .context("failed to build HTTP client")?;

    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        REPO_OWNER, REPO_NAME
    );

    let release: GitHubRelease = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .context("failed to fetch latest release")?
        .json()
        .await
        .context("failed to parse release JSON")?;

    let latest_version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);

    let current = semver::Version::parse(CURRENT_VERSION)
        .context("failed to parse current version")?;
    let latest = semver::Version::parse(latest_version)
        .context("failed to parse latest version")?;

    if latest <= current {
        return Ok(None);
    }

    // Find the asset matching our platform.
    let platform = platform_name();
    let ext = archive_extension();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(platform) && a.name.ends_with(ext))
        .or_else(|| {
            // Fallback: match any asset containing the platform name
            release.assets.iter().find(|a| a.name.contains(platform))
        });

    let Some(asset) = asset else {
        anyhow::bail!(
            "no release asset found for platform '{}' (ext: {})",
            platform,
            ext
        );
    };

    Ok(Some(UpdateInfo {
        version: latest_version.to_string(),
        notes: release.body,
        download_url: asset.browser_download_url.clone(),
        size: asset.size,
    }))
}

/// Download the update archive and extract the binary to a temp directory.
/// Returns the path to the extracted binary.
async fn download_and_extract(url: &str) -> anyhow::Result<PathBuf> {
    let client = reqwest::Client::builder()
        .user_agent(format!("{}/{}", REPO_NAME, CURRENT_VERSION))
        .build()
        .context("failed to build HTTP client")?;

    let response = client
        .get(url)
        .send()
        .await
        .context("failed to download update")?;

    let bytes = response
        .bytes()
        .await
        .context("failed to read update bytes")?;

    let temp_dir = std::env::temp_dir().join(format!("{}-update", REPO_NAME));
    std::fs::create_dir_all(&temp_dir).context("failed to create temp directory")?;

    let cursor = std::io::Cursor::new(bytes);

    if url.ends_with(".tar.gz") {
        let decoder = flate2::read::GzDecoder::new(cursor);
        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(&temp_dir)
            .context("failed to extract tar.gz archive")?;
    } else {
        let mut archive = zip::ZipArchive::new(cursor)
            .context("failed to open zip archive")?;
        archive
            .extract(&temp_dir)
            .context("failed to extract zip archive")?;
    }

    let bin = bin_name();

    // Find the binary in the extracted directory tree.
    let found = walkdir::WalkDir::new(&temp_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name() == bin);

    match found {
        Some(entry) => Ok(entry.path().to_path_buf()),
        None => {
            // On macOS, the release contains a .app bundle, not a standalone binary.
            #[cfg(target_os = "macos")]
            {
                let app = walkdir::WalkDir::new(&temp_dir)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .find(|e| {
                        e.path()
                            .extension()
                            .map_or(false, |ext| ext == "app")
                    });
                if let Some(app_entry) = app {
                    return Ok(app_entry.path().to_path_buf());
                }
            }
            anyhow::bail!("binary '{}' not found in downloaded archive", bin)
        }
    }
}

/// Perform the update: download, extract, and replace the current binary.
pub async fn perform_update(info: &UpdateInfo) -> anyhow::Result<()> {
    tracing::info!("downloading update from {}", info.download_url);
    let new_path = download_and_extract(&info.download_url).await?;

    let current_exe =
        std::env::current_exe().context("failed to get current executable path")?;

    #[cfg(target_os = "linux")]
    {
        install_linux(&new_path, &current_exe)?;
    }

    #[cfg(target_os = "macos")]
    {
        install_macos(&new_path, &current_exe)?;
    }

    #[cfg(target_os = "windows")]
    {
        install_windows(&new_path, &current_exe)?;
    }

    Ok(())
}

/// Restart the application by launching the new binary and exiting.
#[allow(dead_code)]
pub fn restart() -> ! {
    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("failed to get current exe path for restart: {e}");
            std::process::exit(1);
        }
    };

    tracing::info!("restarting application: {}", current_exe.display());

    #[cfg(target_os = "linux")]
    {
        let error = std::os::unix::process::CommandExt::exec(
            &mut std::process::Command::new(&current_exe),
        );
        tracing::error!("exec failed: {error}");
        std::process::exit(1);
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, relaunch the app bundle.
        if let Some(app_path) = current_exe
            .ancestors()
            .find(|p| p.extension().map_or(false, |ext| ext == "app"))
        {
            let _ = std::process::Command::new("open")
                .arg("-n")
                .arg(app_path)
                .spawn();
        } else {
            let error = std::os::unix::process::CommandExt::exec(
                &mut std::process::Command::new(&current_exe),
            );
            tracing::error!("exec failed: {error}");
        }
        std::process::exit(0);
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new(&current_exe).spawn();
        std::process::exit(0);
    }
}

// ── Platform-specific installation ──────────────────────────────────────────

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn install_linux(new_binary: &std::path::Path, current_exe: &std::path::Path) -> anyhow::Result<()> {
    // Write the new binary next to the old one with a .new suffix, then rename.
    // On Linux, rename() over an in-use file is safe — the old inode stays alive
    // until the last file descriptor is closed, so the running process is fine.
    let new_temp = current_exe.with_extension("new");
    std::fs::copy(new_binary, &new_temp)
        .with_context(|| format!("failed to copy new binary to {}", new_temp.display()))?;

    // Make it executable.
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&new_temp)
        .context("failed to get metadata of new binary")?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&new_temp, perms)
        .context("failed to set executable permissions")?;

    // Atomic rename.
    std::fs::rename(&new_temp, current_exe)
        .context("failed to rename new binary into place")?;

    tracing::info!("update installed to {}", current_exe.display());
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_macos(
    new_path: &std::path::Path,
    current_exe: &std::path::Path,
) -> anyhow::Result<()> {
    // Determine whether we are running from an .app bundle.
    let app_bundle = current_exe
        .ancestors()
        .find(|p| p.extension().map_or(false, |ext| ext == "app"));

    if let Some(app_bundle) = app_bundle {
        // new_path is either the extracted .app or a binary inside it.
        let new_app = if new_path.extension().map_or(false, |ext| ext == "app") {
            new_path.to_path_buf()
        } else {
            new_path
                .ancestors()
                .find(|p| p.extension().map_or(false, |ext| ext == "app"))
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| new_path.to_path_buf())
        };

        let parent = app_bundle
            .parent()
            .context("app bundle has no parent directory")?;

        // Move the old bundle aside, then move the new one in.
        let backup = parent.join(format!(
            "{}.old",
            app_bundle
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));
        if backup.exists() {
            std::fs::remove_dir_all(&backup).ok();
        }
        std::fs::rename(app_bundle, &backup)
            .context("failed to backup old app bundle")?;

        if let Err(e) = std::fs::rename(&new_app, app_bundle) {
            // Rollback: restore the backup.
            std::fs::rename(&backup, app_bundle).ok();
            return Err(e).context("failed to install new app bundle");
        }

        // Re-sign the new bundle (ad-hoc).
        let _ = std::process::Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(app_bundle)
            .output();

        // Remove the old backup in the background.
        let _ = std::fs::remove_dir_all(backup);
    } else {
        // Running as a standalone binary (not from .app).
        install_linux(new_path, current_exe)?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn install_windows(
    new_binary: &std::path::Path,
    current_exe: &std::path::Path,
) -> anyhow::Result<()> {
    // On Windows, the running .exe is locked. We write a batch script to a temp
    // location that waits for the current process to exit, then copies the new
    // binary over the old one and re-launches.
    let script_path = std::env::temp_dir().join("tiny-shell-update.bat");

    let script = format!(
        "@echo off\r\n\
         :wait\r\n\
         timeout /t 1 /nobreak >nul\r\n\
         if exist \"{current}\" (\r\n\
             del /f \"{current}\" 2>nul\r\n\
             if exist \"{current}\" goto wait\r\n\
         )\r\n\
         copy /y \"{new}\" \"{current}\" >nul\r\n\
         if exist \"{current}\" start \"\" \"{current}\"\r\n\
         del \"%~f0\" >nul 2>&1\r\n",
        current = current_exe.display(),
        new = new_binary.display(),
    );

    std::fs::write(&script_path, script)
        .context("failed to write update batch script")?;

    // Launch the script detached.
    std::process::Command::new("cmd.exe")
        .args(["/C", &script_path.to_string_lossy()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x00000008) // CREATE_NO_WINDOW on Windows
        .spawn()
        .context("failed to launch update batch script")?;

    tracing::info!("update batch script launched, exiting");
    Ok(())
}