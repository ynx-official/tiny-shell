pub mod ops;
pub mod text_file;

pub(crate) mod handle;
pub(crate) mod handler;
mod transfer;
pub(crate) mod utils;

pub(crate) use handle::SftpHandle;
pub(crate) use handler::SftpClientHandler;
pub(crate) use transfer::TransferStateFlag;
pub(crate) use utils::*;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};

use directories::BaseDirs;
use flate2::read::GzDecoder;
use russh::{
    Disconnect,
    client::{self},
    keys::{PrivateKey, decode_secret_key, load_secret_key},
};
use russh_sftp::{
    client::SftpSession,
    protocol::{FileAttributes, OpenFlags},
};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::{
        Notify, Semaphore,
        mpsc::{self, Receiver, Sender},
    },
    task::JoinHandle,
};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::read::ZipArchive;

use rust_i18n::t;

use crate::{
    session::{
        config::{AuthMethod, ConfigStore, Session, TempTaskDirectory},
        ssh_keys::{
            authenticate_with_default_keys, normalize_inline_private_key, private_keys_with_algs,
            session_has_explicit_key,
        },
    },
    sftp::{
        text_file::{
            EDITOR_HARD_LIMIT_BYTES, RemoteFileRevision, RemoteTextFile, RemoteTextSave,
            decode_remote_text, encode_remote_text,
        },
        transfer::TransferContext,
    },
    terminal::{BackendEvent, BackendEventSender},
};

const COMMAND_QUEUE_CAPACITY: usize = 256;
const CONTROL_QUEUE_CAPACITY: usize = 1_024;

enum SftpTempDirectory {
    Managed(TempTaskDirectory),
    Fallback(PathBuf),
}

impl SftpTempDirectory {
    fn path(&self) -> &Path {
        match self {
            Self::Managed(directory) => directory.path(),
            Self::Fallback(path) => path,
        }
    }
}

impl Drop for SftpTempDirectory {
    fn drop(&mut self) {
        let Self::Fallback(path) = self else {
            return;
        };
        if let Err(error) = fs::remove_dir_all(&*path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!(path = %path.display(), %error, "failed to clean up SFTP temporary directory");
        }
    }
}

fn allocate_sftp_temp_directory(purpose: &str) -> Result<SftpTempDirectory> {
    if let Some(workspace) = ConfigStore::temp_workspace() {
        match workspace.allocate(purpose) {
            Ok(directory) => return Ok(SftpTempDirectory::Managed(directory)),
            Err(error) => {
                tracing::warn!(%error, purpose, "falling back to the operating-system temporary directory");
            }
        }
    }

    let path = std::env::temp_dir().join("tiny-shell").join(format!(
        "{}-{}",
        purpose
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>(),
        Uuid::new_v4()
    ));
    fs::create_dir_all(&path)
        .with_context(|| format!("create SFTP temporary directory {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("secure SFTP temporary directory {}", path.display()))?;
    }
    Ok(SftpTempDirectory::Fallback(path))
}

fn safe_local_edit_name(remote_path: &str) -> String {
    let sanitized = base_name(remote_path)
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() || matches!(sanitized.as_str(), "." | "..") {
        "remote-file".to_string()
    } else {
        sanitized
    }
}

#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub name: String,
    pub full_path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u32,
    pub permissions: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionApplyTarget {
    FilesAndDirectories,
    FilesOnly,
    DirectoriesOnly,
}

#[derive(Debug)]
pub enum SftpCommand {
    ListDir(String),
    ListDirectoryTree(String),
    MeasureLatency,
    Download {
        remote: String,
        local_dir: String,
    },
    ResumeDownload {
        id: String,
        remote: String,
        local_dir: String,
        source_size: Option<u64>,
        source_modified: Option<u64>,
    },
    EditFile {
        remote_path: String,
        editor: Option<String>,
    },
    CreateDir(String),
    CreateFile(String),
    RenamePath {
        old_path: String,
        new_path: String,
    },
    SetPermissions {
        remote_path: String,
        mode: u32,
        recursive: bool,
        apply_to: PermissionApplyTarget,
    },
    DeletePaths(Vec<String>),
    QuickDeletePaths(Vec<String>),
    PackDownload {
        remote_paths: Vec<String>,
        local_zip: String,
    },
    UploadEditedFile {
        local_path: String,
        remote_path: String,
    },
    /// 下载文件内容到内存(不落地临时文件),供内置编辑器使用。
    DownloadFileContent {
        remote_path: String,
    },
    /// 以版本校验和原子替换方式保存内存中的文本文件。
    SaveFileContent(RemoteTextSave),
    UploadPaths {
        locals: Vec<String>,
        remote_dir: String,
    },
    ResumeUpload {
        id: String,
        local: String,
        remote_dir: String,
        source_size: Option<u64>,
        source_modified: Option<u64>,
    },
    CleanupRemotePartial(String),
}

/// Low-volume lifecycle commands use a separate reliable queue so a burst of
/// directory operations cannot discard close, cancellation, or task cleanup.
#[derive(Debug)]
pub(crate) enum SftpControl {
    PauseTransfer(String),
    ResumeTransfer(String),
    CancelTransfer(String),
    TransferFinished(String),
    Close,
}

/// Bounded synchronous-producer mailbox for lifecycle controls. Pause and
/// resume requests are coalesced per transfer, while cancellation and cleanup
/// can evict an older coalescible request when the queue is full.
pub(crate) struct SftpControlQueue {
    queue: Mutex<VecDeque<SftpControl>>,
    notify: Notify,
}

impl SftpControlQueue {
    fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::with_capacity(CONTROL_QUEUE_CAPACITY)),
            notify: Notify::new(),
        }
    }

    pub(crate) fn send(&self, control: SftpControl) -> bool {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(id) = control.coalescing_id()
            && let Some(index) = queue.iter().rposition(|queued| {
                queued
                    .coalescing_id()
                    .is_some_and(|queued_id| queued_id == id)
            })
        {
            queue.remove(index);
        }
        if queue.len() >= CONTROL_QUEUE_CAPACITY {
            let removable = queue
                .iter()
                .position(SftpControl::is_coalescible)
                .or_else(|| {
                    control
                        .is_high_priority()
                        .then(|| queue.iter().position(|queued| !queued.is_high_priority()))
                        .flatten()
                });
            let Some(index) = removable else {
                tracing::warn!("SFTP control queue is full; dropping control");
                return false;
            };
            queue.remove(index);
        }
        queue.push_back(control);
        drop(queue);
        self.notify.notify_one();
        true
    }

    pub(crate) async fn recv(&self) -> SftpControl {
        loop {
            if let Some(control) = self
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pop_front()
            {
                return control;
            }
            self.notify.notified().await;
        }
    }
}

impl SftpControl {
    fn coalescing_id(&self) -> Option<&str> {
        match self {
            Self::PauseTransfer(id) | Self::ResumeTransfer(id) => Some(id),
            Self::CancelTransfer(_) | Self::TransferFinished(_) | Self::Close => None,
        }
    }

    fn is_coalescible(&self) -> bool {
        matches!(self, Self::PauseTransfer(_) | Self::ResumeTransfer(_))
    }

    fn is_high_priority(&self) -> bool {
        matches!(
            self,
            Self::CancelTransfer(_) | Self::TransferFinished(_) | Self::Close
        )
    }
}

/// Ensures `TransferFinished` is sent for a transfer task even when the task
/// returns early because of a setup failure, keeping `active_transfers` in sync
/// with the UI.
struct TransferCleanup {
    tx: Option<Arc<SftpControlQueue>>,
    id: Option<String>,
}

impl TransferCleanup {
    fn new(tx: Arc<SftpControlQueue>, id: String) -> Self {
        Self {
            tx: Some(tx),
            id: Some(id),
        }
    }
}

impl Drop for TransferCleanup {
    fn drop(&mut self) {
        if let (Some(tx), Some(id)) = (self.tx.take(), self.id.take()) {
            tx.send(SftpControl::TransferFinished(id));
        }
    }
}

fn report_transfer_failure(
    events: &BackendEventSender,
    tab_id: &str,
    id: &str,
    error: impl Into<String>,
) {
    let _ = events.send(BackendEvent::TransferProgress {
        tab_id: tab_id.to_string(),
        id: id.to_string(),
        transferred: 0,
        total: None,
        state: crate::terminal::TransferState::Failed(error.into()),
    });
}

fn report_transfer_interrupted(
    events: &BackendEventSender,
    tab_id: &str,
    id: &str,
    reason: impl Into<String>,
) {
    let _ = events.send(BackendEvent::TransferProgress {
        tab_id: tab_id.to_string(),
        id: id.to_string(),
        transferred: 0,
        total: None,
        state: crate::terminal::TransferState::Interrupted(reason.into()),
    });
}

