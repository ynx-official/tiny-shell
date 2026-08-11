use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use gpui::{AnyWindowHandle, SharedString};

use crate::app::{SyncSecretsPasswordDialogState, config_persistence::SaveReceipt, updater};
use crate::terminal::{BackendEventReceiver, BackendEventSender};

#[derive(Default)]
struct TaskGeneration(u64);

#[derive(Clone, Debug)]
pub(crate) struct TaskCancellation {
    cancelled: Arc<AtomicBool>,
    id: u64,
}

impl TaskCancellation {
    fn new(id: u64) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            id,
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

#[derive(Default)]
pub(crate) struct TaskSupervisor {
    tasks: HashMap<String, TaskCancellation>,
    next_id: u64,
}

pub(crate) struct AsyncRuntimeState {
    pub(crate) supervisor: TaskSupervisor,
    pub(crate) events_rx: BackendEventReceiver,
    pub(crate) events_tx: BackendEventSender,
}

impl AsyncRuntimeState {
    pub(crate) fn new(events_tx: BackendEventSender, events_rx: BackendEventReceiver) -> Self {
        Self {
            supervisor: TaskSupervisor::default(),
            events_rx,
            events_tx,
        }
    }
}

impl TaskSupervisor {
    pub(crate) fn start(&mut self, name: impl Into<String>) -> TaskCancellation {
        let name = name.into();
        self.next_id = self.next_id.wrapping_add(1);
        let task = TaskCancellation::new(self.next_id);
        if let Some(previous) = self.tasks.insert(name, task.clone()) {
            previous.cancel();
        }
        task
    }

    pub(crate) fn cancel(&mut self, name: &str) -> bool {
        if let Some(task) = self.tasks.remove(name) {
            task.cancel();
            true
        } else {
            false
        }
    }

    pub(crate) fn finish(&mut self, name: &str, id: u64) -> bool {
        if self.tasks.get(name).is_some_and(|task| task.id() == id) {
            self.tasks.remove(name);
            true
        } else {
            false
        }
    }

    pub(crate) fn cancel_all(&mut self) {
        for task in self.tasks.drain().map(|(_, task)| task) {
            task.cancel();
        }
    }
}

impl TaskGeneration {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(1);
        self.0
    }

    fn is_current(&self, generation: u64) -> bool {
        self.0 == generation
    }
}

