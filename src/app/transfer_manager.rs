use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::{sftp::SftpHandle, terminal::Transfer};

/// 应用层传输编排状态。
///
/// SFTP 任务自身只负责 I/O；跨重启恢复所需的会话绑定和持久化节流集中在这里。
pub(crate) struct TransferManager {
    handles: HashMap<String, SftpHandle>,
    last_persist: Instant,
}

impl TransferManager {
    pub(crate) fn new() -> Self {
        Self {
            handles: HashMap::new(),
            last_persist: Instant::now(),
        }
    }

    pub(crate) fn bind_session(&mut self, session_id: String, handle: SftpHandle) {
        self.handles.insert(session_id, handle);
    }

    pub(crate) fn unbind_session(&mut self, session_id: &str) {
        self.handles.remove(session_id);
    }

    pub(crate) fn handle_for_session(&self, session_id: &str) -> Option<&SftpHandle> {
        self.handles.get(session_id)
    }

    pub(crate) fn handle_for_transfer(&self, transfer: &Transfer) -> Option<&SftpHandle> {
        self.handle_for_session(&transfer.info.session_id)
    }

    pub(crate) fn should_persist(&mut self, force: bool) -> bool {
        if force || self.last_persist.elapsed() >= Duration::from_secs(2) {
            self.last_persist = Instant::now();
            true
        } else {
            false
        }
    }

    pub(crate) fn is_resumable(transfer: &Transfer) -> bool {
        transfer.info.resumable
            && !transfer.info.session_id.is_empty()
            && transfer.info.partial_path.is_some()
            && transfer.info.source_size.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::TransferManager;

    #[test]
    fn persistence_is_throttled_until_forced() {
        let mut manager = TransferManager::new();
        assert!(manager.should_persist(true));
        assert!(!manager.should_persist(false));
    }
}