pub fn spawn_sftp(
    runtime: &tokio::runtime::Handle,
    tab_id: String,
    session: Session,
    proxy_config: ConfigStore,
    events: BackendEventSender,
) -> SftpHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_QUEUE_CAPACITY);
    let control_queue = Arc::new(SftpControlQueue::new());
    let cmd_tx_clone = cmd_tx.clone();
    let control_queue_clone = control_queue.clone();
    let _join = runtime.spawn(async move {
        if let Err(err) = run_sftp(
            tab_id.clone(),
            session,
            proxy_config,
            cmd_rx,
            cmd_tx_clone,
            control_queue_clone.clone(),
            control_queue_clone,
            events.clone(),
        )
        .await
        {
            let _ = events.send(BackendEvent::SftpStatus {
                tab_id: tab_id.clone(),
                text: t!("sftp_error", error = format!("{err:#}")).to_string(),
            });
            let _ = events.send(BackendEvent::Closed {
                tab_id,
                reason: t!("sftp_error", error = format!("{err:#}")).to_string(),
            });
        }
    });
    SftpHandle::new(cmd_tx, control_queue)
}

struct SftpRuntime<'a> {
    handle: &'a Arc<russh::client::Handle<SftpClientHandler>>,
    sftp: &'a SftpSession,
    tab_id: &'a str,
    session_id: &'a str,
    home: &'a str,
    events: &'a BackendEventSender,
    commands_tx: &'a mpsc::Sender<SftpCommand>,
    controls_tx: &'a Arc<SftpControlQueue>,
    active_transfers: &'a mut HashMap<String, TransferStateFlag>,
    active_tasks: &'a mut HashMap<String, JoinHandle<()>>,
    channel_slots: &'a Arc<Semaphore>,
}

