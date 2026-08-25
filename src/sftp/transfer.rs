use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use rust_i18n::t;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::terminal::{BackendEvent, BackendEventSender, TransferState};

use super::{
    SftpClientHandler, SftpControl, SftpControlQueue,
    archive::{create_zip_from_directory, extract_archive_to},
    filesystem::create_remote_dir_all,
    remote_command::{create_remote_archive, create_remote_paths_archive, remove_remote_path},
};
use super::{base_name, join_remote, remote_parent};

pub(super) fn local_partial_path(final_path: &Path, transfer_id: &str) -> PathBuf {
    let file_name = final_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    final_path.with_file_name(format!(".{file_name}.tiny-shell-{transfer_id}.part"))
}

pub(super) fn remote_partial_path(final_path: &str, transfer_id: &str) -> String {
    let parent = remote_parent(final_path);
    let name = base_name(final_path);
    join_remote(&parent, &format!(".{name}.tiny-shell-{transfer_id}.part"))
}

/// Cooperative transfer control shared by upload/download tasks and their UI.
pub(crate) struct TransferStateFlag(pub(crate) Arc<AtomicU8>);

/// Shared context passed through SFTP transfer operations.
pub(crate) struct TransferContext<'a> {
    pub(crate) flag: &'a TransferStateFlag,
    pub(crate) events: &'a BackendEventSender,
    pub(crate) tab_id: &'a str,
    pub(crate) id: &'a str,
}

impl TransferContext<'_> {
    pub(crate) async fn yield_if_paused(&self, transferred: u64, total: Option<u64>) -> Result<()> {
        self.flag
            .yield_if_paused(self.events, self.tab_id, self.id, transferred, total)
            .await
    }

    pub(crate) fn report_progress(
        &self,
        transferred: u64,
        total: Option<u64>,
        state: TransferState,
    ) {
        let _ = self.events.send(BackendEvent::TransferProgress {
            tab_id: self.tab_id.to_string(),
            id: self.id.to_string(),
            transferred,
            total,
            state,
        });
    }
}

