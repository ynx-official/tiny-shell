use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::terminal::{
    BackendEvent, BackendEventEnvelope, BackendEventReceiver, BackendEventSender,
    backend_event_channel,
};

pub(crate) type WindowOwnerId = u64;

pub(crate) struct SessionQueueStats {
    pub(crate) routed: u64,
    pub(crate) deferred: u64,
    pub(crate) peak_pending: usize,
    pub(crate) pending: usize,
    pub(crate) last_routed: usize,
    pub(crate) last_drained: usize,
    pub(crate) last_drain_micros: u64,
    pub(crate) sent: u64,
    pub(crate) rejected: u64,
}

fn saturating_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

pub(crate) struct SessionStore {
    event_routes: HashMap<String, WindowOwnerId>,
    pending_events: HashMap<WindowOwnerId, VecDeque<BackendEventEnvelope>>,
    unrouted_events: HashMap<String, VecDeque<BackendEventEnvelope>>,
    events_tx: BackendEventSender,
    events_rx: BackendEventReceiver,
    deferred_event: Option<BackendEventEnvelope>,
    routed_events: AtomicU64,
    deferred_events: AtomicU64,
    peak_pending_events: AtomicU64,
    last_routed_events: AtomicU64,
    last_drained_events: AtomicU64,
    last_drain_micros: AtomicU64,
}

impl SessionStore {
    pub(crate) fn new() -> Self {
        let (events_tx, events_rx) = backend_event_channel();
        Self {
            event_routes: HashMap::new(),
            pending_events: HashMap::new(),
            unrouted_events: HashMap::new(),
            events_tx,
            events_rx,
            deferred_event: None,
            routed_events: AtomicU64::new(0),
            deferred_events: AtomicU64::new(0),
            peak_pending_events: AtomicU64::new(0),
            last_routed_events: AtomicU64::new(0),
            last_drained_events: AtomicU64::new(0),
            last_drain_micros: AtomicU64::new(0),
        }
    }

    pub(crate) fn events_sender(&self) -> BackendEventSender {
        self.events_tx.clone()
    }

    pub(crate) fn register_event_route(&mut self, route_id: String, owner_id: WindowOwnerId) {
        self.event_routes.insert(route_id.clone(), owner_id);
        if let Some(events) = self.unrouted_events.remove(&route_id) {
            self.pending_events
                .entry(owner_id)
                .or_default()
                .extend(events);
        }
    }

    pub(crate) fn unregister_event_route(
        &mut self,
        route_id: &str,
        owner_id: WindowOwnerId,
    ) -> bool {
        if self.event_routes.get(route_id).copied() != Some(owner_id) {
            return false;
        }
        self.event_routes.remove(route_id);
        self.unrouted_events.remove(route_id);
        for pending in self.pending_events.values_mut() {
            pending.retain(|event| event_route_id(event) != Some(route_id));
        }
        self.pending_events.retain(|_, events| !events.is_empty());
        if self
            .deferred_event
            .as_ref()
            .is_some_and(|event| event_route_id(event) == Some(route_id))
        {
            self.deferred_event = None;
        }
        true
    }

    pub(crate) fn move_event_routes(
        &mut self,
        route_ids: &[String],
        source_id: WindowOwnerId,
        target_id: WindowOwnerId,
    ) -> bool {
        if route_ids
            .iter()
            .any(|route_id| self.event_routes.get(route_id).copied() != Some(source_id))
        {
            return false;
        }
        for route_id in route_ids {
            self.event_routes.insert(route_id.clone(), target_id);
        }

        let route_ids = route_ids.iter().map(String::as_str).collect::<HashSet<_>>();
        if let Some(mut source_events) = self.pending_events.remove(&source_id) {
            let mut retained = VecDeque::new();
            let mut moved = VecDeque::new();
            while let Some(event) = source_events.pop_front() {
                if event_route_id(&event).is_some_and(|route_id| route_ids.contains(route_id)) {
                    moved.push_back(event);
                } else {
                    retained.push_back(event);
                }
            }
            if !retained.is_empty() {
                self.pending_events.insert(source_id, retained);
            }
            if !moved.is_empty() {
                self.pending_events
                    .entry(target_id)
                    .or_default()
                    .extend(moved);
            }
        }
        true
    }

