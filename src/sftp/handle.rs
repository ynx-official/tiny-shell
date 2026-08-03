use tokio::sync::mpsc::{self, Sender};

use super::SftpCommand;
use crate::sftp::text_file::RemoteTextSave;

pub struct SftpHandle {
    commands: Sender<SftpCommand>,
}

impl Clone for SftpHandle {
    fn clone(&self) -> Self {
        Self {
            commands: self.commands.clone(),
        }
    }
}

impl SftpHandle {
    pub(crate) fn new(commands: Sender<SftpCommand>) -> Self {
        Self { commands }
    }

    pub(crate) fn send_command(&self, command: SftpCommand) -> bool {
        match self.commands.try_send(command) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!("sftp command queue is full; dropping command");
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::debug!("sftp command dropped because its receiver is closed");
                false
            }
        }
    }

    pub fn list_dir(&self, path: String) {
        self.send_command(SftpCommand::ListDir(path));
    }

    pub fn list_directory_tree(&self, path: String) {
        self.send_command(SftpCommand::ListDirectoryTree(path));
    }

    pub fn measure_latency(&self) {
        self.send_command(SftpCommand::MeasureLatency);
    }

    #[allow(dead_code)]
    pub fn preview(&self, path: String) {
        self.send_command(SftpCommand::Preview(path));
    }

    pub fn download(&self, remote: String, local_dir: String) {
        self.send_command(SftpCommand::Download { remote, local_dir });
    }

    pub fn upload_paths(&self, locals: Vec<String>, remote_dir: String) {
        self.send_command(SftpCommand::UploadPaths { locals, remote_dir });
    }

    pub fn edit_file(&self, remote_path: String) {
        self.send_command(SftpCommand::EditFile {
            remote_path,
            editor: None,
        });
    }

    pub fn edit_file_with(&self, remote_path: String, editor: String) {
        self.send_command(SftpCommand::EditFile {
            remote_path,
            editor: Some(editor),
        });
    }

    /// 下载文件内容到内存,供内置编辑器使用。
    pub fn download_file_content(&self, remote_path: String) {
        self.send_command(SftpCommand::DownloadFileContent { remote_path });
    }

    pub fn save_file_content(&self, save: RemoteTextSave) {
        self.send_command(SftpCommand::SaveFileContent(save));
    }

    pub fn close(&self) {
        self.send_command(SftpCommand::Close);
    }

    pub fn pause_transfer(&self, id: String) {
        self.send_command(SftpCommand::PauseTransfer(id));
    }

    pub fn resume_transfer(&self, id: String) {
        self.send_command(SftpCommand::ResumeTransfer(id));
    }

    pub fn cancel_transfer(&self, id: String) {
        self.send_command(SftpCommand::CancelTransfer(id));
    }
}
