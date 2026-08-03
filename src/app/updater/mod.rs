use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use anyhow::Context;
use futures::StreamExt;
use semver::Version;

mod release_source;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const REPO_NAME: &str = "tiny-shell";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(60);
/// Hard cap on how many bytes a single update download may buffer in memory
/// before we reject it. The release artifacts (portable zips, .pkg, setup.exe)
/// are all well under 200 MB today; 1 GB leaves ample headroom for future
/// growth while preventing a malicious or buggy server from triggering OOM by
/// streaming an unbounded body.
const MAX_UPDATE_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;

/// Monotonic counter combined with the process id and a high-resolution
/// timestamp to derive an unguessable suffix for per-update temp files. The
/// Windows update flow writes a PowerShell helper and a log under the system
/// temp dir; naming them only by pid lets a local attacker pre-create a
/// symlink or file at that predictable path to hijack the write. Mixing in a
/// nanosecond timestamp and an in-process counter makes the path unguessable
/// in practice without pulling in a new dependency.
#[cfg(target_os = "windows")]
fn unique_temp_suffix() -> String {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{pid}-{nanos:016x}-{n:08x}")
}
#[cfg(target_os = "windows")]
const INNO_UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{8E091D1C-6D7C-4C29-9CA2-8B3D84A42CF8}_is1";
// Matches the --identifier passed to pkgbuild in release.yml. pkgutil records
// a receipt under this id only when the .pkg is installed via installer or
// the GUI installer, which lets us distinguish it from a portable .app.
#[cfg(target_os = "macos")]
const MACOS_PKG_IDENTIFIER: &str = "dev.tiny-shell.app";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum InstallationKind {
    WindowsInstaller,
    Portable,
    MacApp,
    MacInstaller,
    LinuxPackage,
}

pub(crate) fn installation_kind() -> InstallationKind {
    #[cfg(target_os = "windows")]
    {
        return if windows_installer_matches_current_exe() {
            InstallationKind::WindowsInstaller
        } else {
            InstallationKind::Portable
        };
    }

    #[cfg(target_os = "macos")]
    {
        return if std::env::current_exe().ok().is_some_and(|path| {
            path.ancestors()
                .any(|ancestor| ancestor.extension().is_some_and(|ext| ext == "app"))
        }) {
            if macos_pkg_receipt_present() {
                InstallationKind::MacInstaller
            } else {
                InstallationKind::MacApp
            }
        } else {
            InstallationKind::Portable
        };
    }

    #[cfg(target_os = "linux")]
    {
        return if std::env::current_exe()
            .ok()
            .is_some_and(|path| path.starts_with("/usr/bin") || path.starts_with("/opt"))
        {
            InstallationKind::LinuxPackage
        } else {
            InstallationKind::Portable
        };
    }

    #[allow(unreachable_code)]
    InstallationKind::Portable
}

pub(crate) fn runtime_environment_label() -> String {
    let operating_system = if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        std::env::consts::OS
    };
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "ARM64",
        other => other,
    };
    format!("{operating_system} ({architecture})")
}

pub(crate) fn automatic_update_delay(
    interval_hours: u32,
    last_checked_at: i64,
    now: i64,
) -> Duration {
    if last_checked_at <= 0 {
        return Duration::ZERO;
    }
    let interval_seconds = (interval_hours.clamp(1, 8_760) as i64).saturating_mul(3_600);
    let elapsed = now.saturating_sub(last_checked_at).max(0);
    Duration::from_secs(interval_seconds.saturating_sub(elapsed).max(0) as u64)
}

#[cfg(target_os = "macos")]
fn macos_pkg_receipt_present() -> bool {
    // pkgutil returns a non-zero exit when no receipt exists for the id, so
    // success is a reliable signal that the .pkg installer was used.
    std::process::Command::new("pkgutil")
        .args(["--pkg-info-plist", MACOS_PKG_IDENTIFIER])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn windows_installer_matches_current_exe() -> bool {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER, enums::HKEY_LOCAL_MACHINE};

    let Ok(current_exe) = std::env::current_exe() else {
        return false;
    };
    let Some(current_dir) = current_exe.parent() else {
        return false;
    };

    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .filter_map(|hive| RegKey::predef(hive).open_subkey(INNO_UNINSTALL_KEY).ok())
        .filter_map(|key| key.get_value::<String, _>("InstallLocation").ok())
        .map(PathBuf::from)
        .any(|install_dir| paths_equal_case_insensitive(&install_dir, current_dir))
}

#[cfg(target_os = "windows")]
fn paths_equal_case_insensitive(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    normalize(left) == normalize(right)
}

fn parse_version(version: &str) -> anyhow::Result<Version> {
    let normalized = version
        .trim()
        .strip_prefix(['v', 'V'])
        .unwrap_or(version.trim());

    Version::parse(normalized).with_context(|| format!("failed to parse version '{version}'"))
}

fn is_newer_version(current: &str, latest: &str) -> anyhow::Result<bool> {
    let current_version = parse_version(current)?;
    let latest_version = parse_version(latest)?;
    // Compare as numeric SemVer, not as text. The previous implementation
    // compared the string forms of each segment, which made "10" sort below
    // "9" (since '1' < '9') and broke the update chain the moment the patch
    // number reached double digits. Numeric comparison matches the actual
    // release timeline (1.0.7 < 1.0.8 < 1.0.9 < 1.0.10 < ... < 1.0.63 < 1.0.71)
    // and lets pre-release / build metadata from semver still break ties.
    Ok(latest_version > current_version)
}

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

fn prepare_update_dir(temp_root: &Path, version: &str) -> anyhow::Result<PathBuf> {
    let version = parse_version(version)?;
    let update_dir = temp_root.join(format!("{REPO_NAME}-update-{version}"));

    if update_dir.exists() {
        std::fs::remove_dir_all(&update_dir).with_context(|| {
            format!(
                "failed to clear previous update directory {}",
                update_dir.display()
            )
        })?;
    }

    std::fs::create_dir_all(&update_dir)
        .with_context(|| format!("failed to create update directory {}", update_dir.display()))?;

    Ok(update_dir)
}