impl TransferStateFlag {
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicU8::new(0)))
    }

    pub(crate) fn pause(&self) {
        self.0.store(1, Ordering::SeqCst);
    }

    pub(crate) fn resume(&self) {
        self.0.store(0, Ordering::SeqCst);
    }

    pub(crate) fn cancel(&self) {
        self.0.store(2, Ordering::SeqCst);
    }

    pub(crate) async fn yield_if_paused(
        &self,
        events: &BackendEventSender,
        tab_id: &str,
        id: &str,
        transferred: u64,
        total: Option<u64>,
    ) -> Result<()> {
        let mut was_paused = false;
        loop {
            match self.0.load(Ordering::SeqCst) {
                2 => return Err(anyhow::anyhow!("transfer cancelled")),
                1 => {
                    if !was_paused {
                        let _ = events.send(BackendEvent::TransferProgress {
                            tab_id: tab_id.to_string(),
                            id: id.to_string(),
                            transferred,
                            total,
                            state: TransferState::Paused,
                        });
                        was_paused = true;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                _ => {
                    if was_paused {
                        let _ = events.send(BackendEvent::TransferProgress {
                            tab_id: tab_id.to_string(),
                            id: id.to_string(),
                            transferred,
                            total,
                            state: TransferState::Running,
                        });
                    }
                    return Ok(());
                }
            }
        }
    }
}

/// Ensures `TransferFinished` is sent even when a transfer task exits during
/// setup, keeping the runtime's active task registry in sync with the UI.
pub(super) struct TransferCleanup {
    controls: Option<Arc<SftpControlQueue>>,
    id: Option<String>,
}

impl TransferCleanup {
    pub(super) fn new(controls: Arc<SftpControlQueue>, id: String) -> Self {
        Self {
            controls: Some(controls),
            id: Some(id),
        }
    }
}

impl Drop for TransferCleanup {
    fn drop(&mut self) {
        if let (Some(controls), Some(id)) = (self.controls.take(), self.id.take()) {
            controls.send(SftpControl::TransferFinished(id));
        }
    }
}

pub(super) fn report_transfer_failure(
    events: &BackendEventSender,
    tab_id: &str,
    id: &str,
    error: impl Into<String>,
) {
    report_transfer_state(events, tab_id, id, TransferState::Failed(error.into()));
}

pub(super) fn report_transfer_interrupted(
    events: &BackendEventSender,
    tab_id: &str,
    id: &str,
    reason: impl Into<String>,
) {
    report_transfer_state(
        events,
        tab_id,
        id,
        TransferState::Interrupted(reason.into()),
    );
}

fn report_transfer_state(
    events: &BackendEventSender,
    tab_id: &str,
    id: &str,
    state: TransferState,
) {
    let _ = events.send(BackendEvent::TransferProgress {
        tab_id: tab_id.to_string(),
        id: id.to_string(),
        transferred: 0,
        total: None,
        state,
    });
}

pub(super) async fn download_path_impl(
    handle: &russh::client::Handle<SftpClientHandler>,
    sftp: &SftpSession,
    remote: &str,
    local_dir: &Path,
    ctx: &TransferContext<'_>,
    expected_size: Option<u64>,
    expected_modified: Option<u64>,
) -> Result<String> {
    tokio::fs::create_dir_all(local_dir)
        .await
        .with_context(|| format!("create {}", local_dir.display()))?;

    // Check for cancellation after initial setup
    let state = ctx.flag.0.load(Ordering::SeqCst);
    if state == 2 {
        return Err(anyhow::anyhow!("transfer cancelled"));
    }

    let metadata = sftp
        .metadata(remote)
        .await
        .with_context(|| format!("metadata {remote}"))?;
    let is_dir = metadata
        .permissions
        .map(|mode| (mode & 0o170_000) == 0o040_000)
        .unwrap_or(false);

    if is_dir {
        let local_archive = local_dir.join(format!(
            ".tiny-shell-{}-{}.tar.gz",
            base_name(remote),
            Uuid::new_v4()
        ));
        let extracted_to =
            download_remote_directory_archive(handle, sftp, remote, &local_archive, ctx).await?;
        return Ok(t!("downloaded_folder", path = extracted_to.display()).to_string());
    }

    let local_path = local_dir.join(base_name(remote));
    download_file_impl(
        sftp,
        remote,
        &local_path,
        ctx,
        expected_size,
        expected_modified,
    )
    .await?;
    Ok(t!("downloaded_file", path = local_path.display()).to_string())
}

async fn download_remote_directory_archive(
    handle: &russh::client::Handle<SftpClientHandler>,
    sftp: &SftpSession,
    remote_dir: &str,
    local_archive: &Path,
    ctx: &TransferContext<'_>,
) -> Result<PathBuf> {
    let remote_archive = format!(
        "/tmp/tiny-shell-{}-{}.tar.gz",
        base_name(remote_dir),
        Uuid::new_v4()
    );

    // Check for cancellation before creating remote archive
    let state = ctx.flag.0.load(Ordering::SeqCst);
    if state == 2 {
        return Err(anyhow::anyhow!("transfer cancelled"));
    }

    create_remote_archive(handle, remote_dir, &remote_archive).await?;

    let local_extract_root = local_archive
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(base_name(remote_dir));

    let archive_download = async {
        download_file_impl(sftp, &remote_archive, local_archive, ctx, None, None).await?;
        extract_archive_to(
            local_archive,
            local_archive.parent().unwrap_or_else(|| Path::new(".")),
        )
        .await?;
        tokio::fs::remove_file(local_archive)
            .await
            .with_context(|| format!("remove {}", local_archive.display()))?;
        Ok::<PathBuf, anyhow::Error>(local_extract_root)
    }
    .await;

    let cleanup_result = remove_remote_path(handle, &remote_archive).await;

    let extracted_to = archive_download?;
    if let Err(err) = cleanup_result {
        tracing::warn!("failed to clean remote archive {remote_archive}: {err:#}");
    }

    Ok(extracted_to)
}

#[derive(Clone, Copy)]
pub(super) struct SourceFingerprint {
    size: Option<u64>,
    modified: Option<u64>,
}

pub(super) async fn download_file_impl(
    sftp: &SftpSession,
    remote: &str,
    local: &Path,
    ctx: &TransferContext<'_>,
    expected_size: Option<u64>,
    expected_modified: Option<u64>,
) -> Result<()> {
    let source_metadata = sftp
        .metadata(remote)
        .await
        .with_context(|| format!("stat remote {remote}"))?;
    let total = source_metadata
        .size
        .ok_or_else(|| anyhow!("remote file {remote} has no size"))?;
    if expected_size.is_some_and(|size| size != total)
        || expected_modified
            .is_some_and(|mtime| source_metadata.mtime.map(u64::from) != Some(mtime))
    {
        return Err(anyhow!("remote source changed; cannot resume {remote}"));
    }
    let partial = local_partial_path(local, ctx.id);
    let offset = match tokio::fs::metadata(&partial).await {
        Ok(metadata) if metadata.len() > total => {
            return Err(anyhow!("partial download is larger than remote file"));
        }
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error).with_context(|| format!("stat {}", partial.display())),
    };

    if offset == total {
        tokio::fs::rename(&partial, local)
            .await
            .with_context(|| format!("finalize {}", local.display()))?;
        ctx.report_progress(
            total,
            Some(total),
            crate::terminal::TransferState::Completed,
        );
        return Ok(());
    }

    let mut remote_file = sftp
        .open(remote)
        .await
        .with_context(|| format!("open remote {remote}"))?;
    remote_file
        .seek(SeekFrom::Start(offset))
        .await
        .with_context(|| format!("seek remote {remote} to {offset}"))?;
    let mut local_file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&partial)
        .await
        .with_context(|| format!("open partial {}", partial.display()))?;
    local_file
        .seek(SeekFrom::Start(offset))
        .await
        .with_context(|| format!("seek partial {} to {offset}", partial.display()))?;

    let mut transferred = offset;
    let mut buffer = vec![0u8; 128 * 1024];
    let mut last_progress_time = Instant::now();
    let mut last_reported = transferred;
    ctx.report_progress(
        transferred,
        Some(total),
        crate::terminal::TransferState::Running,
    );
    loop {
        ctx.yield_if_paused(transferred, Some(total)).await?;
        let read = remote_file
            .read(&mut buffer)
            .await
            .context("read remote file")?;
        if read == 0 {
            break;
        }
        local_file
            .write_all(&buffer[..read])
            .await
            .with_context(|| format!("write {}", partial.display()))?;
        transferred += read as u64;
        if last_progress_time.elapsed() >= Duration::from_millis(100)
            || transferred - last_reported >= 1024 * 1024
        {
            ctx.report_progress(
                transferred,
                Some(total),
                crate::terminal::TransferState::Running,
            );
            last_progress_time = Instant::now();
            last_reported = transferred;
        }
    }
    local_file.flush().await.context("flush partial download")?;
    if transferred != total || tokio::fs::metadata(&partial).await?.len() != total {
        return Err(anyhow!("download size verification failed for {remote}"));
    }
    tokio::fs::rename(&partial, local)
        .await
        .with_context(|| format!("finalize {}", local.display()))?;
    ctx.report_progress(
        transferred,
        Some(total),
        crate::terminal::TransferState::Completed,
    );
    Ok(())
}

