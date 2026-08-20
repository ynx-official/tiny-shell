use std::{collections::VecDeque, sync::Mutex};

use tokio::sync::Notify;

use super::{PermissionApplyTarget, text_file::RemoteTextSave};

pub(super) const COMMAND_QUEUE_CAPACITY: usize = 256;
const CONTROL_QUEUE_CAPACITY: usize = 1_024;

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
#[derive(Debug, PartialEq, Eq)]
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
    pub(super) fn new() -> Self {
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

#[cfg(test)]
mod tests {
    use super::{CONTROL_QUEUE_CAPACITY, SftpControl, SftpControlQueue};

    #[tokio::test]
    async fn latest_pause_or_resume_for_a_transfer_wins() {
        let queue = SftpControlQueue::new();
        assert!(queue.send(SftpControl::PauseTransfer("transfer".to_string())));
        assert!(queue.send(SftpControl::ResumeTransfer("transfer".to_string())));

        assert_eq!(
            queue.recv().await,
            SftpControl::ResumeTransfer("transfer".to_string())
        );
    }

    #[tokio::test]
    async fn high_priority_control_evicts_a_coalescible_entry_when_full() {
        let queue = SftpControlQueue::new();
        for index in 0..CONTROL_QUEUE_CAPACITY {
            assert!(queue.send(SftpControl::PauseTransfer(format!("transfer-{index}"))));
        }

        assert!(queue.send(SftpControl::Close));
        let mut saw_close = false;
        for _ in 0..CONTROL_QUEUE_CAPACITY {
            saw_close |= queue.recv().await == SftpControl::Close;
        }
        assert!(saw_close);
    }
}