fn select_release_asset<'a>(
    assets: &'a [release_source::ReleaseAssetMetadata],
    platform: &str,
    archive_extension: &str,
    installation_kind: InstallationKind,
) -> Option<&'a release_source::ReleaseAssetMetadata> {
    let is_archive = |asset: &&release_source::ReleaseAssetMetadata| {
        asset.name.contains(platform) && asset.name.ends_with(archive_extension)
    };

    // Windows releases contain both a setup executable and a portable ZIP.
    // Keep the installed edition on its matching update path.
    if platform.starts_with("windows-") {
        match installation_kind {
            InstallationKind::WindowsInstaller => assets
                .iter()
                .find(|asset| asset.name.contains(platform) && asset.name.ends_with("-setup.exe")),
            _ => assets.iter().find(|asset| {
                asset.name.contains(platform)
                    && asset.name.contains("-portable.")
                    && asset.name.ends_with(archive_extension)
            }),
        }
    } else if platform.starts_with("macos-") {
        // macOS releases contain a portable .zip and a .pkg installer.
        // Mirror Windows: a .pkg-installed app updates via the .pkg, every
        // other .app (portable zip) updates via the portable .zip.
        match installation_kind {
            InstallationKind::MacInstaller => assets
                .iter()
                .find(|asset| asset.name.contains(platform) && asset.name.ends_with("-setup.pkg")),
            _ => assets.iter().find(|asset| {
                asset.name.contains(platform)
                    && asset.name.contains("-portable.")
                    && asset.name.ends_with(archive_extension)
            }),
        }
    } else {
        assets.iter().find(is_archive)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ReleaseInfo {
    pub version: String,
    pub notes: String,
}

#[derive(Debug, Clone)]
pub(crate) struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub download_url: String,
    pub size: u64,
    pub digest: String,
    pub installation_kind: InstallationKind,
}

#[derive(Debug, Clone)]
pub(crate) enum UpdateCheckResult {
    UpToDate(ReleaseInfo),
    UpdateAvailable(UpdateInfo),
}

#[derive(Debug, Clone)]
pub(crate) enum UpdateStatus {
    Checking,
    UpToDate(ReleaseInfo),
    UpdateAvailable(UpdateInfo),
    Downloading(UpdateInfo, u64, u64),
    DownloadCancelled(UpdateInfo),
    DownloadFailed(UpdateInfo, String),
    ReadyToRestart(UpdateInfo, PathBuf),
    Error(String),
}

#[derive(Debug, Default)]
struct DownloadCancellationInner {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DownloadCancellation {
    inner: Arc<DownloadCancellationInner>,
}

impl DownloadCancellation {
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Relaxed);
        self.inner.notify.notify_one();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Relaxed)
    }

    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.inner.notify.notified().await;
    }
}

/// Check the latest release and return both availability and release notes.
pub(crate) async fn check_for_update() -> anyhow::Result<UpdateCheckResult> {
    let client = reqwest::Client::builder()
        .user_agent(format!("{}/{}", REPO_NAME, CURRENT_VERSION))
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .read_timeout(HTTP_READ_TIMEOUT)
        .build()
        .context("failed to build HTTP client")?;
    let release = release_source::fetch_latest_release(&client).await?;
    let latest_version = release.version.trim();

    if !is_newer_version(CURRENT_VERSION, latest_version)? {
        return Ok(UpdateCheckResult::UpToDate(ReleaseInfo {
            version: parse_version(latest_version)?.to_string(),
            notes: release.notes,
        }));
    }

    let platform = platform_name();
    let ext = archive_extension();
    let installation_kind = installation_kind();
    let asset = select_release_asset(&release.assets, platform, ext, installation_kind);

    let Some(asset) = asset else {
        anyhow::bail!(
            "no release asset found for platform '{}' (ext: {})",
            platform,
            ext
        );
    };

    Ok(UpdateCheckResult::UpdateAvailable(UpdateInfo {
        version: parse_version(latest_version)?.to_string(),
        notes: release.notes,
        download_url: asset.download_url.clone(),
        size: asset.size,
        digest: asset.digest.clone(),
        installation_kind,
    }))
}

fn verify_download_digest(bytes: &[u8], expected_digest: &str) -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};

    let expected = expected_digest
        .strip_prefix("sha256:")
        .context("release asset digest is not SHA-256")?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("release asset contains an invalid SHA-256 digest");
    }

    let actual = hex::encode(Sha256::digest(bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        anyhow::bail!("downloaded update SHA-256 mismatch: expected {expected}, received {actual}");
    }
    Ok(())
}