use std::ops::Not;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DialogToken {
    pub(crate) generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DialogActivation(Option<DialogToken>);

impl DialogActivation {
    pub(crate) fn token(self) -> Option<DialogToken> {
        self.0
    }
}

impl Not for DialogActivation {
    type Output = bool;

    fn not(self) -> Self::Output {
        self.0.is_none()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DialogRequest {
    pub(crate) kind: crate::app::DialogKind,
    pub(crate) token: DialogToken,
}

/// Pure application-level dialog lifecycle state. GPUI opening/closing stays
/// at the UI boundary; this object only decides which request is current.
#[derive(Default)]
pub(crate) struct DialogCoordinator {
    active: Option<DialogRequest>,
    pending: Option<DialogRequest>,
    next_generation: u64,
}

#[allow(dead_code)]
impl DialogCoordinator {
    fn next_token(&mut self) -> DialogToken {
        self.next_generation = self.next_generation.wrapping_add(1);
        DialogToken {
            generation: self.next_generation,
        }
    }

    pub(crate) fn request(&mut self, kind: crate::app::DialogKind) -> DialogToken {
        let request = DialogRequest {
            kind,
            token: self.next_token(),
        };
        // Pending is deliberately a single slot: while an active dialog is
        // closing, the latest request replaces any earlier request.
        self.pending = Some(request);
        request.token
    }

    pub(crate) fn activate(&mut self, token: DialogToken) -> DialogActivation {
        if self.active.is_some() {
            return DialogActivation(None);
        }
        let Some(request) = self.pending.take() else {
            return DialogActivation(None);
        };
        if request.token != token {
            self.pending = Some(request);
            return DialogActivation(None);
        }
        self.active = Some(request);
        DialogActivation(Some(token))
    }

    pub(crate) fn close(&mut self, token: DialogToken) -> bool {
        if self.active.is_some_and(|request| request.token == token) {
            self.active = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn cancel(&mut self, token: DialogToken) -> bool {
        if self.pending.is_some_and(|request| request.token == token) {
            self.pending = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn take_pending(&mut self) -> Option<DialogRequest> {
        self.pending.take()
    }

    pub(crate) fn active(&self) -> Option<DialogRequest> {
        self.active
    }

    pub(crate) fn active_kind(&self) -> Option<crate::app::DialogKind> {
        self.active.map(|request| request.kind)
    }

    pub(crate) fn is_same_active(&self, kind: crate::app::DialogKind) -> bool {
        self.active_kind() == Some(kind)
    }

    pub(crate) fn pending(&self) -> Option<DialogRequest> {
        self.pending
    }

    pub(crate) fn is_current(&self, token: DialogToken) -> bool {
        self.active.is_some_and(|request| request.token == token)
            || self.pending.is_some_and(|request| request.token == token)
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
    generation: u64,
    persisted_generation: u64,
    last_dirty_at: Option<Instant>,
    retry_after: Option<Instant>,
    save_immediately: bool,
    in_flight: Option<PendingPreferenceSave>,
    full_commit_in_flight: bool,
}

struct PendingPreferenceSave {
    generation: u64,
    receipt: SaveReceipt,
}

impl ConfigPersistenceState {
    pub(crate) fn mark_dirty(&mut self, now: Instant) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            // Overflow is practically unreachable, but resetting both counters
            // preserves the generation comparison contract if it occurs.
            self.generation = 1;
            self.persisted_generation = 0;
        }
        self.last_dirty_at = Some(now);
        self.generation
    }

    pub(crate) fn request_immediate_save(&mut self) {
        self.save_immediately = true;
    }

    pub(crate) fn ready_generation(&self, now: Instant, debounce: Duration) -> Option<u64> {
        if !self.is_dirty() || self.in_flight.is_some() {
            return None;
        }
        if self
            .retry_after
            .is_some_and(|retry_after| now < retry_after)
        {
            return None;
        }
        let quiet_long_enough = self
            .last_dirty_at
            .is_some_and(|last_dirty_at| now.saturating_duration_since(last_dirty_at) >= debounce);
        (self.save_immediately || quiet_long_enough).then_some(self.generation)
    }

    pub(crate) fn set_in_flight(&mut self, generation: u64, receipt: SaveReceipt) {
        self.save_immediately = false;
        self.retry_after = None;
        self.in_flight = Some(PendingPreferenceSave {
            generation,
            receipt,
        });
    }

    pub(crate) fn poll_result(&mut self) -> Option<(u64, anyhow::Result<()>)> {
        let result = self.in_flight.as_ref()?.receipt.try_result()?;
        let generation = self.in_flight.take()?.generation;
        Some((generation, result))
    }

    pub(crate) fn mark_saved(&mut self, generation: u64) {
        self.persisted_generation = self.persisted_generation.max(generation);
        self.retry_after = None;
    }

    pub(crate) fn mark_save_failed(&mut self, now: Instant, retry_delay: Duration) {
        self.retry_after = Some(now + retry_delay);
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.persisted_generation < self.generation
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn begin_full_commit(&mut self) -> bool {
        if self.full_commit_in_flight {
            return false;
        }
        self.full_commit_in_flight = true;
        true
    }

    pub(crate) fn finish_full_commit(&mut self) {
        self.full_commit_in_flight = false;
    }

    pub(crate) fn is_full_commit_in_flight(&self) -> bool {
        self.full_commit_in_flight
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
    pub(crate) failed: bool,
    pub(crate) secrets_password_dialog: Option<SyncSecretsPasswordDialogState>,
    schedule_generation: TaskGeneration,
}

impl SyncRuntimeState {
    pub(crate) fn new(status: SharedString) -> Self {
        Self {
            in_progress: false,
            status,
            failed: false,
            secrets_password_dialog: None,
            schedule_generation: TaskGeneration::default(),
        }
    }

    pub(crate) fn set_status(&mut self, status: SharedString, failed: bool) {
        self.status = status;
        self.failed = failed;
    }

    pub(crate) fn set_failed(&mut self, status: SharedString) {
        self.set_status(status, true);
    }

    pub(crate) fn clear_failure(&mut self) {
        self.failed = false;
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
    use super::{ConfigPersistenceState, DialogCoordinator, TaskGeneration, TaskSupervisor};
    use crate::app::DialogKind;
    use std::time::Instant;

    #[test]
    fn dialog_coordinator_reports_same_active_kind_for_normal_open_ignore() {
        let mut coordinator = DialogCoordinator::default();
        let token = coordinator.request(DialogKind::Updater);
        assert_eq!(coordinator.activate(token).token(), Some(token));
        assert!(coordinator.is_same_active(DialogKind::Updater));
        assert!(!coordinator.is_same_active(DialogKind::Transfers));
    }

    #[test]
    fn dialog_coordinator_activates_pending_after_active_closes() {
        let mut coordinator = DialogCoordinator::default();
        let active_token = coordinator.request(DialogKind::Updater);
        assert_eq!(
            coordinator.activate(active_token).token(),
            Some(active_token)
        );

        let pending_token = coordinator.request(DialogKind::Transfers);
        assert!(coordinator.close(active_token));
        let pending = coordinator
            .pending()
            .expect("pending request remains after active closes");
        assert_eq!(pending.token, pending_token);
        assert_eq!(
            coordinator.activate(pending_token).token(),
            Some(pending_token)
        );
        assert_eq!(
            coordinator.active().map(|request| request.kind),
            Some(DialogKind::Transfers)
        );
    }

    #[test]
    fn dialog_coordinator_latest_pending_request_replaces_older_request() {
        let mut coordinator = DialogCoordinator::default();
        let active_token = coordinator.request(DialogKind::Updater);
        coordinator.activate(active_token);
        let older_pending = coordinator.request(DialogKind::Transfers);
        let latest_pending = coordinator.request(DialogKind::SessionSelector);

        assert!(!coordinator.is_current(older_pending));
        assert_eq!(
            coordinator.pending().map(|request| request.token),
            Some(latest_pending)
        );
        assert_eq!(coordinator.activate(older_pending).token(), None);
        assert_eq!(coordinator.activate(latest_pending).token(), None);
        assert!(coordinator.close(active_token));
        assert_eq!(
            coordinator.activate(latest_pending).token(),
            Some(latest_pending)
        );
    }

    #[test]
    fn dialog_coordinator_late_old_close_cannot_clear_new_active() {
        let mut coordinator = DialogCoordinator::default();
        let old_token = coordinator.request(DialogKind::Updater);
        coordinator.activate(old_token);
        let pending_token = coordinator.request(DialogKind::Transfers);
        assert!(coordinator.close(old_token));
        coordinator.activate(pending_token);

        assert!(!coordinator.close(old_token));
        assert_eq!(
            coordinator.active().map(|request| request.token),
            Some(pending_token)
        );
    }

    #[test]
    fn dialog_coordinator_old_async_active_close_is_ignored() {
        let mut coordinator = DialogCoordinator::default();
        let old_token = coordinator.request(DialogKind::Updater);
        coordinator.activate(old_token);
        let new_token = coordinator.request(DialogKind::Transfers);
        coordinator.close(old_token);
        coordinator.activate(new_token);

        assert!(!coordinator.close(old_token));
        assert_eq!(
            coordinator.active().map(|request| request.kind),
            Some(DialogKind::Transfers)
        );
    }

    #[test]
    fn dialog_coordinator_delayed_transfers_request_activates_after_previous_close() {
        let mut coordinator = DialogCoordinator::default();
        let first_token = coordinator.request(DialogKind::Updater);
        coordinator.activate(first_token);
        let transfers_token = coordinator.request(DialogKind::Transfers);

        assert_eq!(coordinator.activate(transfers_token).token(), None);
        assert!(coordinator.close(first_token));
        assert_eq!(
            coordinator.activate(transfers_token).token(),
            Some(transfers_token)
        );
        assert_eq!(
            coordinator.active().map(|request| request.kind),
            Some(DialogKind::Transfers)
        );
    }

    #[test]
    fn dialog_coordinator_repeated_request_and_close_are_idempotent() {
        let mut coordinator = DialogCoordinator::default();
        let token = coordinator.request(DialogKind::Transfers);
        assert_eq!(coordinator.activate(token).token(), Some(token));
        assert!(coordinator.activate(token).token().is_none());
        assert!(coordinator.close(token));
        assert!(!coordinator.close(token));
        assert!(!coordinator.cancel(token));
    }

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

        state.mark_dirty(Instant::now());
        assert!(state.is_dirty());

        state.mark_saved(state.generation());
        assert!(!state.is_dirty());
    }

    #[test]
    fn starting_same_task_cancels_previous_generation() {
        let mut supervisor = TaskSupervisor::default();
        let first = supervisor.start("sync");
        let second = supervisor.start("sync");

        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
    }

    #[test]
    fn cancelling_named_task_removes_and_stops_it() {
        let mut supervisor = TaskSupervisor::default();
        let task = supervisor.start("sync");

        assert!(supervisor.cancel("sync"));
        assert!(task.is_cancelled());
        assert!(!supervisor.cancel("sync"));
    }

    #[test]
    fn finishing_task_releases_its_name_for_future_work() {
        let mut supervisor = TaskSupervisor::default();
        let first = supervisor.start("sync");

        supervisor.finish("sync", first.id());
        let second = supervisor.start("sync");

        assert!(!first.is_cancelled());
        assert!(!second.is_cancelled());
    }

    #[test]
    fn stale_completion_cannot_remove_replacement_task() {
        let mut supervisor = TaskSupervisor::default();
        let first = supervisor.start("sync");
        let second = supervisor.start("sync");

        assert!(!supervisor.finish("sync", first.id()));
        assert!(!second.is_cancelled());
    }

    #[test]
    fn cancel_all_stops_every_registered_task() {
        let mut supervisor = TaskSupervisor::default();
        let first = supervisor.start("sync");
        let second = supervisor.start("update");

        supervisor.cancel_all();

        assert!(first.is_cancelled());
        assert!(second.is_cancelled());
    }
}