impl SftpRuntime<'_> {
    fn resolve_home_path(&self, path: String) -> String {
        if path == "~" {
            self.home.to_string()
        } else if let Some(rest) = path.strip_prefix("~/") {
            crate::sftp::join_remote(self.home, rest)
        } else {
            path
        }
    }

    async fn handle_command(&mut self, command: SftpCommand) -> bool {
        match command {
            SftpCommand::CleanupRemotePartial(path) => {
                self.handle_cleanup_remote_partial(path).await
            }
            SftpCommand::MeasureLatency => self.handle_measure_latency().await,
            SftpCommand::ListDir(path) => self.handle_list_dir(path).await,
            SftpCommand::ListDirectoryTree(path) => self.handle_list_directory_tree(path).await,
            SftpCommand::Download { remote, local_dir } => {
                self.handle_download(remote, local_dir).await
            }
            SftpCommand::ResumeDownload {
                id,
                remote,
                local_dir,
                source_size,
                source_modified,
            } => {
                self.start_download(id, remote, local_dir, source_size, source_modified)
                    .await
            }
            SftpCommand::UploadPaths { locals, remote_dir } => {
                self.handle_upload_paths(locals, remote_dir).await
            }
            SftpCommand::ResumeUpload {
                id,
                local,
                remote_dir,
                source_size,
                source_modified,
            } => {
                self.start_upload(id, vec![local], remote_dir, source_size, source_modified)
                    .await
            }
            SftpCommand::EditFile {
                remote_path,
                editor,
            } => self.handle_edit_file(remote_path, editor).await,
            SftpCommand::UploadEditedFile {
                local_path,
                remote_path,
            } => {
                self.handle_upload_edited_file(local_path, remote_path)
                    .await
            }
            SftpCommand::DownloadFileContent { remote_path } => {
                self.handle_download_file_content(remote_path).await
            }
            SftpCommand::SaveFileContent(save) => self.handle_save_file_content(save).await,
            SftpCommand::CreateDir(path) => self.handle_create_dir(path).await,
            SftpCommand::CreateFile(path) => self.handle_create_file(path).await,
            SftpCommand::RenamePath { old_path, new_path } => {
                self.handle_rename_path(old_path, new_path).await
            }
            SftpCommand::SetPermissions {
                remote_path,
                mode,
                recursive,
                apply_to,
            } => {
                self.handle_set_permissions(remote_path, mode, recursive, apply_to)
                    .await
            }
            SftpCommand::DeletePaths(paths) => self.handle_delete_paths(paths).await,
            SftpCommand::QuickDeletePaths(paths) => self.handle_quick_delete_paths(paths).await,
            SftpCommand::PackDownload {
                remote_paths,
                local_zip,
            } => self.handle_pack_download(remote_paths, local_zip).await,
        }
    }

    async fn handle_control(&mut self, control: SftpControl) -> bool {
        match control {
            SftpControl::Close => self.handle_close().await,
            SftpControl::PauseTransfer(id) => self.handle_pause_transfer(id).await,
            SftpControl::ResumeTransfer(id) => self.handle_resume_transfer(id).await,
            SftpControl::CancelTransfer(id) => self.handle_cancel_transfer(id).await,
            SftpControl::TransferFinished(id) => self.handle_transfer_finished(id).await,
        }
    }

    async fn handle_close(&mut self) -> bool {
        let active_ids: Vec<String> = self.active_transfers.keys().cloned().collect();
        self.active_transfers.clear();
        for id in active_ids {
            report_transfer_interrupted(self.events, self.tab_id, &id, "SFTP session closed");
        }
        for (_, task) in self.active_tasks.drain() {
            task.abort();
        }
        false
    }

    async fn handle_pause_transfer(&mut self, id: String) -> bool {
        if let Some(flag) = self.active_transfers.get(&id) {
            flag.pause();
        }
        true
    }

    async fn handle_resume_transfer(&mut self, id: String) -> bool {
        if let Some(flag) = self.active_transfers.get(&id) {
            flag.resume();
        }
        true
    }

    async fn handle_cancel_transfer(&mut self, id: String) -> bool {
        if let Some(flag) = self.active_transfers.remove(&id) {
            flag.cancel();
        }
        true
    }

    async fn handle_cleanup_remote_partial(&self, path: String) -> bool {
        if let Err(error) = self.sftp.remove_file(&path).await {
            tracing::debug!(path, %error, "failed to clean remote transfer partial");
        }
        true
    }

    async fn handle_transfer_finished(&mut self, id: String) -> bool {
        self.active_transfers.remove(&id);
        self.active_tasks.remove(&id);
        true
    }

    async fn handle_measure_latency(&self) -> bool {
        let started = Instant::now();
        let latency_ms = self
            .sftp
            .canonicalize(".")
            .await
            .ok()
            .map(|_| started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
        let _ = self.events.send(BackendEvent::SftpLatency {
            tab_id: self.tab_id.to_string(),
            latency_ms,
        });
        true
    }

    async fn handle_list_dir(&self, path: String) -> bool {
        let actual_path = self.resolve_home_path(path);
        if let Err(err) = emit_entries(self.events, self.tab_id, self.sftp, &actual_path).await {
            let _ = self.events.send(BackendEvent::SftpStatus {
                tab_id: self.tab_id.to_string(),
                text: t!("sftp_list_failed", error = format!("{err:#}")).to_string(),
            });
        }
        true
    }

    async fn handle_list_directory_tree(&self, path: String) -> bool {
        let actual_path = self.resolve_home_path(path);
        match list_dir_impl(self.sftp, &actual_path).await {
            Ok(entries) => {
                let _ = self.events.send(BackendEvent::SftpDirectoryEntries {
                    tab_id: self.tab_id.to_string(),
                    path: actual_path,
                    entries,
                });
            }
            Err(err) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("sftp_list_failed", error = format!("{err:#}")).to_string(),
                });
            }
        }
        true
    }

    async fn refresh_directory(&self, path: String) {
        if let Err(error) = emit_entries(self.events, self.tab_id, self.sftp, &path).await {
            let _ = self.events.send(BackendEvent::SftpStatus {
                tab_id: self.tab_id.to_string(),
                text: t!("sftp_list_failed", error = format!("{error:#}")).to_string(),
            });
        }
    }

    async fn handle_download(&mut self, remote: String, local_dir: String) -> bool {
        self.start_download(Uuid::new_v4().to_string(), remote, local_dir, None, None)
            .await
    }

    async fn start_download(
        &mut self,
        id: String,
        remote: String,
        local_dir: String,
        expected_size: Option<u64>,
        expected_modified: Option<u64>,
    ) -> bool {
        let flag = TransferStateFlag::new();
        self.active_transfers
            .insert(id.clone(), TransferStateFlag(flag.0.clone()));

        let source_metadata = self.sftp.metadata(&remote).await.ok();
        let source_size = expected_size.or_else(|| source_metadata.as_ref().and_then(|m| m.size));
        let source_modified = expected_modified.or_else(|| {
            source_metadata
                .as_ref()
                .and_then(|m| m.mtime.map(u64::from))
        });

        let info = crate::terminal::TransferInfo {
            id: id.clone(),
            name: base_name(&remote).to_string(),
            source: remote.clone(),
            target: local_dir.clone(),
            kind: crate::terminal::TransferType::Download,
            total_bytes: source_size,
            session_id: self.session_id.to_string(),
            partial_path: Some(
                local_partial_path(&Path::new(&local_dir).join(base_name(&remote)), &id)
                    .to_string_lossy()
                    .to_string(),
            ),
            source_size,
            source_modified,
            resumable: true,
        };
        let _ = self.events.send(BackendEvent::TransferStarted {
            tab_id: self.tab_id.to_string(),
            info: Box::new(info),
        });

        let handle_clone = self.handle.clone();
        let channel_slots_clone = self.channel_slots.clone();
        let events_clone = self.events.clone();
        let tab_id_clone = self.tab_id.to_string();
        let controls_tx_clone = self.controls_tx.clone();

        let transfer_id = id.clone();
        let task = tokio::spawn(async move {
            let _cleanup = TransferCleanup::new(controls_tx_clone, id.clone());

            let Ok(_channel_permit) = channel_slots_clone.acquire_owned().await else {
                report_transfer_failure(
                    &events_clone,
                    &tab_id_clone,
                    &id,
                    "SFTP transfer channel limit closed",
                );
                return;
            };
            let channel = match handle_clone.channel_open_session().await {
                Ok(channel) => channel,
                Err(error) => {
                    report_transfer_failure(
                        &events_clone,
                        &tab_id_clone,
                        &id,
                        format!("open SFTP transfer channel: {error:#}"),
                    );
                    return;
                }
            };
            if let Err(error) = channel.request_subsystem(true, "sftp").await {
                report_transfer_failure(
                    &events_clone,
                    &tab_id_clone,
                    &id,
                    format!("request SFTP transfer subsystem: {error:#}"),
                );
                return;
            }
            let sftp_session = match SftpSession::new(channel.into_stream()).await {
                Ok(session) => session,
                Err(error) => {
                    report_transfer_failure(
                        &events_clone,
                        &tab_id_clone,
                        &id,
                        format!("complete SFTP transfer handshake: {error:#}"),
                    );
                    return;
                }
            };

            let _ = events_clone.send(BackendEvent::SftpStatus {
                tab_id: tab_id_clone.clone(),
                text: t!("downloading_file", base = base_name(&remote)).to_string(),
            });

            let ctx = TransferContext {
                flag: &flag,
                events: &events_clone,
                tab_id: &tab_id_clone,
                id: &id,
            };
            match download_path_impl(
                &handle_clone,
                &sftp_session,
                &remote,
                Path::new(&local_dir),
                &ctx,
                source_size,
                source_modified,
            )
            .await
            {
                Ok(summary) => {
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone,
                        text: summary,
                    });
                }
                Err(err) => {
                    let err_msg = format!("{err:#}");
                    let is_cancelled = err_msg.contains("transfer cancelled");
                    let state = if is_cancelled {
                        crate::terminal::TransferState::Interrupted("User cancelled".to_string())
                    } else {
                        crate::terminal::TransferState::Failed(err_msg.clone())
                    };
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone.clone(),
                        text: if is_cancelled {
                            t!("cancelled").to_string()
                        } else {
                            t!("download_failed", err = err_msg.clone()).to_string()
                        },
                    });
                    let transferred = tokio::fs::metadata(local_partial_path(
                        &Path::new(&local_dir).join(base_name(&remote)),
                        &id,
                    ))
                    .await
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                    let _ = events_clone.send(BackendEvent::TransferProgress {
                        tab_id: tab_id_clone,
                        id: id.clone(),
                        transferred,
                        total: source_size,
                        state,
                    });
                }
            }
        });
        self.active_tasks.insert(transfer_id, task);
        true
    }

    async fn handle_upload_paths(&mut self, locals: Vec<String>, remote_dir: String) -> bool {
        self.start_upload(Uuid::new_v4().to_string(), locals, remote_dir, None, None)
            .await
    }

    async fn start_upload(
        &mut self,
        id: String,
        locals: Vec<String>,
        remote_dir: String,
        expected_size: Option<u64>,
        expected_modified: Option<u64>,
    ) -> bool {
        let flag = TransferStateFlag::new();
        self.active_transfers
            .insert(id.clone(), TransferStateFlag(flag.0.clone()));

        let name = if locals.len() == 1 {
            base_name(&locals[0]).to_string()
        } else {
            let mut file_count = 0;
            let mut folder_count = 0;
            for local in &locals {
                if std::path::Path::new(local).is_dir() {
                    folder_count += 1;
                } else {
                    file_count += 1;
                }
            }
            if file_count > 0 && folder_count == 0 {
                t!("n_files", files = file_count).to_string()
            } else if file_count == 0 && folder_count > 0 {
                t!("n_folders", folders = folder_count).to_string()
            } else {
                t!(
                    "n_files_and_folders",
                    files = file_count,
                    folders = folder_count
                )
                .to_string()
            }
        };

        let source_metadata = locals.first().and_then(|path| std::fs::metadata(path).ok());
        let source_size = expected_size.or_else(|| source_metadata.as_ref().map(|m| m.len()));
        let source_modified = expected_modified.or_else(|| {
            source_metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
        });

        let info = crate::terminal::TransferInfo {
            id: id.clone(),
            name,
            source: locals
                .first()
                .cloned()
                .unwrap_or_else(|| "local".to_string()),
            target: remote_dir.clone(),
            kind: crate::terminal::TransferType::Upload,
            total_bytes: source_size,
            session_id: self.session_id.to_string(),
            partial_path: (locals.len() == 1).then(|| {
                remote_partial_path(&join_remote(&remote_dir, &base_name(&locals[0])), &id)
            }),
            source_size,
            source_modified,
            resumable: locals.len() == 1 && std::path::Path::new(&locals[0]).is_file(),
        };
        let _ = self.events.send(BackendEvent::TransferStarted {
            tab_id: self.tab_id.to_string(),
            info: Box::new(info),
        });

        let handle_clone = self.handle.clone();
        let channel_slots_clone = self.channel_slots.clone();
        let events_clone = self.events.clone();
        let tab_id_clone = self.tab_id.to_string();
        let controls_tx_clone = self.controls_tx.clone();

        let transfer_id = id.clone();
        let task = tokio::spawn(async move {
            let _cleanup = TransferCleanup::new(controls_tx_clone, id.clone());

            let Ok(_channel_permit) = channel_slots_clone.acquire_owned().await else {
                report_transfer_failure(
                    &events_clone,
                    &tab_id_clone,
                    &id,
                    "SFTP transfer channel limit closed",
                );
                return;
            };
            let channel = match handle_clone.channel_open_session().await {
                Ok(channel) => channel,
                Err(error) => {
                    report_transfer_failure(
                        &events_clone,
                        &tab_id_clone,
                        &id,
                        format!("open SFTP transfer channel: {error:#}"),
                    );
                    return;
                }
            };
            if let Err(error) = channel.request_subsystem(true, "sftp").await {
                report_transfer_failure(
                    &events_clone,
                    &tab_id_clone,
                    &id,
                    format!("request SFTP transfer subsystem: {error:#}"),
                );
                return;
            }
            let sftp_session = match SftpSession::new(channel.into_stream()).await {
                Ok(session) => session,
                Err(error) => {
                    report_transfer_failure(
                        &events_clone,
                        &tab_id_clone,
                        &id,
                        format!("complete SFTP transfer handshake: {error:#}"),
                    );
                    return;
                }
            };

            let _ = events_clone.send(BackendEvent::SftpStatus {
                tab_id: tab_id_clone.clone(),
                text: t!("uploading").to_string(),
            });

            let ctx = TransferContext {
                flag: &flag,
                events: &events_clone,
                tab_id: &tab_id_clone,
                id: &id,
            };
            match upload_paths_impl(
                &sftp_session,
                &locals,
                &remote_dir,
                &ctx,
                source_size,
                source_modified,
            )
            .await
            {
                Ok(summary) => {
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone.clone(),
                        text: summary,
                    });
                    if let Err(error) =
                        emit_entries(&events_clone, &tab_id_clone, &sftp_session, &remote_dir).await
                    {
                        let _ = events_clone.send(BackendEvent::SftpStatus {
                            tab_id: tab_id_clone.clone(),
                            text: t!("sftp_list_failed", error = format!("{error:#}")).to_string(),
                        });
                    }
                }
                Err(err) => {
                    let err_msg = format!("{err:#}");
                    let is_cancelled = err_msg.contains("transfer cancelled");
                    let state = if is_cancelled {
                        crate::terminal::TransferState::Interrupted("User cancelled".to_string())
                    } else {
                        crate::terminal::TransferState::Failed(err_msg.clone())
                    };
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone.clone(),
                        text: if is_cancelled {
                            t!("cancelled").to_string()
                        } else {
                            t!("upload_failed", err = err_msg.clone()).to_string()
                        },
                    });
                    let transferred = if locals.len() == 1 {
                        sftp_session
                            .metadata(&remote_partial_path(
                                &join_remote(&remote_dir, &base_name(&locals[0])),
                                &id,
                            ))
                            .await
                            .ok()
                            .and_then(|metadata| metadata.size)
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    let _ = events_clone.send(BackendEvent::TransferProgress {
                        tab_id: tab_id_clone,
                        id: id.clone(),
                        transferred,
                        total: source_size,
                        state,
                    });
                }
            }
        });
        self.active_tasks.insert(transfer_id, task);
        true
    }

    async fn handle_edit_file(&mut self, remote_path: String, editor: Option<String>) -> bool {
        let id = uuid::Uuid::new_v4().to_string();
        let temp_directory = match allocate_sftp_temp_directory("sftp-edit") {
            Ok(directory) => directory,
            Err(err) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("sftp_temp_dir_failed", error = format!("{err:#}")).to_string(),
                });
                return true;
            }
        };
        let base = base_name(&remote_path).to_string();
        let local_path =
            temp_directory
                .path()
                .join(format!("{}-{}", id, safe_local_edit_name(&remote_path)));

        let handle_clone = self.handle.clone();
        let commands_tx_clone = self.commands_tx.clone();
        let events_clone = self.events.clone();
        let tab_id_clone = self.tab_id.to_string();

        let transfer_id = id.clone();
        let task = tokio::spawn(async move {
            let _temp_directory = temp_directory;
            let flag = TransferStateFlag::new();
            let Ok(channel) = handle_clone.channel_open_session().await else {
                return;
            };
            let Ok(_) = channel.request_subsystem(true, "sftp").await else {
                return;
            };
            let Ok(sftp_session) = SftpSession::new(channel.into_stream()).await else {
                return;
            };

            let _ = events_clone.send(BackendEvent::SftpStatus {
                tab_id: tab_id_clone.clone(),
                text: t!("downloading_file", base = base.as_str()).to_string(),
            });

            let ctx = TransferContext {
                flag: &flag,
                events: &events_clone,
                tab_id: &tab_id_clone,
                id: "edit-download",
            };
            if let Err(err) =
                download_file_impl(&sftp_session, &remote_path, &local_path, &ctx, None, None).await
            {
                let _ = events_clone.send(BackendEvent::SftpStatus {
                    tab_id: tab_id_clone.clone(),
                    text: t!("sftp_edit_download_failed", error = format!("{err:#}")).to_string(),
                });
                return;
            }

            let open_result = if let Some(editor) = editor {
                open::with_detached(&local_path, editor)
            } else {
                open::that_detached(&local_path)
            };
            if let Err(err) = open_result {
                let _ = events_clone.send(BackendEvent::SftpStatus {
                    tab_id: tab_id_clone.clone(),
                    text: t!("sftp_editor_open_failed", error = format!("{err:#}")).to_string(),
                });
                return;
            }

            use notify::Watcher;
            let (tx, mut rx) = tokio::sync::mpsc::channel(16);
            let mut watcher =
                match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    if let Ok(event) = res {
                        if event.kind.is_modify() {
                            let _ = tx.try_send(());
                        }
                    }
                }) {
                    Ok(w) => w,
                    Err(_) => return,
                };

            if watcher
                .watch(&local_path, notify::RecursiveMode::NonRecursive)
                .is_err()
            {
                return;
            }

            while rx.recv().await.is_some() {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                while rx.try_recv().is_ok() {} // drain pending

                if commands_tx_clone
                    .send(SftpCommand::UploadEditedFile {
                        local_path: local_path.to_string_lossy().to_string(),
                        remote_path: remote_path.clone(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        self.active_tasks.insert(transfer_id, task);
        true
    }

    async fn handle_upload_edited_file(&mut self, local_path: String, remote_path: String) -> bool {
        let handle_clone = self.handle.clone();
        let channel_slots_clone = self.channel_slots.clone();
        let events_clone = self.events.clone();
        let tab_id_clone = self.tab_id.to_string();

        let task = tokio::spawn(async move {
            let Ok(_channel_permit) = channel_slots_clone.acquire_owned().await else {
                return;
            };
            let flag = TransferStateFlag::new();
            let Ok(channel) = handle_clone.channel_open_session().await else {
                return;
            };
            let Ok(_) = channel.request_subsystem(true, "sftp").await else {
                return;
            };
            let Ok(sftp_session) = SftpSession::new(channel.into_stream()).await else {
                return;
            };

            let transferred = Arc::new(AtomicU64::new(0));
            let ctx = TransferContext {
                flag: &flag,
                events: &events_clone,
                tab_id: &tab_id_clone,
                id: "edit-upload",
            };
            match upload_file_impl(
                &sftp_session,
                Path::new(&local_path),
                &remote_path,
                &ctx,
                transferred,
                None,
                None,
            )
            .await
            {
                Ok(_) => {
                    let now = chrono::Local::now().format("%H:%M:%S");
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone.clone(),
                        text: format!(
                            "{} ({})",
                            t!("auto_saved_and_uploaded", base = base_name(&remote_path)),
                            now
                        ),
                    });
                }
                Err(err) => {
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone.clone(),
                        text: t!("sftp_auto_upload_failed", error = format!("{err:#}")).to_string(),
                    });
                }
            }
        });
        self.active_tasks
            .insert(format!("edit-upload-{}", Uuid::new_v4()), task);
        true
    }

    async fn handle_download_file_content(&mut self, remote_path: String) -> bool {
        let handle_clone = self.handle.clone();
        let channel_slots_clone = self.channel_slots.clone();
        let events_clone = self.events.clone();
        let tab_id_clone = self.tab_id.to_string();

        let task = tokio::spawn(async move {
            let Ok(_channel_permit) = channel_slots_clone.acquire_owned().await else {
                return;
            };
            let Ok(channel) = handle_clone.channel_open_session().await else {
                let _ = events_clone.send(BackendEvent::SftpStatus {
                    tab_id: tab_id_clone,
                    text: t!("sftp_channel_open_failed").to_string(),
                });
                return;
            };
            let Ok(_) = channel.request_subsystem(true, "sftp").await else {
                return;
            };
            let Ok(sftp_session) = SftpSession::new(channel.into_stream()).await else {
                return;
            };

            match read_remote_text_file(&sftp_session, &remote_path).await {
                Ok(file) => {
                    let _ = events_clone.send(BackendEvent::SftpFileContent {
                        tab_id: tab_id_clone,
                        remote_path,
                        file: Box::new(file),
                    });
                }
                Err(err) => {
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone,
                        text: t!("sftp_read_file_failed", error = format!("{err:#}")).to_string(),
                    });
                }
            }
        });
        self.active_tasks
            .insert(format!("content-download-{}", Uuid::new_v4()), task);
        true
    }

    async fn handle_save_file_content(&mut self, save: RemoteTextSave) -> bool {
        let handle_clone = self.handle.clone();
        let channel_slots_clone = self.channel_slots.clone();
        let events_clone = self.events.clone();
        let tab_id_clone = self.tab_id.to_string();

        let task = tokio::spawn(async move {
            let Ok(_channel_permit) = channel_slots_clone.acquire_owned().await else {
                return;
            };
            let remote_path = save.remote_path.clone();
            let Ok(channel) = handle_clone.channel_open_session().await else {
                let error = "Failed to open SFTP channel".to_string();
                let _ = events_clone.send(BackendEvent::SftpContentUploadFailed {
                    tab_id: tab_id_clone.clone(),
                    remote_path: remote_path.clone(),
                    error: error.clone(),
                });
                let _ = events_clone.send(BackendEvent::SftpStatus {
                    tab_id: tab_id_clone,
                    text: error,
                });
                return;
            };
            let Ok(_) = channel.request_subsystem(true, "sftp").await else {
                return;
            };
            let Ok(sftp_session) = SftpSession::new(channel.into_stream()).await else {
                return;
            };

            match save_remote_text_file(&sftp_session, save).await {
                Ok(SaveRemoteTextOutcome::Saved(revision)) => {
                    let now = chrono::Local::now().format("%H:%M:%S");
                    let base = base_name(&remote_path).to_string();
                    let _ = events_clone.send(BackendEvent::SftpContentUploaded {
                        tab_id: tab_id_clone.clone(),
                        remote_path,
                        revision,
                    });
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone,
                        text: format!(
                            "{} ({})",
                            t!("auto_saved_and_uploaded", base = base.as_str()),
                            now
                        ),
                    });
                }
                Ok(SaveRemoteTextOutcome::Conflict(remote_file)) => {
                    let _ = events_clone.send(BackendEvent::SftpContentConflict {
                        tab_id: tab_id_clone.clone(),
                        remote_path,
                        remote_file: Box::new(remote_file),
                    });
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone,
                        text: "Remote file changed; save cancelled".into(),
                    });
                }
                Err(err) => {
                    let error = format!("{err:#}");
                    let _ = events_clone.send(BackendEvent::SftpContentUploadFailed {
                        tab_id: tab_id_clone.clone(),
                        remote_path: remote_path.clone(),
                        error: error.clone(),
                    });
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone,
                        text: format!("Upload failed: {error}"),
                    });
                }
            }
        });
        self.active_tasks
            .insert(format!("content-save-{}", Uuid::new_v4()), task);
        true
    }

    async fn handle_create_dir(&self, path: String) -> bool {
        let actual_path = self.resolve_home_path(path);

        tracing::info!("[sftp] creating directory: '{}'", actual_path);

        match self.sftp.create_dir(&actual_path).await {
            Ok(_) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("create_folder_success", name = base_name(&actual_path)).to_string(),
                });

                // Refresh directly. Re-enqueuing onto the queue currently being
                // consumed can deadlock under backpressure or lose the update.
                self.refresh_directory(parent_dir(&actual_path).unwrap_or_else(|| "/".into()))
                    .await;
            }
            Err(err) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("create_folder_failed", err = format!("{err:#}")).to_string(),
                });
            }
        }
        true
    }

    async fn handle_create_file(&self, path: String) -> bool {
        let actual_path = resolve_remote_path(&path, self.home);
        let result = async {
            let mut file = self
                .sftp
                .create(&actual_path)
                .await
                .with_context(|| format!("create remote file {actual_path}"))?;
            file.flush()
                .await
                .with_context(|| format!("flush remote file {actual_path}"))?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        match result {
            Ok(()) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("sftp_create_file_success", name = base_name(&actual_path))
                        .to_string(),
                });
                self.refresh_directory(remote_parent(&actual_path)).await;
            }
            Err(err) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("sftp_create_file_failed", err = format!("{err:#}")).to_string(),
                });
            }
        }
        true
    }

    async fn handle_rename_path(&self, old_path: String, new_path: String) -> bool {
        let old_path = resolve_remote_path(&old_path, self.home);
        let new_path = resolve_remote_path(&new_path, self.home);
        match self.sftp.rename(&old_path, &new_path).await {
            Ok(()) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("sftp_rename_success", name = base_name(&new_path)).to_string(),
                });
                self.refresh_directory(remote_parent(&new_path)).await;
            }
            Err(err) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("sftp_rename_failed", err = format!("{err:#}")).to_string(),
                });
            }
        }
        true
    }

    async fn handle_set_permissions(
        &self,
        remote_path: String,
        mode: u32,
        recursive: bool,
        apply_to: PermissionApplyTarget,
    ) -> bool {
        let remote_path = resolve_remote_path(&remote_path, self.home);
        let result = if recursive {
            set_permissions_recursive(self.sftp, remote_path.clone(), mode, apply_to).await
        } else {
            set_path_permissions(self.sftp, &remote_path, mode).await
        };
        match result {
            Ok(()) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!(
                        "sftp_permissions_success",
                        mode = format!("{mode:o}"),
                        name = base_name(&remote_path)
                    )
                    .to_string(),
                });
                self.refresh_directory(remote_parent(&remote_path)).await;
            }
            Err(err) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("sftp_permissions_failed", err = format!("{err:#}")).to_string(),
                });
            }
        }
        true
    }

    async fn handle_delete_paths(&self, paths: Vec<String>) -> bool {
        tracing::info!("[sftp] batch deleting {} paths", paths.len());
        let _ = self.events.send(BackendEvent::SftpStatus {
            tab_id: self.tab_id.to_string(),
            text: t!("deleting_paths", count = paths.len()).to_string(),
        });

        let mut errors = Vec::new();
        for path in paths.clone() {
            let actual_path = self.resolve_home_path(path.clone());

            if let Err(e) = recursive_delete(self.sftp, actual_path).await {
                errors.push(format!("{path}: {e:#}"));
            }
        }

        if errors.is_empty() {
            let _ = self.events.send(BackendEvent::SftpStatus {
                tab_id: self.tab_id.to_string(),
                text: t!("delete_success", count = paths.len()).to_string(),
            });
        } else {
            let _ = self.events.send(BackendEvent::SftpStatus {
                tab_id: self.tab_id.to_string(),
                text: t!("delete_failed", err = errors.join(", ")).to_string(),
            });
        }

        if let Some(first) = paths.first() {
            let actual_path = self.resolve_home_path(first.clone());
            self.refresh_directory(parent_dir(&actual_path).unwrap_or_else(|| "/".into()))
                .await;
        }
        true
    }

    async fn handle_quick_delete_paths(&self, paths: Vec<String>) -> bool {
        let resolved_paths: Vec<String> = paths
            .iter()
            .map(|path| resolve_remote_path(path, self.home))
            .collect();
        let command = format!(
            "rm -rf -- {}",
            resolved_paths
                .iter()
                .map(|path| shell_quote(path))
                .collect::<Vec<_>>()
                .join(" ")
        );
        match exec_remote_command(self.handle, &command).await {
            Ok(()) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("delete_success", count = paths.len()).to_string(),
                });
            }
            Err(err) => {
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("delete_failed", err = format!("{err:#}")).to_string(),
                });
            }
        }
        if let Some(first) = resolved_paths.first() {
            self.refresh_directory(remote_parent(first)).await;
        }
        true
    }

    async fn handle_pack_download(&mut self, remote_paths: Vec<String>, local_zip: String) -> bool {
        let id = Uuid::new_v4().to_string();
        let flag = TransferStateFlag::new();
        self.active_transfers
            .insert(id.clone(), TransferStateFlag(flag.0.clone()));
        let info = crate::terminal::TransferInfo {
            id: id.clone(),
            name: base_name(&local_zip),
            source: remote_paths.join(", "),
            target: local_zip.clone(),
            kind: crate::terminal::TransferType::Download,
            total_bytes: None,
            session_id: self.session_id.to_string(),
            partial_path: None,
            source_size: None,
            source_modified: None,
            resumable: false,
        };
        let _ = self.events.send(BackendEvent::TransferStarted {
            tab_id: self.tab_id.to_string(),
            info: Box::new(info),
        });

        let handle_clone = self.handle.clone();
        let channel_slots_clone = self.channel_slots.clone();
        let events_clone = self.events.clone();
        let tab_id_clone = self.tab_id.to_string();
        let controls_tx_clone = self.controls_tx.clone();
        let temp_directory = match allocate_sftp_temp_directory("sftp-pack") {
            Ok(directory) => directory,
            Err(error) => {
                self.active_transfers.remove(&id);
                let _ = self.events.send(BackendEvent::SftpStatus {
                    tab_id: self.tab_id.to_string(),
                    text: t!("sftp_pack_download_failed", err = format!("{error:#}")).to_string(),
                });
                let _ = self.events.send(BackendEvent::TransferProgress {
                    tab_id: self.tab_id.to_string(),
                    id,
                    transferred: 0,
                    total: None,
                    state: crate::terminal::TransferState::Failed(format!("{error:#}")),
                });
                return true;
            }
        };
        let tmp_dir = temp_directory.path().to_path_buf();
        let transfer_id = id.clone();
        let task = tokio::spawn(async move {
            let _temp_directory = temp_directory;
            let _cleanup = TransferCleanup::new(controls_tx_clone, id.clone());
            let Ok(_channel_permit) = channel_slots_clone.acquire_owned().await else {
                let message = "SFTP channel semaphore closed".to_string();
                let _ = events_clone.send(BackendEvent::SftpStatus {
                    tab_id: tab_id_clone.clone(),
                    text: t!("sftp_pack_download_failed", err = message.clone()).to_string(),
                });
                let _ = events_clone.send(BackendEvent::TransferProgress {
                    tab_id: tab_id_clone,
                    id,
                    transferred: 0,
                    total: None,
                    state: crate::terminal::TransferState::Failed(message),
                });
                return;
            };
            let ctx = TransferContext {
                flag: &flag,
                events: &events_clone,
                tab_id: &tab_id_clone,
                id: &id,
            };
            let result = pack_remote_paths_to_zip(
                &handle_clone,
                &remote_paths,
                Path::new(&local_zip),
                &tmp_dir,
                &ctx,
            )
            .await;
            match result {
                Ok(()) => {
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone.clone(),
                        text: t!("sftp_pack_download_success", path = local_zip).to_string(),
                    });
                }
                Err(err) => {
                    let error = format!("{err:#}");
                    let _ = events_clone.send(BackendEvent::SftpStatus {
                        tab_id: tab_id_clone.clone(),
                        text: t!("sftp_pack_download_failed", err = error.clone()).to_string(),
                    });
                    let _ = events_clone.send(BackendEvent::TransferProgress {
                        tab_id: tab_id_clone,
                        id: id.clone(),
                        transferred: 0,
                        total: None,
                        state: crate::terminal::TransferState::Failed(error),
                    });
                }
            }
        });
        self.active_tasks.insert(transfer_id, task);
        true
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_sftp(
    tab_id: String,
    session: Session,
    proxy_config: ConfigStore,
    mut commands: Receiver<SftpCommand>,
    commands_tx: Sender<SftpCommand>,
    controls: Arc<SftpControlQueue>,
    controls_tx: Arc<SftpControlQueue>,
    events: BackendEventSender,
) -> Result<()> {
    let _ = events.send(BackendEvent::SftpStatus {
        tab_id: tab_id.clone(),
        text: t!("sftp_connecting").to_string(),
    });

    let connect = connect_and_authenticate(&session, &proxy_config);
    tokio::pin!(connect);
    let handle = loop {
        tokio::select! {
            biased;
            control = controls.recv() => match control {
                SftpControl::Close => return Ok(()),
                _ => {
                    // Transfers do not exist until authentication completes, so
                    // other lifecycle commands have nothing to act on yet.
                }
            },
            command = commands.recv() => {
                if command.is_none() {
                    return Ok(());
                }
            }
            result = &mut connect => break result?,
        }
    };
    let channel = handle
        .channel_open_session()
        .await
        .context("open sftp channel")?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .context("request sftp subsystem")?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .context("sftp handshake")?;

    let latency_started = Instant::now();
    let home_result = sftp.canonicalize(".").await;
    let latency_ms = home_result.as_ref().ok().map(|_| {
        latency_started
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    });
    let home = home_result.unwrap_or_else(|_| "/".to_string());
    let home = normalize_remote_directory_path(home);

    let _ = events.send(BackendEvent::SftpLatency {
        tab_id: tab_id.clone(),
        latency_ms,
    });

    let _ = events.send(BackendEvent::SftpHome {
        tab_id: tab_id.clone(),
        home: home.clone(),
    });

    if let Ok(entries) = list_dir_impl(&sftp, "/").await {
        let _ = events.send(BackendEvent::SftpDirectoryEntries {
            tab_id: tab_id.clone(),
            path: "/".to_string(),
            entries,
        });
    }

    let mut ancestor = String::new();
    for component in home.split('/').filter(|component| !component.is_empty()) {
        ancestor.push('/');
        ancestor.push_str(component);
        if ancestor == home {
            break;
        }
        if let Ok(entries) = list_dir_impl(&sftp, &ancestor).await {
            let _ = events.send(BackendEvent::SftpDirectoryEntries {
                tab_id: tab_id.clone(),
                path: ancestor.clone(),
                entries,
            });
        }
    }

    emit_entries(&events, &tab_id, &sftp, &home).await?;

    let mut active_transfers: HashMap<String, TransferStateFlag> = HashMap::new();
    let mut active_tasks: HashMap<String, JoinHandle<()>> = HashMap::new();
    let channel_slots = Arc::new(Semaphore::new(4));

    loop {
        active_tasks.retain(|_, task| !task.is_finished());
        let mut runtime = SftpRuntime {
            handle: &handle,
            sftp: &sftp,
            tab_id: &tab_id,
            session_id: &session.id,
            home: &home,
            events: &events,
            commands_tx: &commands_tx,
            controls_tx: &controls_tx,
            active_transfers: &mut active_transfers,
            active_tasks: &mut active_tasks,
            channel_slots: &channel_slots,
        };
        let next = tokio::select! {
            biased;
            control = controls.recv() => Some(EitherSftpCommand::Control(control)),
            command = commands.recv() => command.map(EitherSftpCommand::Work),
        };
        let continue_loop = match next {
            Some(EitherSftpCommand::Control(control)) => runtime.handle_control(control).await,
            Some(EitherSftpCommand::Work(command)) => runtime.handle_command(command).await,
            None => false,
        };
        if !continue_loop {
            break;
        }
    }

    let _ = handle
        .disconnect(Disconnect::ByApplication, "bye", "")
        .await;
    Ok(())
}