/// Download the update archive and extract the binary to a temp directory.
/// Returns the path to the extracted binary.
async fn download_and_extract<F>(
    url: &str,
    version: &str,
    expected_size: u64,
    expected_digest: &str,
    installation_kind: InstallationKind,
    cancellation: &DownloadCancellation,
    mut on_progress: F,
) -> anyhow::Result<PathBuf>
where
    F: FnMut(u64, u64),
{
    let client = reqwest::Client::builder()
        .user_agent(format!("{}/{}", REPO_NAME, CURRENT_VERSION))
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .read_timeout(HTTP_READ_TIMEOUT)
        .build()
        .context("failed to build HTTP client")?;

    let response = tokio::select! {
        _ = cancellation.cancelled() => anyhow::bail!("update download cancelled"),
        response = client.get(url).send() => response.context("failed to download update")?,
    }
    .error_for_status()
    .context("update download returned an error status")?;

    let total = response.content_length().unwrap_or(expected_size);
    // Cap the initial allocation at the hard limit; a server claiming a huge
    // content-length must not trick us into reserving gigabytes up front.
    let reserve = total.min(MAX_UPDATE_DOWNLOAD_BYTES) as usize;
    let mut downloaded = 0_u64;
    let mut bytes = Vec::with_capacity(reserve);
    let mut stream = response.bytes_stream();
    loop {
        let chunk = tokio::select! {
            _ = cancellation.cancelled() => anyhow::bail!("update download cancelled"),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk.context("failed to read update bytes")?;
        downloaded += chunk.len() as u64;
        if downloaded > MAX_UPDATE_DOWNLOAD_BYTES {
            anyhow::bail!(
                "update download exceeded the {MAX_UPDATE_DOWNLOAD_BYTES} byte safety limit (got {downloaded} bytes); refusing to buffer further"
            );
        }
        bytes.extend_from_slice(&chunk);
        on_progress(downloaded, total);
    }

    if cancellation.is_cancelled() {
        anyhow::bail!("update download cancelled");
    }

    if expected_size > 0 && downloaded != expected_size {
        anyhow::bail!(
            "downloaded update size mismatch: expected {expected_size} bytes, received {downloaded} bytes"
        );
    }
    verify_download_digest(&bytes, expected_digest)?;

    // Release archives contain a versioned top-level directory. Reusing one
    // extraction directory lets older binaries accumulate, and a recursive
    // search can then select a stale executable instead of the downloaded one.
    let temp_dir = prepare_update_dir(&std::env::temp_dir(), version)?;

    // Installer editions (Windows setup.exe, macOS .pkg) are not unpacked —
    // they are handed to the platform installer verbatim. Persist the bytes to
    // a stable path and fsync so a crash before launch does not leave a
    // half-written installer on disk.
    let installer_suffix = match installation_kind {
        InstallationKind::WindowsInstaller => Some("exe"),
        InstallationKind::MacInstaller => Some("pkg"),
        _ => None,
    };
    if let Some(suffix) = installer_suffix {
        let installer_path = temp_dir.join(format!("tiny-shell-v{version}-setup.{suffix}"));
        std::fs::write(&installer_path, &bytes).with_context(|| {
            format!(
                "failed to write downloaded installer {}",
                installer_path.display()
            )
        })?;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&installer_path)
            .with_context(|| {
                format!(
                    "failed to open downloaded installer {}",
                    installer_path.display()
                )
            })?
            .sync_all()
            .context("failed to flush downloaded installer to disk")?;
        return Ok(installer_path);
    }

    let cursor = std::io::Cursor::new(bytes);

    if url.ends_with(".tar.gz") {
        let decoder = flate2::read::GzDecoder::new(cursor);
        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(&temp_dir)
            .context("failed to extract tar.gz archive")?;
    } else {
        let mut archive = zip::ZipArchive::new(cursor).context("failed to open zip archive")?;
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
                    .find(|e| e.path().extension().is_some_and(|ext| ext == "app"));
                if let Some(app_entry) = app {
                    return Ok(app_entry.path().to_path_buf());
                }
            }
            anyhow::bail!("binary '{}' not found in downloaded archive", bin)
        }
    }
}

/// Download and extract an update without touching the running application.
pub(crate) async fn download_update<F>(
    info: &UpdateInfo,
    cancellation: &DownloadCancellation,
    on_progress: F,
) -> anyhow::Result<PathBuf>
where
    F: FnMut(u64, u64),
{
    tracing::info!("downloading update from {}", info.download_url);
    let prepared_path = download_and_extract(
        &info.download_url,
        &info.version,
        info.size,
        &info.digest,
        info.installation_kind,
        cancellation,
        on_progress,
    )
    .await?;

    #[cfg(target_os = "windows")]
    if info.installation_kind == InstallationKind::Portable {
        return stage_windows_portable_binary(&prepared_path);
    }

    Ok(prepared_path)
}

#[cfg(target_os = "windows")]
fn stage_windows_portable_binary(new_binary: &Path) -> anyhow::Result<PathBuf> {
    let current_exe = std::env::current_exe().context("failed to get current executable path")?;
    stage_windows_portable_binary_at(new_binary, &current_exe, std::process::id())
}

#[cfg(target_os = "windows")]
fn stage_windows_portable_binary_at(
    new_binary: &Path,
    current_exe: &Path,
    process_id: u32,
) -> anyhow::Result<PathBuf> {
    let source_size = std::fs::metadata(new_binary)
        .with_context(|| format!("failed to inspect update binary {}", new_binary.display()))?
        .len();
    if source_size == 0 {
        anyhow::bail!("downloaded update binary is empty");
    }

    let install_dir = current_exe
        .parent()
        .context("current executable has no parent directory")?;
    let staged_binary = install_dir.join(format!(".tiny-shell-update-{process_id}.exe"));
    if staged_binary.exists() {
        std::fs::remove_file(&staged_binary).with_context(|| {
            format!(
                "failed to remove stale staged update {}",
                staged_binary.display()
            )
        })?;
    }

    let copied_size = std::fs::copy(new_binary, &staged_binary).with_context(|| {
        format!(
            "failed to stage update next to the installed executable at {}",
            staged_binary.display()
        )
    })?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&staged_binary)
        .with_context(|| format!("failed to open staged update {}", staged_binary.display()))?
        .sync_all()
        .context("failed to flush staged update to disk")?;

    if copied_size != source_size {
        std::fs::remove_file(&staged_binary).ok();
        anyhow::bail!(
            "staged update size mismatch: expected {source_size} bytes, copied {copied_size} bytes"
        );
    }

    Ok(staged_binary)
}