pub(super) async fn upload_paths_impl(
    sftp: &SftpSession,
    locals: &[String],
    remote_dir: &str,
    ctx: &TransferContext<'_>,
    expected_size: Option<u64>,
    expected_modified: Option<u64>,
) -> Result<String> {
    // Check for cancellation before starting
    let state = ctx.flag.0.load(Ordering::SeqCst);
    if state == 2 {
        return Err(anyhow::anyhow!("transfer cancelled"));
    }

    create_remote_dir_all(sftp, remote_dir).await?;
    let mut file_count = 0usize;
    let mut folder_count = 0usize;

    let mut total_bytes = 0u64;
    let mut files_to_upload = Vec::new();
    let mut dirs_to_create = Vec::new();

    for local in locals {
        let p = PathBuf::from(local);
        if p.is_dir() {
            folder_count += 1;
            let root_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("folder");
            let remote_root = join_remote(remote_dir, root_name);
            dirs_to_create.push(remote_root.clone());

            for entry in WalkDir::new(&p) {
                let entry = entry?;
                let path = entry.path();
                if path == p {
                    continue;
                }

                let meta = tokio::fs::metadata(&path).await?;
                let relative = path.strip_prefix(&p)?;
                let remote_path = if relative.as_os_str().is_empty() {
                    remote_root.clone()
                } else {
                    let rel = relative
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join("/");
                    join_remote(&remote_root, &rel)
                };

                if path.is_dir() {
                    dirs_to_create.push(remote_path);
                } else {
                    total_bytes += meta.len();
                    files_to_upload.push((path.to_path_buf(), remote_path));
                }
            }
        } else {
            let meta = tokio::fs::metadata(&p).await?;
            total_bytes += meta.len();
            let file_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("file");
            files_to_upload.push((p.clone(), join_remote(remote_dir, file_name)));
            file_count += 1;
        }
    }

    // Check for cancellation before creating directories
    let state = ctx.flag.0.load(Ordering::SeqCst);
    if state == 2 {
        return Err(anyhow::anyhow!("transfer cancelled"));
    }

    // Create directories sequentially first
    for dir in dirs_to_create {
        // Check for cancellation between each directory creation
        let state = ctx.flag.0.load(Ordering::SeqCst);
        if state == 2 {
            return Err(anyhow::anyhow!("transfer cancelled"));
        }
        create_remote_dir_all(sftp, &dir).await?;
    }

    let transferred = Arc::new(AtomicU64::new(0));
    let mut futures = Vec::new();

    for (local_path, remote_path) in files_to_upload {
        let transferred_clone = Arc::clone(&transferred);

        futures.push(async move {
            let flag = TransferStateFlag(Arc::clone(&ctx.flag.0));
            let ctx_clone = TransferContext {
                flag: &flag,
                events: ctx.events,
                tab_id: ctx.tab_id,
                id: ctx.id,
            };
            upload_file_impl(
                sftp,
                &local_path,
                &remote_path,
                &ctx_clone,
                transferred_clone,
                Some(total_bytes),
                Some(SourceFingerprint {
                    size: (locals.len() == 1).then_some(expected_size).flatten(),
                    modified: (locals.len() == 1).then_some(expected_modified).flatten(),
                }),
            )
            .await
        });
    }

    use futures::StreamExt as _;
    let mut stream = futures::stream::iter(futures).buffer_unordered(4);
    while let Some(res) = stream.next().await {
        res?;
    }

    ctx.report_progress(
        total_bytes,
        Some(total_bytes),
        crate::terminal::TransferState::Completed,
    );

    let summary = if file_count == 1 && folder_count == 0 {
        t!("uploaded_file").to_string()
    } else if file_count == 0 && folder_count == 1 {
        t!("uploaded_folder").to_string()
    } else if file_count > 0 && folder_count == 0 {
        t!("uploaded_n_files", files = file_count).to_string()
    } else if file_count == 0 && folder_count > 0 {
        t!("uploaded_n_folders", folders = folder_count).to_string()
    } else {
        t!(
            "uploaded_files_and_folders",
            files = file_count,
            folders = folder_count
        )
        .to_string()
    };
    Ok(summary)
}