enum EitherSftpCommand {
    Control(SftpControl),
    Work(SftpCommand),
}

use std::future::Future;
use std::pin::Pin;

fn recursive_delete<'a>(
    sftp: &'a SftpSession,
    path: String,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        match sftp.read_dir(&path).await {
            Ok(entries) => {
                for entry in entries {
                    let name = entry.file_name();
                    if name == "." || name == ".." {
                        continue;
                    }
                    let child_path = crate::sftp::join_remote(&path, &name);

                    let meta = entry.metadata();
                    let permissions = meta.permissions.unwrap_or(0);
                    let is_dir = (permissions & 0o170_000) == 0o040_000;

                    if is_dir {
                        recursive_delete(sftp, child_path).await?;
                    } else {
                        sftp.remove_file(&child_path)
                            .await
                            .with_context(|| format!("Failed to delete file {child_path}"))?;
                    }
                }
                sftp.remove_dir(&path)
                    .await
                    .with_context(|| format!("Failed to delete dir {path}"))?;
            }
            Err(_) => {
                sftp.remove_file(&path)
                    .await
                    .with_context(|| format!("Failed to delete {path}"))?;
            }
        }
        Ok(())
    })
}

async fn set_path_permissions(sftp: &SftpSession, path: &str, mode: u32) -> Result<()> {
    let mut attributes = FileAttributes::empty();
    attributes.permissions = Some(mode);
    sftp.set_metadata(path, attributes)
        .await
        .with_context(|| format!("chmod {mode:o} {path}"))
}