/// Install a prepared update, then close this process while the new version starts.
pub(crate) fn install_and_restart(
    new_path: &std::path::Path,
    expected_version: &str,
    installation_kind: InstallationKind,
) -> anyhow::Result<()> {
    let current_exe = std::env::current_exe().context("failed to get current executable path")?;
    #[cfg(target_os = "linux")]
    let _ = (expected_version, installation_kind);

    #[cfg(target_os = "linux")]
    {
        install_linux(new_path, &current_exe)?;
    }

    #[cfg(target_os = "macos")]
    {
        match installation_kind {
            InstallationKind::MacInstaller => {
                install_macos_pkg(new_path, &current_exe, expected_version)?
            }
            _ => install_macos(new_path, &current_exe)?,
        }
    }

    #[cfg(target_os = "windows")]
    {
        match installation_kind {
            InstallationKind::WindowsInstaller => {
                install_windows_setup(new_path, &current_exe, expected_version)?
            }
            _ => install_windows_portable(new_path, &current_exe, expected_version)?,
        }
    }

    restart()
}

/// Restart the application by launching the new binary and exiting.
fn restart() -> ! {
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
        let error =
            std::os::unix::process::CommandExt::exec(&mut std::process::Command::new(&current_exe));
        tracing::error!("exec failed: {error}");
        std::process::exit(1);
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, relaunch the app bundle.
        if let Some(app_path) = current_exe
            .ancestors()
            .find(|p| p.extension().is_some_and(|ext| ext == "app"))
        {
            let _ = std::process::Command::new("open")
                .arg("-n")
                .arg(app_path)
                .spawn();
        } else {
            let error = std::os::unix::process::CommandExt::exec(&mut std::process::Command::new(
                &current_exe,
            ));
            tracing::error!("exec failed: {error}");
        }
        std::process::exit(0);
    }

    #[cfg(target_os = "windows")]
    {
        // The updater script replaces this locked executable after this
        // process exits, then launches the replacement.
        std::process::exit(0);
    }
}

// ── Platform-specific installation ──────────────────────────────────────────

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn install_linux(
    new_binary: &std::path::Path,
    current_exe: &std::path::Path,
) -> anyhow::Result<()> {
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
    std::fs::set_permissions(&new_temp, perms).context("failed to set executable permissions")?;

    // Atomic rename.
    std::fs::rename(&new_temp, current_exe).context("failed to rename new binary into place")?;

    tracing::info!("update installed to {}", current_exe.display());
    Ok(())
}