    pub(crate) fn queue_stats(&self) -> SessionQueueStats {
        let pending = self
            .pending_events
            .values()
            .map(VecDeque::len)
            .sum::<usize>()
            + self
                .unrouted_events
                .values()
                .map(VecDeque::len)
                .sum::<usize>()
            + usize::from(self.deferred_event.is_some());
        let send_stats = self.events_tx.stats();
        SessionQueueStats {
            routed: self.routed_events.load(Ordering::Relaxed),
            deferred: self.deferred_events.load(Ordering::Relaxed),
            peak_pending: saturating_usize(
                self.peak_pending_events
                    .load(Ordering::Relaxed)
                    .max(saturating_u64(pending)),
            ),
            pending,
            last_routed: saturating_usize(self.last_routed_events.load(Ordering::Relaxed)),
            last_drained: saturating_usize(self.last_drained_events.load(Ordering::Relaxed)),
            last_drain_micros: self.last_drain_micros.load(Ordering::Relaxed),
            sent: send_stats.sent,
            rejected: send_stats.rejected,
        }
    }

    pub(crate) fn drain_events_for(
        &mut self,
        owner_id: WindowOwnerId,
        limit: usize,
    ) -> Vec<BackendEventEnvelope> {
        // Keep routing work bounded as well as UI dispatch work. A busy SSH
        // session must not monopolize the window that is currently draining
        // events, especially when another window owns the next event batch.
        const MAX_ROUTED_EVENTS_PER_TICK: usize = 2_048;
        const MAX_PENDING_EVENTS_PER_OWNER: usize = 8_192;
        let started = Instant::now();
        let mut routed = 0;
        while routed < MAX_ROUTED_EVENTS_PER_TICK {
            let event = if let Some(event) = self.deferred_event.take() {
                event
            } else {
                let Ok(event) = self.events_rx.try_recv() else {
                    break;
                };
                event
            };
            routed += 1;
            let Some(route_id) = event_route_id(&event) else {
                continue;
            };
            if let Some(owner_id) = self.event_routes.get(route_id).copied() {
                let pending = self.pending_events.entry(owner_id).or_default();
                if pending.len() >= MAX_PENDING_EVENTS_PER_OWNER {
                    self.deferred_events.fetch_add(1, Ordering::Relaxed);
                    self.deferred_event = Some(event);
                    break;
                }
                pending.push_back(event);
                self.routed_events.fetch_add(1, Ordering::Relaxed);
            } else {
                let pending = self
                    .unrouted_events
                    .entry(route_id.to_string())
                    .or_default();
                if pending.len() >= MAX_PENDING_EVENTS_PER_OWNER {
                    self.deferred_events.fetch_add(1, Ordering::Relaxed);
                    self.deferred_event = Some(event);
                    break;
                }
                pending.push_back(event);
                self.routed_events.fetch_add(1, Ordering::Relaxed);
            }
        }
        let pending_total = self
            .pending_events
            .values()
            .map(VecDeque::len)
            .sum::<usize>()
            + self
                .unrouted_events
                .values()
                .map(VecDeque::len)
                .sum::<usize>()
            + usize::from(self.deferred_event.is_some());
        self.peak_pending_events
            .fetch_max(saturating_u64(pending_total), Ordering::Relaxed);
        let pending = self.pending_events.entry(owner_id).or_default();
        let count = limit.min(pending.len());
        let drained = pending.drain(..count).collect();
        if pending.is_empty() {
            self.pending_events.remove(&owner_id);
        }
        self.last_routed_events
            .store(saturating_u64(routed), Ordering::Relaxed);
        self.last_drained_events
            .store(saturating_u64(count), Ordering::Relaxed);
        self.last_drain_micros.store(
            u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        drained
    }
}

fn event_route_id(event: &BackendEventEnvelope) -> Option<&str> {
    match &event.event {
        BackendEvent::Output { tab_id, .. }
        | BackendEvent::Status { tab_id, .. }
        | BackendEvent::Connected { tab_id }
        | BackendEvent::SftpEntries { tab_id, .. }
        | BackendEvent::SftpDirectoryEntries { tab_id, .. }
        | BackendEvent::SftpStatus { tab_id, .. }
        | BackendEvent::SftpLatency { tab_id, .. }
        | BackendEvent::SftpFileContent { tab_id, .. }
        | BackendEvent::SftpContentUploaded { tab_id, .. }
        | BackendEvent::SftpContentConflict { tab_id, .. }
        | BackendEvent::SftpContentUploadFailed { tab_id, .. }
        | BackendEvent::RemoteSystem { tab_id, .. }
        | BackendEvent::RemoteSystemUnavailable { tab_id, .. }
        | BackendEvent::SftpHome { tab_id, .. }
        | BackendEvent::TransferProgress { tab_id, .. }
        | BackendEvent::TransferStarted { tab_id, .. }
        | BackendEvent::Closed { tab_id, .. }
        | BackendEvent::TerminalTitleChanged { tab_id, .. } => Some(tab_id),
        BackendEvent::SyncFinished { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::SessionStore;
    use crate::terminal::BackendEvent;

    #[test]
    fn queued_event_follows_moved_route() {
        let mut store = SessionStore::new();
        store.register_event_route("session-a".to_string(), 1);
        store.move_event_routes(&["session-a".to_string()], 1, 2);
        store
            .events_sender()
            .send(BackendEvent::Status {
                tab_id: "session-a".to_string(),
                text: "connected".to_string(),
            })
            .unwrap();

        assert!(store.drain_events_for(1, usize::MAX).is_empty());
        assert_eq!(store.drain_events_for(2, usize::MAX).len(), 1);
    }

    #[test]
    fn already_queued_event_moves_with_its_route() {
        let mut store = SessionStore::new();
        store.register_event_route("session-a".to_string(), 1);
        store
            .events_sender()
            .send(BackendEvent::Status {
                tab_id: "session-a".to_string(),
                text: "connected".to_string(),
            })
            .unwrap();

        assert!(store.drain_events_for(99, usize::MAX).is_empty());
        assert!(store.move_event_routes(&["session-a".to_string()], 1, 2));
        assert!(store.drain_events_for(1, usize::MAX).is_empty());
        assert_eq!(store.drain_events_for(2, usize::MAX).len(), 1);
    }

    #[test]
    fn event_sent_before_registration_is_not_lost() {
        let mut store = SessionStore::new();
        store
            .events_sender()
            .send(BackendEvent::Status {
                tab_id: "session-a".to_string(),
                text: "connecting".to_string(),
            })
            .unwrap();

        assert!(store.drain_events_for(1, usize::MAX).is_empty());
        store.register_event_route("session-a".to_string(), 1);
        assert_eq!(store.drain_events_for(1, usize::MAX).len(), 1);
    }

    #[test]
    fn unregister_route_drops_only_its_queued_events() {
        let mut store = SessionStore::new();
        store.register_event_route("session-a".to_string(), 1);
        store.register_event_route("session-b".to_string(), 1);
        store
            .events_sender()
            .send(BackendEvent::Status {
                tab_id: "session-a".to_string(),
                text: "stale".to_string(),
            })
            .unwrap();
        store
            .events_sender()
            .send(BackendEvent::Status {
                tab_id: "session-b".to_string(),
                text: "keep".to_string(),
            })
            .unwrap();
        assert!(store.drain_events_for(99, usize::MAX).is_empty());
        assert!(store.unregister_event_route("session-a", 1));
        let events = store.drain_events_for(1, usize::MAX);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0].event, BackendEvent::Status { tab_id, .. } if tab_id == "session-b")
        );
    }

    #[test]
    fn failed_move_keeps_existing_route() {
        let mut store = SessionStore::new();
        store.register_event_route("session-a".to_string(), 1);

        assert!(!store.move_event_routes(&["session-a".to_string()], 3, 2));
        assert_eq!(store.drain_events_for(2, usize::MAX).len(), 0);
    }

    #[test]
    fn routing_work_is_bounded_per_tick() {
        let mut store = SessionStore::new();
        store.register_event_route("session-a".to_string(), 1);
        store.register_event_route("session-b".to_string(), 2);
        for index in 0..3_000 {
            store
                .events_sender()
                .send(BackendEvent::Status {
                    tab_id: if index == 0 {
                        "session-a".to_string()
                    } else {
                        "session-b".to_string()
                    },
                    text: "queued".to_string(),
                })
                .unwrap();
        }

        assert_eq!(store.drain_events_for(1, usize::MAX).len(), 1);
        assert_eq!(store.drain_events_for(2, 2_047).len(), 2_047);
        assert_eq!(store.drain_events_for(2, usize::MAX).len(), 952);
        let stats = store.queue_stats();
        assert_eq!(stats.routed, 3_000);
        assert_eq!(stats.deferred, 0);
        assert!(stats.peak_pending >= 2_048);
    }
}