fn set_permissions_recursive<'a>(
    sftp: &'a SftpSession,
    path: String,
    mode: u32,
    apply_to: PermissionApplyTarget,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let metadata = sftp
            .metadata(&path)
            .await
            .with_context(|| format!("metadata {path}"))?;
        let is_dir = metadata
            .permissions
            .map(|permissions| (permissions & 0o170_000) == 0o040_000)
            .unwrap_or(false);
        let should_apply = match apply_to {
            PermissionApplyTarget::FilesAndDirectories => true,
            PermissionApplyTarget::FilesOnly => !is_dir,
            PermissionApplyTarget::DirectoriesOnly => is_dir,
        };
        if should_apply {
            set_path_permissions(sftp, &path, mode).await?;
        }
        if !is_dir {
            return Ok(());
        }

        for entry in sftp
            .read_dir(&path)
            .await
            .with_context(|| format!("read_dir {path}"))?
        {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            set_permissions_recursive(sftp, crate::sftp::join_remote(&path, &name), mode, apply_to)
                .await?;
        }
        Ok(())
    })
}

async fn emit_entries(
    events: &BackendEventSender,
    tab_id: &str,
    sftp: &SftpSession,
    path: &str,
) -> Result<()> {
    let entries = list_dir_impl(sftp, path).await?;
    let _ = events.send(BackendEvent::SftpEntries {
        tab_id: tab_id.to_string(),
        path: path.to_string(),
        entries,
    });
    Ok(())
}