/// Recursively copy a directory tree. Unlike `std::fs::rename`, this works
/// across volumes and leaves the source intact, so a failure mid-copy does
/// not corrupt the running app bundle. Used when staging a new .app before
/// the atomic swap into place.
#[cfg(target_os = "macos")]
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_all(&path, &target)?;
        } else if ft.is_symlink() {
            let link_target = std::fs::read_link(&path)?;
            std::os::unix::fs::symlink(&link_target, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_macos(new_path: &std::path::Path, current_exe: &std::path::Path) -> anyhow::Result<()> {
    // Determine whether we are running from an .app bundle.
    let app_bundle = current_exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|ext| ext == "app"));

    if let Some(app_bundle) = app_bundle {
        // new_path is either the extracted .app or a binary inside it.
        let new_app = if new_path.extension().is_some_and(|ext| ext == "app") {
            new_path.to_path_buf()
        } else {
            new_path
                .ancestors()
                .find(|p| p.extension().is_some_and(|ext| ext == "app"))
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| new_path.to_path_buf())
        };

        let parent = app_bundle
            .parent()
            .context("app bundle has no parent directory")?;

        // Stage the new bundle next to the live one as `<name>.new`. Copying
        // (rather than renaming the live bundle aside first) keeps the running
        // app's bundle intact while the new tree is written, so a copy failure
        // leaves the user on the previous version instead of with no app at
        // all. Only after a successful copy do we swap the bundles.
        let staging = parent.join(format!(
            "{}.new",
            app_bundle.file_name().unwrap_or_default().to_string_lossy()
        ));
        if staging.exists() {
            std::fs::remove_dir_all(&staging).with_context(|| {
                format!("failed to clear stale staging dir {}", staging.display())
            })?;
        }
        copy_dir_all(&new_app, &staging)
            .with_context(|| format!("failed to stage new app bundle at {}", staging.display()))?;

        // Swap: move the live bundle aside, move the staged one into place,
        // then delete the old bundle. If the move-into-place fails we restore
        // the live bundle from the sidecar.
        let backup = parent.join(format!(
            "{}.old",
            app_bundle.file_name().unwrap_or_default().to_string_lossy()
        ));
        if backup.exists() {
            std::fs::remove_dir_all(&backup).ok();
        }
        std::fs::rename(app_bundle, &backup).context("failed to backup old app bundle")?;

        if let Err(e) = std::fs::rename(&staging, app_bundle) {
            // Rollback: restore the live bundle from the sidecar.
            std::fs::rename(&backup, app_bundle).ok();
            std::fs::remove_dir_all(&staging).ok();
            return Err(e).context("failed to install new app bundle");
        }

        // Re-sign the new bundle (ad-hoc). Log on failure instead of staying
        // silent: a failed re-sign leaves Gatekeeper blocking the app on next
        // launch, which is worth surfacing in the log.
        let codesign = std::process::Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(app_bundle)
            .output();
        if let Err(e) = &codesign {
            tracing::warn!("ad-hoc re-sign of {} failed: {e}", app_bundle.display());
        } else if let Ok(out) = &codesign {
            if !out.status.success() {
                tracing::warn!(
                    "ad-hoc re-sign of {} failed: {}",
                    app_bundle.display(),
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
        }

        // Remove the old backup in the background.
        let _ = std::fs::remove_dir_all(backup);
    } else {
        // Running as a standalone binary (not from .app).
        install_linux(new_path, current_exe)?;
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn install_macos_pkg(
    pkg_path: &std::path::Path,
    _current_exe: &std::path::Path,
    expected_version: &str,
) -> anyhow::Result<()> {
    // The .pkg's install-location targets /Applications, so install into the
    // root domain and let the package lay the bundle down itself. installer
    // requires root; elevate via osascript, which shows the standard macOS
    // admin password prompt. The running process keeps its old inode alive,
    // and restart() relaunches the freshly installed bundle.
    let pkg = pkg_path.to_string_lossy().replace('\'', "'\\''");
    let shell_command = format!("/usr/sbin/installer -pkg '{pkg}' -target /");
    let apple_script = format!(
        "do shell script \"{}\" with administrator privileges",
        shell_command.replace('"', "\\\"")
    );

    let output = std::process::Command::new("osascript")
        .args(["-e", &apple_script])
        .output()
        .context("failed to invoke installer via osascript")?;
    if !output.status.success() {
        anyhow::bail!(
            "pkg installer failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    // Verify the installed bundle matches the expected version before
    // restarting. A silent installer failure (e.g. the old bundle was in use
    // and the new one could not replace it) would otherwise leave the app
    // running the previous version with no error surfaced. Compare as plain
    // strings: CFBundleShortVersionString is written verbatim from the plist
    // template, so it must equal the release version (with any leading 'v'
    // stripped) character-for-character.
    let installed_app = std::path::Path::new("/Applications/TinyShell.app");
    let installed_version = std::process::Command::new("defaults")
        .args([
            "read",
            &installed_app.join("Contents/Info.plist").to_string_lossy(),
            "CFBundleShortVersionString",
        ])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        });
    let expected = expected_version.trim_start_matches('v');
    match installed_version.as_deref() {
        Some(v) if v == expected => Ok(()),
        Some(v) => anyhow::bail!("installed pkg version mismatch: expected {expected}, got {v}"),
        None => anyhow::bail!(
            "failed to read installed version from {} after pkg install",
            installed_app.display()
        ),
    }
}

#[cfg(target_os = "windows")]
fn install_windows_setup(
    installer: &std::path::Path,
    current_exe: &std::path::Path,
    expected_version: &str,
) -> anyhow::Result<()> {
    let expected_version = parse_version(expected_version)?.to_string();
    let installer_size = std::fs::metadata(installer)
        .with_context(|| format!("failed to inspect update installer {}", installer.display()))?
        .len();
    if installer_size == 0 {
        anyhow::bail!("downloaded update installer is empty");
    }

    let process_id = std::process::id();
    let suffix = unique_temp_suffix();
    let script_path = std::env::temp_dir().join(format!("tiny-shell-setup-update-{suffix}.ps1"));
    let log_path = std::env::temp_dir().join(format!("tiny-shell-update-{suffix}.log"));
    let plan = WindowsSetupUpdateScriptPlan {
        process_id,
        current_exe,
        installer,
        script_path: &script_path,
        log_path: &log_path,
        expected_version: &expected_version,
    };

    if let Err(error) =
        launch_windows_update_script(&script_path, build_windows_setup_update_script(&plan))
    {
        std::fs::remove_file(&script_path).ok();
        return Err(error);
    }

    tracing::info!(
        "setup update helper launched for process {process_id}; installer: {}",
        installer.display()
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_windows_portable(
    new_binary: &std::path::Path,
    current_exe: &std::path::Path,
    expected_version: &str,
) -> anyhow::Result<()> {
    let expected_version = parse_version(expected_version)?.to_string();
    let source_size = std::fs::metadata(new_binary)
        .with_context(|| format!("failed to inspect staged update {}", new_binary.display()))?
        .len();
    if source_size == 0 {
        anyhow::bail!("staged update binary is empty");
    }

    let process_id = std::process::id();
    let install_dir = current_exe
        .parent()
        .context("current executable has no parent directory")?;
    let staged_binary = new_binary.to_path_buf();
    if staged_binary.parent() != Some(install_dir) {
        anyhow::bail!(
            "staged update is not next to the installed executable: {}",
            staged_binary.display()
        );
    }
    let backup_binary = install_dir.join(format!(".tiny-shell-backup-{process_id}.exe"));
    let suffix = unique_temp_suffix();
    let script_path = std::env::temp_dir().join(format!("tiny-shell-update-{suffix}.ps1"));
    let log_path = std::env::temp_dir().join(format!("tiny-shell-update-{suffix}.log"));

    let script_plan = WindowsUpdateScriptPlan {
        process_id,
        current_exe,
        staged_binary: &staged_binary,
        backup_binary: &backup_binary,
        script_path: &script_path,
        log_path: &log_path,
        expected_size: source_size,
        expected_version: &expected_version,
    };
    if let Err(error) =
        launch_windows_update_script(&script_path, build_windows_update_script(&script_plan))
    {
        std::fs::remove_file(&staged_binary).ok();
        std::fs::remove_file(&script_path).ok();
        return Err(error);
    }

    tracing::info!(
        "update helper launched for process {process_id}; staged binary: {}",
        staged_binary.display()
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn launch_windows_update_script(script_path: &Path, script: String) -> anyhow::Result<()> {
    std::fs::write(script_path, script).context("failed to write update PowerShell script")?;
    std::process::Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn()
        .context("failed to launch update PowerShell script")?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn powershell_path_literal(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

#[cfg(target_os = "windows")]
struct WindowsSetupUpdateScriptPlan<'a> {
    process_id: u32,
    current_exe: &'a Path,
    installer: &'a Path,
    script_path: &'a Path,
    log_path: &'a Path,
    expected_version: &'a str,
}

#[cfg(target_os = "windows")]
fn build_windows_setup_update_script(plan: &WindowsSetupUpdateScriptPlan<'_>) -> String {
    let current_exe = powershell_path_literal(plan.current_exe);
    let installer = powershell_path_literal(plan.installer);
    let script_path = powershell_path_literal(plan.script_path);
    let log_path = powershell_path_literal(plan.log_path);
    let expected_version = powershell_string_literal(plan.expected_version);
    let process_id = plan.process_id;

    format!(
        "$ErrorActionPreference = 'Stop'\r\n\
         $current = {current_exe}\r\n\
         $installer = {installer}\r\n\
         $script = {script_path}\r\n\
         $log = {log_path}\r\n\
         $expectedVersion = {expected_version}\r\n\
         function Write-UpdateLog([string]$message) {{\r\n\
             try {{\r\n\
                 Add-Content -LiteralPath $log -Value \"$(Get-Date -Format o) $message\" -Encoding UTF8\r\n\
             }} catch {{}}\r\n\
         }}\r\n\
         try {{\r\n\
             Write-UpdateLog 'waiting for application process {process_id} before setup update'\r\n\
             Wait-Process -Id {process_id} -ErrorAction SilentlyContinue\r\n\
             $arguments = @('/SP-', '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER')\r\n\
             $setup = Start-Process -FilePath $installer -ArgumentList $arguments -Wait -PassThru\r\n\
             if ($setup.ExitCode -ne 0) {{\r\n\
                 throw \"setup exited with code $($setup.ExitCode)\"\r\n\
             }}\r\n\
             $installedVersion = (Get-Item -LiteralPath $current).VersionInfo.ProductVersion\r\n\
             if ($installedVersion -ne $expectedVersion) {{\r\n\
                 throw \"installed setup version mismatch: expected $expectedVersion, got $installedVersion\"\r\n\
             }}\r\n\
             Start-Process -FilePath $current -WorkingDirectory (Split-Path -Parent $current)\r\n\
             Write-UpdateLog 'setup update installed and application restarted successfully'\r\n\
             Remove-Item -LiteralPath $installer -Force -ErrorAction SilentlyContinue\r\n\
             Remove-Item -LiteralPath $script -Force -ErrorAction SilentlyContinue\r\n\
         }} catch {{\r\n\
             Write-UpdateLog \"setup update failed: $($_.Exception.Message)\"\r\n\
             if (Test-Path -LiteralPath $current) {{\r\n\
                 Start-Process -FilePath $current -WorkingDirectory (Split-Path -Parent $current)\r\n\
             }}\r\n\
             exit 1\r\n\
         }}\r\n"
    )
}

#[cfg(target_os = "windows")]
struct WindowsUpdateScriptPlan<'a> {
    process_id: u32,
    current_exe: &'a std::path::Path,
    staged_binary: &'a std::path::Path,
    backup_binary: &'a std::path::Path,
    script_path: &'a std::path::Path,
    log_path: &'a std::path::Path,
    expected_size: u64,
    expected_version: &'a str,
}

#[cfg(target_os = "windows")]
fn build_windows_update_script(plan: &WindowsUpdateScriptPlan<'_>) -> String {
    let current_exe = powershell_path_literal(plan.current_exe);
    let staged_binary = powershell_path_literal(plan.staged_binary);
    let backup_binary = powershell_path_literal(plan.backup_binary);
    let script_path = powershell_path_literal(plan.script_path);
    let log_path = powershell_path_literal(plan.log_path);
    let expected_version = powershell_string_literal(plan.expected_version);
    let process_id = plan.process_id;
    let expected_size = plan.expected_size;

    format!(
        "$ErrorActionPreference = 'Stop'\r\n\
         $current = {current_exe}\r\n\
         $staged = {staged_binary}\r\n\
         $backup = {backup_binary}\r\n\
         $script = {script_path}\r\n\
         $log = {log_path}\r\n\
         $expectedVersion = {expected_version}\r\n\
         function Write-UpdateLog([string]$message) {{\r\n\
             try {{\r\n\
                 Add-Content -LiteralPath $log -Value \"$(Get-Date -Format o) $message\" -Encoding UTF8\r\n\
             }} catch {{}}\r\n\
         }}\r\n\
         try {{\r\n\
             Write-UpdateLog 'waiting for application process {process_id} to exit'\r\n\
             Wait-Process -Id {process_id} -ErrorAction SilentlyContinue\r\n\
             Start-Sleep -Milliseconds 300\r\n\
             $stagedVersion = (Get-Item -LiteralPath $staged).VersionInfo.ProductVersion\r\n\
             if ($stagedVersion -ne $expectedVersion) {{\r\n\
                 throw \"staged update version mismatch: expected $expectedVersion, got $stagedVersion\"\r\n\
             }}\r\n\
             if (Test-Path -LiteralPath $backup) {{\r\n\
                 Remove-Item -LiteralPath $backup -Force\r\n\
             }}\r\n\
             $moved = $false\r\n\
             for ($attempt = 1; $attempt -le 20; $attempt++) {{\r\n\
                 try {{\r\n\
                     Move-Item -LiteralPath $current -Destination $backup -Force\r\n\
                     $moved = $true\r\n\
                     break\r\n\
                 }} catch {{\r\n\
                     if ($attempt -eq 20) {{ throw }}\r\n\
                     Start-Sleep -Milliseconds 250\r\n\
                 }}\r\n\
             }}\r\n\
             if (-not $moved) {{ throw 'failed to move installed executable to backup' }}\r\n\
             Move-Item -LiteralPath $staged -Destination $current -Force\r\n\
             $installedSize = (Get-Item -LiteralPath $current).Length\r\n\
             if ($installedSize -ne {expected_size}) {{\r\n\
                 throw \"installed update size mismatch: expected {expected_size}, got $installedSize\"\r\n\
             }}\r\n\
             Start-Process -FilePath $current -WorkingDirectory (Split-Path -Parent $current)\r\n\
             Remove-Item -LiteralPath $backup -Force\r\n\
             Write-UpdateLog 'update installed and application restarted successfully'\r\n\
             Remove-Item -LiteralPath $script -Force -ErrorAction SilentlyContinue\r\n\
         }} catch {{\r\n\
             Write-UpdateLog \"update failed: $($_.Exception.Message)\"\r\n\
             try {{\r\n\
                 if (Test-Path -LiteralPath $backup) {{\r\n\
                     if (Test-Path -LiteralPath $current) {{\r\n\
                         Remove-Item -LiteralPath $current -Force\r\n\
                     }}\r\n\
                     Move-Item -LiteralPath $backup -Destination $current -Force\r\n\
                 }}\r\n\
                 if (Test-Path -LiteralPath $current) {{\r\n\
                     Start-Process -FilePath $current -WorkingDirectory (Split-Path -Parent $current)\r\n\
                 }}\r\n\
             }} catch {{\r\n\
                 Write-UpdateLog \"rollback failed: $($_.Exception.Message)\"\r\n\
             }}\r\n\
             exit 1\r\n\
         }}\r\n"
    )
}

#[cfg(target_os = "windows")]
fn powershell_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::release_source::ReleaseAssetMetadata;
    use super::{
        DownloadCancellation, InstallationKind, is_newer_version, prepare_update_dir,
        select_release_asset, verify_download_digest,
    };
    #[cfg(target_os = "windows")]
    use super::{
        WindowsSetupUpdateScriptPlan, WindowsUpdateScriptPlan, build_windows_setup_update_script,
        build_windows_update_script, powershell_path_literal, powershell_string_literal,
        stage_windows_portable_binary_at,
    };

    fn asset(name: &str) -> ReleaseAssetMetadata {
        ReleaseAssetMetadata {
            name: name.to_string(),
            download_url: format!("https://example.invalid/{name}"),
            size: 1,
            digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
        }
    }

    #[test]
    fn verifies_release_asset_sha256_digest() {
        verify_download_digest(
            b"",
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        )
        .unwrap();
    }

    #[test]
    fn rejects_release_asset_sha256_mismatch() {
        let error = verify_download_digest(
            b"modified",
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        )
        .unwrap_err();
        assert!(error.to_string().contains("SHA-256 mismatch"));
    }

    #[test]
    fn update_download_cancellation_is_shared_between_clones() {
        let cancellation = DownloadCancellation::default();
        let cloned = cancellation.clone();
        assert!(!cancellation.is_cancelled());

        cloned.cancel();

        assert!(cancellation.is_cancelled());
    }

    #[cfg(target_os = "windows")]
    fn assert_powershell_script_parses(script: &str, name: &str) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let script_path = std::env::temp_dir().join(format!(
            "tiny-shell-{name}-{}-{unique}.ps1",
            std::process::id()
        ));
        std::fs::write(&script_path, script).unwrap();
        let parse_command = format!(
            "$tokens = $null; $errors = $null; \
             [System.Management.Automation.Language.Parser]::ParseFile({}, [ref]$tokens, [ref]$errors) | Out-Null; \
             if ($errors.Count -gt 0) {{ $errors | ForEach-Object {{ Write-Error $_.Message }}; exit 1 }}",
            powershell_path_literal(&script_path)
        );
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
            .arg(parse_command)
            .status()
            .unwrap();
        std::fs::remove_file(script_path).unwrap();
        assert!(status.success());
    }

    #[test]
    fn windows_updater_prefers_portable_archive_over_installer() {
        let assets = vec![
            asset("tiny-shell-v1.1.0-windows-x86_64-setup.exe"),
            asset("tiny-shell-v1.1.0-windows-x86_64-portable.zip"),
        ];

        let selected =
            select_release_asset(&assets, "windows-x86_64", "zip", InstallationKind::Portable)
                .unwrap();
        assert_eq!(
            selected.name,
            "tiny-shell-v1.1.0-windows-x86_64-portable.zip"
        );
    }

    #[test]
    fn windows_setup_install_selects_installer_asset() {
        let assets = vec![
            asset("tiny-shell-v1.1.0-windows-x86_64-setup.exe"),
            asset("tiny-shell-v1.1.0-windows-x86_64-portable.zip"),
        ];

        let selected = select_release_asset(
            &assets,
            "windows-x86_64",
            "zip",
            InstallationKind::WindowsInstaller,
        )
        .unwrap();
        assert_eq!(selected.name, "tiny-shell-v1.1.0-windows-x86_64-setup.exe");
    }

    #[test]
    fn macos_installer_selects_pkg_asset() {
        let assets = vec![
            asset("tiny-shell-v1.1.0-macos-aarch64-portable.zip"),
            asset("tiny-shell-v1.1.0-macos-aarch64-setup.pkg"),
        ];

        let selected = select_release_asset(
            &assets,
            "macos-aarch64",
            "zip",
            InstallationKind::MacInstaller,
        )
        .unwrap();
        assert_eq!(selected.name, "tiny-shell-v1.1.0-macos-aarch64-setup.pkg");
    }

    #[test]
    fn macos_portable_selects_zip_asset() {
        let assets = vec![
            asset("tiny-shell-v1.1.0-macos-aarch64-portable.zip"),
            asset("tiny-shell-v1.1.0-macos-aarch64-setup.pkg"),
        ];

        let selected =
            select_release_asset(&assets, "macos-aarch64", "zip", InstallationKind::MacApp)
                .unwrap();
        assert_eq!(
            selected.name,
            "tiny-shell-v1.1.0-macos-aarch64-portable.zip"
        );
    }

    #[test]
    fn compares_version_segments_using_release_order() {
        // Numeric SemVer ordering: matches the actual release timeline
        // (1.0.7 < 1.0.8 < 1.0.9 < 1.0.10 < ... < 1.0.63 < 1.0.71).
        assert!(is_newer_version("v1.0.1", "v1.0.2").unwrap());
        // 11 is newer than 2 under numeric ordering (the old text-based
        // comparison wrongly treated "2" as greater than "11").
        assert!(is_newer_version("v1.0.2", "v1.0.11").unwrap());
        assert!(!is_newer_version("v1.0.11", "v1.0.2").unwrap());
        assert!(is_newer_version("v1.0.2", "v1.0.21").unwrap());
        assert!(!is_newer_version("v1.0.21", "v1.0.2").unwrap());
        assert!(!is_newer_version("v1.0.11", "v1.0.11").unwrap());
        // Double-digit transitions that the old text comparison broke:
        // "10" must sort above "9", "71" above "9".
        assert!(is_newer_version("v1.0.9", "v1.0.10").unwrap());
        assert!(!is_newer_version("v1.0.10", "v1.0.9").unwrap());
        assert!(is_newer_version("v1.0.9", "v1.0.71").unwrap());
        assert!(is_newer_version("v1.0.63", "v1.0.71").unwrap());
    }

    #[test]
    fn accepts_common_release_tag_formatting() {
        assert!(is_newer_version(" 1.0.1 ", " V1.0.2 ").unwrap());
    }

    #[test]
    fn automatic_update_delay_respects_last_check_time() {
        assert_eq!(super::automatic_update_delay(24, 0, 1_000), Duration::ZERO);
        assert_eq!(
            super::automatic_update_delay(24, 1_000, 1_000 + 3_600),
            Duration::from_secs(23 * 3_600)
        );
        assert_eq!(
            super::automatic_update_delay(24, 1_000, 1_000 + 24 * 3_600),
            Duration::ZERO
        );
        assert_eq!(
            super::automatic_update_delay(24, 2_000, 1_000),
            Duration::from_secs(24 * 3_600)
        );
    }

    #[test]
    fn update_directory_is_reset_for_each_download() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!(
            "tiny-shell-updater-test-{}-{unique}",
            std::process::id()
        ));
        let update_dir = prepare_update_dir(&temp_root, "v1.0.61").unwrap();
        let stale_binary = update_dir.join("old-release").join("tiny-shell.exe");
        std::fs::create_dir_all(stale_binary.parent().unwrap()).unwrap();
        std::fs::write(&stale_binary, b"old version").unwrap();

        let prepared_again = prepare_update_dir(&temp_root, "v1.0.61").unwrap();

        assert_eq!(prepared_again, update_dir);
        assert!(!stale_binary.exists());
        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn powershell_paths_escape_single_quotes() {
        let path = std::path::Path::new(r"C:\Users\O'Brien\TinyShell.exe");

        assert_eq!(
            powershell_path_literal(path),
            r"'C:\Users\O''Brien\TinyShell.exe'"
        );
        assert_eq!(
            powershell_string_literal("1.0.7'preview"),
            "'1.0.7''preview'"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn portable_update_is_staged_next_to_installed_executable() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!(
            "tiny-shell-stage-test-{}-{unique}",
            std::process::id()
        ));
        let download_dir = temp_root.join("download");
        let install_dir = temp_root.join("install");
        std::fs::create_dir_all(&download_dir).unwrap();
        std::fs::create_dir_all(&install_dir).unwrap();
        let downloaded = download_dir.join("tiny-shell.exe");
        let current = install_dir.join("tiny-shell.exe");
        std::fs::write(&downloaded, b"new executable").unwrap();
        std::fs::write(&current, b"old executable").unwrap();

        let staged = stage_windows_portable_binary_at(&downloaded, &current, 4242).unwrap();

        assert_eq!(staged, install_dir.join(".tiny-shell-update-4242.exe"));
        assert_eq!(std::fs::read(staged).unwrap(), b"new executable");
        assert_eq!(std::fs::read(current).unwrap(), b"old executable");
        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_update_script_waits_verifies_and_rolls_back() {
        let plan = WindowsUpdateScriptPlan {
            process_id: 4242,
            current_exe: std::path::Path::new(r"C:\TinyShell\tiny-shell.exe"),
            staged_binary: std::path::Path::new(r"C:\TinyShell\.tiny-shell-update-4242.exe"),
            backup_binary: std::path::Path::new(r"C:\TinyShell\.tiny-shell-backup-4242.exe"),
            script_path: std::path::Path::new(r"C:\Temp\tiny-shell-update-4242.ps1"),
            log_path: std::path::Path::new(r"C:\Temp\tiny-shell-update.log"),
            expected_size: 123_456,
            expected_version: "1.0.7",
        };
        let script = build_windows_update_script(&plan);

        assert!(script.contains("Wait-Process -Id 4242"));
        assert!(script.contains("$installedSize -ne 123456"));
        assert!(script.contains("$expectedVersion = '1.0.7'"));
        assert!(script.contains("$stagedVersion -ne $expectedVersion"));
        assert!(script.contains("Move-Item -LiteralPath $backup -Destination $current -Force"));
        assert!(script.contains("update failed:"));
        assert!(script.contains("Start-Process -FilePath $current"));

        assert_powershell_script_parses(&script, "portable-update-script-test");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_setup_update_script_runs_installer_and_checks_version() {
        let plan = WindowsSetupUpdateScriptPlan {
            process_id: 4242,
            current_exe: std::path::Path::new(r"C:\TinyShell\tiny-shell.exe"),
            installer: std::path::Path::new(r"C:\Temp\tiny-shell-v1.0.7-setup.exe"),
            script_path: std::path::Path::new(r"C:\Temp\tiny-shell-setup-update-4242.ps1"),
            log_path: std::path::Path::new(r"C:\Temp\tiny-shell-update.log"),
            expected_version: "1.0.7",
        };
        let script = build_windows_setup_update_script(&plan);

        assert!(script.contains("Wait-Process -Id 4242"));
        assert!(script.contains("'/VERYSILENT'"));
        assert!(script.contains("'/CURRENTUSER'"));
        assert!(script.contains("$setup.ExitCode -ne 0"));
        assert!(script.contains("$installedVersion -ne $expectedVersion"));
        assert!(script.contains("Start-Process -FilePath $current"));
        assert_powershell_script_parses(&script, "setup-update-script-test");
    }
}
pub(crate) mod ui;
