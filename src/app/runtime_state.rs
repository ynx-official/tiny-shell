use gpui::{AnyWindowHandle, SharedString};

use crate::app::{SyncSecretsPasswordDialogState, updater};

#[derive(Default)]
struct TaskGeneration(u64);

impl TaskGeneration {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(1);
        self.0
    }

    fn is_current(&self, generation: u64) -> bool {
        self.0 == generation
    }
}

#[derive(Default)]
pub(crate) struct ManagedWindowState {
    pub(crate) handle: Option<AnyWindowHandle>,
    pub(crate) opening: bool,
}

#[derive(Default)]
pub(crate) struct AuxiliaryWindowsState {
    pub(crate) settings: ManagedWindowState,
    pub(crate) connection_manager: ManagedWindowState,
}

#[derive(Default)]
pub(crate) struct ConfigPersistenceState {
    dirty: bool,
}

impl ConfigPersistenceState {
    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(crate) fn clear(&mut self) {
        self.dirty = false;
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }
}

#[derive(Default)]
pub(crate) struct UpdateRuntimeState {
    pub(crate) status: Option<updater::UpdateStatus>,
    download_cancellation: Option<updater::DownloadCancellation>,
    download_generation: TaskGeneration,
    schedule_generation: TaskGeneration,
}

impl UpdateRuntimeState {
    pub(crate) fn start_schedule(&mut self) -> u64 {
        self.schedule_generation.next()
    }

    pub(crate) fn is_current_schedule(&self, generation: u64) -> bool {
        self.schedule_generation.is_current(generation)
    }

    pub(crate) fn start_download(&mut self, cancellation: updater::DownloadCancellation) -> u64 {
        self.download_cancellation = Some(cancellation);
        self.download_generation.next()
    }

    pub(crate) fn is_current_download(&self, generation: u64) -> bool {
        self.download_generation.is_current(generation)
    }

    pub(crate) fn finish_download(&mut self, generation: u64) -> bool {
        if !self.is_current_download(generation) {
            return false;
        }
        self.download_cancellation = None;
        true
    }

    pub(crate) fn cancel_download(&mut self) {
        if let Some(cancellation) = self.download_cancellation.take() {
            cancellation.cancel();
        }
        self.download_generation.next();
    }
}

pub(crate) struct SyncRuntimeState {
    pub(crate) in_progress: bool,
    pub(crate) status: SharedString,
    pub(crate) secrets_password_dialog: Option<SyncSecretsPasswordDialogState>,
    schedule_generation: TaskGeneration,
}

impl SyncRuntimeState {
    pub(crate) fn new(status: SharedString) -> Self {
        Self {
            in_progress: false,
            status,
            secrets_password_dialog: None,
            schedule_generation: TaskGeneration::default(),
        }
    }

    pub(crate) fn start_schedule(&mut self) -> u64 {
        self.schedule_generation.next()
    }

    pub(crate) fn is_current_schedule(&self, generation: u64) -> bool {
        self.schedule_generation.is_current(generation)
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigPersistenceState, TaskGeneration};

    #[test]
    fn task_generation_invalidates_previous_work() {
        let mut generation = TaskGeneration::default();
        let first = generation.next();
        assert!(generation.is_current(first));

        let second = generation.next();
        assert!(!generation.is_current(first));
        assert!(generation.is_current(second));
    }

    #[test]
    fn config_persistence_tracks_pending_changes() {
        let mut state = ConfigPersistenceState::default();
        assert!(!state.is_dirty());

        state.mark_dirty();
        assert!(state.is_dirty());

        state.clear();
        assert!(!state.is_dirty());
    }
}