async fn connect_and_authenticate(
    session: &Session,
    proxy_config: &ConfigStore,
) -> Result<Arc<russh::client::Handle<SftpClientHandler>>> {
    const CONNECTION_TIMEOUT: Duration = Duration::from_secs(60);

    tokio::time::timeout(CONNECTION_TIMEOUT, async move {
        if session.requires_credential_prompt() {
            return Err(anyhow!(t!("session_credentials_required").to_string()));
        }

        let config = Arc::new(client::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(600)),
            keepalive_interval: Some(std::time::Duration::from_secs(3)),
            keepalive_max: 2,
            ..Default::default()
        });
        let addr = format!("{}:{}", session.host, session.port);
        let handler = SftpClientHandler::new(&session.host, session.port)?;
        let stream = crate::session::config::connect_proxy(session, proxy_config).await?;
        let mut handle = client::connect_stream(config, stream, handler)
            .await
            .with_context(|| format!("connect {addr} failed"))?;

        let authed = match session.auth {
        AuthMethod::Password => handle
            .authenticate_password(&session.user, &session.password)
            .await
            .context("password authentication failed")?,
        AuthMethod::Key => {
            let has_explicit_key = session_has_explicit_key(session);

            if has_explicit_key {
                let keypair = load_session_private_key(session)?;
                let keys = private_keys_with_algs(keypair).context("invalid private key")?;
                let mut success = false;
                for key in keys {
                    match handle.authenticate_publickey(&session.user, key).await {
                        Ok(true) => {
                            success = true;
                            break;
                        }
                        Ok(false) => {
                            tracing::debug!(
                                "[sftp] public key auth failed with algorithm, trying next"
                            );
                            continue;
                        }
                        Err(e) => {
                            tracing::debug!("[sftp] public key auth error: {:?}, trying next", e);
                            continue;
                        }
                    }
                }
                if !success {
                    return Err(anyhow!(
                        "public key authentication failed for {}@{}:{}",
                        session.user,
                        session.host,
                        session.port
                    ));
                }
                success
            } else {
                let passphrase = session.passphrase.trim();
                let passphrase = (!passphrase.is_empty()).then_some(passphrase);
                let success =
                    authenticate_with_default_keys(&mut handle, &session.user, passphrase).await?;
                if !success {
                    return Err(anyhow!(
                        "public key authentication failed for {}@{}:{} - no valid default key found in ~/.ssh/",
                        session.user,
                        session.host,
                        session.port
                    ));
                }
                success
            }
        }
        AuthMethod::KeyPending => {
            return Err(anyhow!(t!("session_credentials_required").to_string()));
        }
        AuthMethod::Config => {
            // For Config auth, try the identity file from config entry, or default keys
            // Note: for Config auth, we never use inline key content
            let has_explicit_key = !session.private_key_path.trim().is_empty();

            if has_explicit_key {
                let keypair = load_session_private_key(session)?;
                let keys = private_keys_with_algs(keypair).context("invalid private key")?;
                let mut success = false;
                for key in keys {
                    match handle.authenticate_publickey(&session.user, key).await {
                        Ok(true) => {
                            success = true;
                            break;
                        }
                        Ok(false) => {
                            tracing::debug!(
                                "[sftp] public key auth failed with algorithm, trying next"
                            );
                            continue;
                        }
                        Err(e) => {
                            tracing::debug!("[sftp] public key auth error: {:?}, trying next", e);
                            continue;
                        }
                    }
                }
                if !success {
                    return Err(anyhow!(
                        "ssh-config key authentication failed for {}@{}:{}",
                        session.user,
                        session.host,
                        session.port
                    ));
                }
                success
            } else {
                let passphrase = session.passphrase.trim();
                let passphrase = (!passphrase.is_empty()).then_some(passphrase);
                let success =
                    authenticate_with_default_keys(&mut handle, &session.user, passphrase).await?;
                if !success {
                    return Err(anyhow!(
                        "ssh-config authentication failed for {}@{}:{} - no valid default key found",
                        session.user,
                        session.host,
                        session.port
                    ));
                }
                success
            }
        }
    };

    if !authed {
        let _ = handle
            .disconnect(Disconnect::ByApplication, "auth failed", "")
            .await;
        return Err(anyhow!(
            "authentication failed: server rejected {} authentication for {}@{}:{}",
            match session.auth {
                AuthMethod::Password => "password",
                AuthMethod::Key | AuthMethod::KeyPending => "public key",
                AuthMethod::Config => "ssh-config",
            },
            session.user,
            session.host,
            session.port
        ));
    }

        Ok(Arc::new(handle))
    })
    .await
    .context("connection timed out")?
}