pub(super) async fn upload_file_impl(
    sftp: &SftpSession,
    local_file: &Path,
    remote_path: &str,
    ctx: &TransferContext<'_>,
    transferred: Arc<AtomicU64>,
    total: Option<u64>,
    expected: Option<SourceFingerprint>,
) -> Result<()> {
    let source_metadata = tokio::fs::metadata(local_file)
        .await
        .with_context(|| format!("stat local {}", local_file.display()))?;
    let local_size = source_metadata.len();
    let local_modified = source_metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    if expected.is_some_and(|expected| {
        expected.size.is_some_and(|size| size != local_size)
            || expected
                .modified
                .is_some_and(|mtime| local_modified != Some(mtime))
    }) {
        return Err(anyhow!(
            "local source changed; cannot resume {}",
            local_file.display()
        ));
    }
    let partial_path = remote_partial_path(remote_path, ctx.id);
    let offset = match sftp.metadata(&partial_path).await {
        Ok(metadata) => {
            let size = metadata.size.unwrap_or(0);
            if size > local_size {
                return Err(anyhow!("partial upload is larger than local file"));
            }
            size
        }
        Err(_) => 0,
    };

    let mut local = tokio::fs::File::open(local_file)
        .await
        .with_context(|| format!("open local {}", local_file.display()))?;
    local
        .seek(SeekFrom::Start(offset))
        .await
        .with_context(|| format!("seek local {} to {offset}", local_file.display()))?;
    let mut remote = sftp
        .open_with_flags(&partial_path, OpenFlags::CREATE | OpenFlags::WRITE)
        .await
        .with_context(|| format!("open partial remote {partial_path}"))?;
    remote
        .seek(SeekFrom::Start(offset))
        .await
        .with_context(|| format!("seek partial remote {partial_path} to {offset}"))?;

    let mut buffer = vec![0u8; 128 * 1024];
    let mut last_progress_time = Instant::now();
    let mut last_reported = offset;
    transferred.fetch_add(offset, Ordering::Relaxed);
    ctx.report_progress(offset, total, crate::terminal::TransferState::Running);
    loop {
        let cur = transferred.load(Ordering::Relaxed);
        ctx.yield_if_paused(cur, total).await?;
        let read = local.read(&mut buffer).await.context("read local file")?;
        if read == 0 {
            break;
        }
        remote
            .write_all(&buffer[..read])
            .await
            .with_context(|| format!("write remote {partial_path}"))?;
        let new_cur = transferred.fetch_add(read as u64, Ordering::Relaxed) + read as u64;
        if last_progress_time.elapsed() >= Duration::from_millis(100)
            || new_cur - last_reported >= 1024 * 1024
        {
            ctx.report_progress(new_cur, total, crate::terminal::TransferState::Running);
            last_progress_time = Instant::now();
            last_reported = new_cur;
        }
    }
    remote.flush().await.context("flush partial remote file")?;
    remote
        .shutdown()
        .await
        .context("close partial remote file")?;
    if sftp
        .metadata(&partial_path)
        .await
        .ok()
        .and_then(|metadata| metadata.size)
        != Some(local_size)
    {
        return Err(anyhow!("upload size verification failed for {remote_path}"));
    }
    sftp.rename(&partial_path, remote_path)
        .await
        .with_context(|| format!("finalize remote {remote_path}"))?;
    Ok(())
}

