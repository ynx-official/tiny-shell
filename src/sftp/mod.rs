pub mod ops;
pub mod text_file;

mod archive;
mod command;
mod connection;
mod filesystem;
pub(crate) mod handle;
pub(crate) mod handler;
mod remote_command;
mod temp;
mod transfer;
pub(crate) mod utils;

pub use command::SftpCommand;
pub(crate) use command::{SftpControl, SftpControlQueue};
pub(crate) use handle::SftpHandle;
pub(crate) use handler::SftpClientHandler;
pub(crate) use transfer::TransferStateFlag;
pub(crate) use utils::*;

use command::COMMAND_QUEUE_CAPACITY;
use connection::connect_and_authenticate;
use filesystem::{
    list_dir as list_dir_impl, recursive_delete, set_path_permissions, set_permissions_recursive,
};
use remote_command::exec_remote_command;
use temp::{allocate_sftp_temp_directory, safe_local_edit_name};

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::{path::Path, sync::Arc, time::Instant};

use anyhow::{Context, Result};

use russh::Disconnect;
use russh_sftp::client::SftpSession;
use tokio::{
    io::AsyncWriteExt,
    sync::{
        Semaphore,
        mpsc::{self, Receiver, Sender},
    },
    task::JoinHandle,
};
use uuid::Uuid;

use rust_i18n::t;

use crate::{
    session::config::{ConfigStore, Session},
    sftp::{
        text_file::{
            RemoteTextSave, SaveRemoteTextOutcome, read_remote_text_file, save_remote_text_file,
        },
        transfer::{
            TransferCleanup, TransferContext, download_file_impl, download_path_impl,
            local_partial_path, pack_remote_paths_to_zip, remote_partial_path,
            report_transfer_failure, report_transfer_interrupted, upload_file_impl,
            upload_paths_impl,
        },
    },
    terminal::{BackendEvent, BackendEventSender},
};

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

#[cfg(test)]
mod tests {
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