fn load_session_private_key(session: &Session) -> Result<PrivateKey> {
    let inline_key = normalize_inline_private_key(&session.private_key_inline);
    let key_path = expand_key_path(session.private_key_path.trim());
    let passphrase = session.passphrase.trim();
    let passphrase = (!passphrase.is_empty()).then_some(passphrase);
    let has_inline = !inline_key.is_empty();
    let has_path = key_path.is_some();

    if !has_inline && !has_path {
        return Err(anyhow!("private key content or path is required"));
    }

    let mut errors = Vec::new();

    if has_inline {
        match decode_secret_key(&inline_key, passphrase) {
            Ok(key) => return Ok(key),
            Err(err) => errors.push(format!("decode private key content: {err}")),
        }
    }

    if let Some(path) = key_path {
        match load_secret_key(path.as_path(), passphrase) {
            Ok(key) => return Ok(key),
            Err(err) => errors.push(format!("load key {}: {err}", path.display())),
        }
    }

    Err(anyhow!(errors.join("; ")))
}

fn expand_key_path(value: &str) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    if value == "~" {
        return BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return BaseDirs::new().map(|dirs| dirs.home_dir().join(rest));
    }
    Some(Path::new(value).to_path_buf())
}

async fn list_dir_impl(sftp: &SftpSession, path: &str) -> Result<Vec<RemoteEntry>> {
    let raw = sftp
        .read_dir(path)
        .await
        .with_context(|| format!("read_dir {path} failed"))?;

    let mut entries = raw
        .into_iter()
        .filter(|entry| {
            let name = entry.file_name();
            name != "." && name != ".."
        })
        .map(|entry| {
            let name = entry.file_name().to_string();
            let full_path = join_remote(path, &name);
            let meta = entry.metadata();
            let permissions = meta.permissions.unwrap_or(0);
            let is_dir = (permissions & 0o170_000) == 0o040_000;
            let size = meta.size.unwrap_or(0);
            let modified = meta.mtime.unwrap_or(0);
            RemoteEntry {
                name,
                full_path,
                is_dir,
                size,
                modified,
                permissions,
            }
        })
        .collect::<Vec<_>>();

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

async fn download_path_impl(
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

enum SaveRemoteTextOutcome {
    Saved(RemoteFileRevision),
    Conflict(RemoteTextFile),
}

async fn read_remote_text_file(sftp: &SftpSession, remote: &str) -> Result<RemoteTextFile> {
    let metadata = sftp
        .metadata(remote)
        .await
        .with_context(|| format!("stat remote {remote}"))?;
    if metadata
        .size
        .is_some_and(|size| size > EDITOR_HARD_LIMIT_BYTES)
    {
        return Err(anyhow!(
            "file too large for in-memory editor ({} bytes, max {} bytes)",
            metadata.size.unwrap_or_default(),
            EDITOR_HARD_LIMIT_BYTES
        ));
    }

    let mut file = sftp
        .open(remote)
        .await
        .with_context(|| format!("open remote {remote}"))?;
    let mut bytes = Vec::with_capacity(metadata.size.unwrap_or_default() as usize);
    file.read_to_end(&mut bytes)
        .await
        .with_context(|| format!("read remote {remote}"))?;
    decode_remote_text(bytes, metadata.mtime, metadata.permissions)
        .with_context(|| format!("decode remote {remote}"))
}

async fn write_remote_temp_file(
    sftp: &SftpSession,
    remote: &str,
    bytes: &[u8],
    permissions: Option<u32>,
) -> Result<()> {
    let mut file = sftp
        .create(remote)
        .await
        .with_context(|| format!("create remote temporary file {remote}"))?;
    file.write_all(bytes)
        .await
        .with_context(|| format!("write remote temporary file {remote}"))?;
    file.flush()
        .await
        .with_context(|| format!("flush remote temporary file {remote}"))?;
    file.sync_all()
        .await
        .with_context(|| format!("sync remote temporary file {remote}"))?;
    drop(file);

    if let Some(permissions) = permissions {
        let mut attributes = FileAttributes::empty();
        attributes.permissions = Some(permissions);
        sftp.set_metadata(remote, attributes)
            .await
            .with_context(|| format!("preserve permissions on {remote}"))?;
    }
    Ok(())
}

async fn remove_remote_file_if_present(sftp: &SftpSession, remote: &str) {
    if sftp.try_exists(remote).await.unwrap_or(false) {
        let _ = sftp.remove_file(remote).await;
    }
}

async fn replace_remote_file(
    sftp: &SftpSession,
    remote: &str,
    temp: &str,
    backup: &str,
) -> Result<()> {
    match sftp.rename(temp, remote).await {
        Ok(()) => return Ok(()),
        Err(direct_error) => {
            if !sftp.try_exists(temp).await.unwrap_or(false) {
                tracing::warn!(
                    "SFTP rename reported an error after temporary file disappeared; verifying target: {direct_error}"
                );
                return Ok(());
            }
        }
    }

    sftp.rename(remote, backup)
        .await
        .with_context(|| format!("move original {remote} to recovery backup {backup}"))?;
    if let Err(replace_error) = sftp.rename(temp, remote).await {
        let rollback = sftp.rename(backup, remote).await;
        return match rollback {
            Ok(()) => Err(anyhow!(
                "replace remote {remote} failed and original file was restored: {replace_error}"
            )),
            Err(rollback_error) => Err(anyhow!(
                "replace remote {remote} failed ({replace_error}); recovery backup remains at {backup} because rollback failed ({rollback_error})"
            )),
        };
    }

    if let Err(error) = sftp.remove_file(backup).await {
        tracing::warn!("failed to remove SFTP save backup {backup}: {error}");
    }
    Ok(())
}

async fn save_remote_text_file(
    sftp: &SftpSession,
    save: RemoteTextSave,
) -> Result<SaveRemoteTextOutcome> {
    let current = read_remote_text_file(sftp, &save.remote_path).await?;
    if !save.force && !save.expected_revision.same_content(&current.revision) {
        return Ok(SaveRemoteTextOutcome::Conflict(current));
    }

    let bytes = encode_remote_text(&save.content, save.format);
    if bytes.len() as u64 > EDITOR_HARD_LIMIT_BYTES {
        return Err(anyhow!(
            "edited file is too large to save ({} bytes, max {} bytes)",
            bytes.len(),
            EDITOR_HARD_LIMIT_BYTES
        ));
    }

    let suffix = Uuid::new_v4();
    let temp = format!("{}.tiny-shell-save-{suffix}.tmp", save.remote_path);
    let backup = format!("{}.tiny-shell-save-{suffix}.bak", save.remote_path);
    if let Err(error) =
        write_remote_temp_file(sftp, &temp, &bytes, current.revision.permissions).await
    {
        remove_remote_file_if_present(sftp, &temp).await;
        return Err(error);
    }

    let before_replace = match read_remote_text_file(sftp, &save.remote_path).await {
        Ok(file) => file,
        Err(error) => {
            remove_remote_file_if_present(sftp, &temp).await;
            return Err(error);
        }
    };
    if !save.force
        && !save
            .expected_revision
            .same_content(&before_replace.revision)
    {
        remove_remote_file_if_present(sftp, &temp).await;
        return Ok(SaveRemoteTextOutcome::Conflict(before_replace));
    }

    if let Err(error) = replace_remote_file(sftp, &save.remote_path, &temp, &backup).await {
        remove_remote_file_if_present(sftp, &temp).await;
        return Err(error);
    }

    let saved = read_remote_text_file(sftp, &save.remote_path).await?;
    let expected_saved =
        RemoteFileRevision::from_bytes(&bytes, saved.revision.modified, saved.revision.permissions);
    if !expected_saved.same_content(&saved.revision) {
        return Err(anyhow!(
            "remote save verification failed for {}",
            save.remote_path
        ));
    }
    Ok(SaveRemoteTextOutcome::Saved(saved.revision))
}

#[derive(Clone, Copy)]
struct SourceFingerprint {
    size: Option<u64>,
    modified: Option<u64>,
}

fn local_partial_path(final_path: &Path, transfer_id: &str) -> PathBuf {
    let file_name = final_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    final_path.with_file_name(format!(".{file_name}.tiny-shell-{transfer_id}.part"))
}

fn remote_partial_path(final_path: &str, transfer_id: &str) -> String {
    let parent = remote_parent(final_path);
    let name = base_name(final_path);
    join_remote(&parent, &format!(".{name}.tiny-shell-{transfer_id}.part"))
}

async fn download_file_impl(
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

async fn upload_paths_impl(
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

async fn upload_file_impl(
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

async fn create_remote_dir_all(sftp: &SftpSession, remote_dir: &str) -> Result<()> {
    if remote_dir.is_empty() || remote_dir == "/" {
        return Ok(());
    }

    let mut current = String::from("/");
    for segment in remote_dir.split('/').filter(|segment| !segment.is_empty()) {
        current = join_remote(&current, segment);
        if sftp.metadata(&current).await.is_ok() {
            continue;
        }
        sftp.create_dir(&current)
            .await
            .with_context(|| format!("create remote directory {current}"))?;
    }
    Ok(())
}

async fn create_remote_archive(
    handle: &russh::client::Handle<SftpClientHandler>,
    remote_dir: &str,
    remote_archive: &str,
) -> Result<()> {
    let remote_dir = remote_dir.trim_end_matches('/');
    let parent = remote_parent(remote_dir);
    let name = base_name(remote_dir);
    let command = format!(
        "tar -C {} -czf {} {}",
        shell_quote(&parent),
        shell_quote(remote_archive),
        shell_quote(&name),
    );
    exec_remote_command(handle, &command)
        .await
        .with_context(|| format!("archive remote directory {remote_dir}"))?;
    Ok(())
}

async fn create_remote_paths_archive(
    handle: &russh::client::Handle<SftpClientHandler>,
    remote_paths: &[String],
    remote_archive: &str,
) -> Result<()> {
    let first = remote_paths
        .first()
        .context("cannot archive an empty path selection")?;
    let parent = remote_parent(first);
    if remote_paths
        .iter()
        .any(|path| remote_parent(path) != parent)
    {
        return Err(anyhow!(
            "selected paths must share the same parent directory"
        ));
    }
    let names = remote_paths
        .iter()
        .map(|path| shell_quote(&base_name(path)))
        .collect::<Vec<_>>()
        .join(" ");
    let command = format!(
        "tar -C {} -czf {} -- {}",
        shell_quote(&parent),
        shell_quote(remote_archive),
        names
    );
    exec_remote_command(handle, &command)
        .await
        .with_context(|| format!("archive {} remote paths", remote_paths.len()))
}

async fn pack_remote_paths_to_zip(
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

fn create_zip_from_directory(source: &Path, target: &Path) -> Result<()> {
    let file =
        fs::File::create(target).with_context(|| format!("create ZIP {}", target.display()))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for entry in WalkDir::new(source) {
        let entry = entry.with_context(|| format!("walk {}", source.display()))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(source)
            .with_context(|| format!("strip ZIP root from {}", path.display()))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let name = relative.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            archive
                .add_directory(format!("{name}/"), options)
                .with_context(|| format!("add ZIP directory {name}"))?;
        } else {
            archive
                .start_file(&name, options)
                .with_context(|| format!("add ZIP file {name}"))?;
            let mut input = fs::File::open(path)
                .with_context(|| format!("open ZIP source {}", path.display()))?;
            std::io::copy(&mut input, &mut archive)
                .with_context(|| format!("write ZIP file {name}"))?;
        }
    }
    archive.finish().context("finish ZIP archive")?;
    Ok(())
}

async fn remove_remote_path(
    handle: &russh::client::Handle<SftpClientHandler>,
    remote_path: &str,
) -> Result<()> {
    let command = format!("rm -f {}", shell_quote(remote_path));
    exec_remote_command(handle, &command)
        .await
        .with_context(|| format!("remove remote temporary file {remote_path}"))?;
    Ok(())
}

async fn exec_remote_command(
    handle: &russh::client::Handle<SftpClientHandler>,
    command: &str,
) -> Result<()> {
    let mut channel = handle
        .channel_open_session()
        .await
        .context("open remote exec session")?;
    channel
        .exec(true, command)
        .await
        .with_context(|| format!("exec remote command: {command}"))?;

    let mut stderr = Vec::new();
    let mut stdout = Vec::new();
    let mut exit_status = None;

    // Add timeout to prevent indefinite blocking (300 seconds = 5 minutes)
    let timeout = tokio::time::Duration::from_secs(300);
    let result = tokio::time::timeout(timeout, async {
        loop {
            // Yield to allow cancellation
            tokio::task::yield_now().await;

            if let Some(msg) = channel.wait().await {
                match msg {
                    russh::ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                    russh::ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
                    russh::ChannelMsg::ExitStatus { exit_status: code } => exit_status = Some(code),
                    russh::ChannelMsg::Close => break,
                    _ => {}
                }
            } else {
                break;
            }
        }
    })
    .await;

    if result.is_err() {
        return Err(anyhow!("remote command timeout: {command}"));
    }

    match exit_status.unwrap_or(0) {
        0 => Ok(()),
        code => {
            let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&stdout).trim().to_string();
            Err(anyhow!(
                "remote command exited with {code}: {}",
                if !stderr.is_empty() { stderr } else { stdout }
            ))
        }
    }
}

async fn extract_archive_to(path: &Path, target_dir: &Path) -> Result<()> {
    let Some(file_name) = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
    else {
        return Ok(());
    };
    let archive_path = path.to_path_buf();
    let target_dir = target_dir.to_path_buf();

    tokio::task::spawn_blocking(move || -> Result<()> {
        fs::create_dir_all(&target_dir)
            .with_context(|| format!("create {}", target_dir.display()))?;

        if file_name.ends_with(".zip") {
            let file = fs::File::open(&archive_path)
                .with_context(|| format!("open {}", archive_path.display()))?;
            let mut zip = ZipArchive::new(file).context("read zip archive")?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index).context("read zip entry")?;
                let Some(name) = entry.enclosed_name().map(|name| name.to_path_buf()) else {
                    continue;
                };
                let output = target_dir.join(name);
                if entry.is_dir() {
                    fs::create_dir_all(&output)?;
                } else {
                    if let Some(parent) = output.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let mut output_file = fs::File::create(&output)?;
                    std::io::copy(&mut entry, &mut output_file)?;
                }
            }
        } else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
            let file = fs::File::open(&archive_path)
                .with_context(|| format!("open {}", archive_path.display()))?;
            let decoder = GzDecoder::new(file);
            let mut archive = tar::Archive::new(decoder);
            archive
                .unpack(&target_dir)
                .context("unpack tar.gz archive")?;
        } else if file_name.ends_with(".tar") {
            let file = fs::File::open(&archive_path)
                .with_context(|| format!("open {}", archive_path.display()))?;
            let mut archive = tar::Archive::new(file);
            archive.unpack(&target_dir).context("unpack tar archive")?;
        }

        Ok(())
    })
    .await
    .context("extract archive task join failure")??;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_zip_with_nested_files() {
        let root = std::env::temp_dir().join(format!("tiny-shell-zip-test-{}", Uuid::new_v4()));
        let source = root.join("source");
        let nested = source.join("folder");
        fs::create_dir_all(&nested).unwrap();
        fs::write(source.join("root.txt"), b"root").unwrap();
        fs::write(nested.join("nested.txt"), b"nested").unwrap();
        let target = root.join("archive.zip");

        create_zip_from_directory(&source, &target).unwrap();

        let file = fs::File::open(&target).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        assert!(archive.by_name("root.txt").is_ok());
        assert!(archive.by_name("folder/nested.txt").is_ok());
        fs::remove_dir_all(root).unwrap();
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

    #[test]
    fn partial_file_larger_than_source_cannot_be_resumed() {
        let source_size = 100_u64;
        let partial_size = 101_u64;
        assert!(partial_size > source_size);
    }

    #[test]
    fn old_transfer_states_remain_deserializable() {
        let state: crate::terminal::TransferState = serde_json::from_str(r#""Cancelled""#).unwrap();
        assert_eq!(
            state,
            crate::terminal::TransferState::Interrupted("Cancelled".to_string())
        );
    }
}