pub(super) async fn pack_remote_paths_to_zip(
    handle: &russh::client::Handle<SftpClientHandler>,
    remote_paths: &[String],
    local_zip: &Path,
    tmp_dir: &Path,
    ctx: &TransferContext<'_>,
) -> Result<()> {
    let work_id = Uuid::new_v4();
    let remote_archive = format!("/tmp/tiny-shell-pack-{work_id}.tar.gz");
    let local_archive = tmp_dir.join(format!("tiny-shell-pack-{work_id}.tar.gz"));
    let extract_dir = tmp_dir.join(format!("tiny-shell-pack-{work_id}"));
    tokio::fs::create_dir_all(tmp_dir)
        .await
        .with_context(|| format!("create {}", tmp_dir.display()))?;

    create_remote_paths_archive(handle, remote_paths, &remote_archive).await?;
    let operation = async {
        let channel = handle
            .channel_open_session()
            .await
            .context("open SFTP channel for packed download")?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .context("request SFTP subsystem for packed download")?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .context("create SFTP session for packed download")?;
        download_file_impl(&sftp, &remote_archive, &local_archive, ctx, None, None).await?;
        tokio::fs::create_dir_all(&extract_dir)
            .await
            .with_context(|| format!("create {}", extract_dir.display()))?;
        extract_archive_to(&local_archive, &extract_dir).await?;

        if let Some(parent) = local_zip.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let zip_source = extract_dir.clone();
        let zip_target = local_zip.to_path_buf();
        tokio::task::spawn_blocking(move || create_zip_from_directory(&zip_source, &zip_target))
            .await
            .context("join ZIP creation task")??;
        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(err) = remove_remote_path(handle, &remote_archive).await {
        tracing::warn!("failed to clean remote packed archive: {err:#}");
    }
    let _ = tokio::fs::remove_file(&local_archive).await;
    let _ = tokio::fs::remove_dir_all(&extract_dir).await;
    operation
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{TransferStateFlag, local_partial_path, remote_partial_path};
    use crate::terminal::backend_event_channel;

    #[test]
    fn cancellation_is_shared_between_clones() {
        let original = TransferStateFlag::new();
        let clone = TransferStateFlag(std::sync::Arc::clone(&original.0));
        original.cancel();
        assert_eq!(clone.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cancelled_transfer_stops_at_next_cooperative_yield() {
        let flag = TransferStateFlag::new();
        let (events, _receiver) = backend_event_channel();
        flag.cancel();

        let result = flag
            .yield_if_paused(&events, "tab", "transfer", 42, Some(100))
            .await;

        assert!(result.is_err_and(|error| error.to_string().contains("cancelled")));
    }

    #[test]
    fn partial_paths_are_stable_and_keep_the_parent_directory() {
        let local = Path::new("C:/downloads/report.bin");
        assert_eq!(
            local_partial_path(local, "transfer-1"),
            PathBuf::from("C:/downloads/.report.bin.tiny-shell-transfer-1.part")
        );
        assert_eq!(
            remote_partial_path("/var/tmp/report.bin", "transfer-1"),
            "/var/tmp/.report.bin.tiny-shell-transfer-1.part"
        );
    }
}
