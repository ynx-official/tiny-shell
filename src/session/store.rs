use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::terminal::{
    BackendEvent, BackendEventEnvelope, BackendEventReceiver, BackendEventSender,
    backend_event_channel,
};

pub(crate) type WindowOwnerId = u64;

const MAX_ROUTED_EVENTS_PER_TICK: usize = 2_048;
const MAX_PENDING_EVENTS_PER_OWNER: usize = 8_192;
const MAX_PENDING_OUTPUT_BYTES_PER_OWNER: usize = 8 * 1024 * 1024;
const MAX_UNROUTED_ROUTES: usize = 256;
const MAX_UNROUTED_EVENTS: usize = 4_096;
const MAX_UNROUTED_EVENTS_PER_ROUTE: usize = 256;
const MAX_UNROUTED_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_UNROUTED_OUTPUT_BYTES_PER_ROUTE: usize = 512 * 1024;
const UNROUTED_EVENT_TTL: Duration = Duration::from_secs(30);
const MAX_ROUTE_TOMBSTONES: usize = 4_096;
const MAX_COALESCED_PENDING_OUTPUT_BYTES: usize = 128 * 1024;

#[derive(Default)]
struct EventQueue {
    events: VecDeque<BackendEventEnvelope>,
    output_bytes: usize,
}

impl EventQueue {
    fn len(&self) -> usize {
        self.events.len()
    }

    fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    fn push_bounded(
        &mut self,
        event: BackendEventEnvelope,
        max_events: usize,
        max_output_bytes: usize,
    ) -> Result<(), BackendEventEnvelope> {
        let incoming_output_bytes = output_bytes(&event);
        let would_exceed_output_limit = incoming_output_bytes > 0
            && self.output_bytes.saturating_add(incoming_output_bytes) > max_output_bytes;
        if would_exceed_output_limit && !event.event.is_reliable_lifecycle() {
            return Err(event);
        }
        if self.try_coalesce_output(&event) {
            self.output_bytes += incoming_output_bytes;
            return Ok(());
        }

        let reliable = event.event.is_reliable_lifecycle();
        let would_exceed_event_limit = self.events.len() >= max_events;

        // Keep lifecycle events ahead of ordinary data when a queue is full,
        // but never let the queue itself grow without limit. This also makes
        // route restoration and cross-window moves obey the same bounds as
        // direct routing.
        if would_exceed_event_limit {
            if !reliable {
                return Err(event);
            }
            let Some(index) = self
                .events
                .iter()
                .position(|queued| !queued.event.is_reliable_lifecycle())
            else {
                return Err(event);
            };
            if let Some(dropped) = self.events.remove(index) {
                self.output_bytes = self.output_bytes.saturating_sub(output_bytes(&dropped));
            }
        }

        self.output_bytes += incoming_output_bytes;
        self.events.push_back(event);
        Ok(())
    }

    fn push_preserving(&mut self, event: BackendEventEnvelope) {
        let incoming_output_bytes = output_bytes(&event);
        if !self.try_coalesce_output(&event) {
            self.events.push_back(event);
        }
        self.output_bytes += incoming_output_bytes;
    }

    fn try_coalesce_output(&mut self, incoming: &BackendEventEnvelope) -> bool {
        let BackendEvent::Output {
            tab_id: incoming_tab,
            bytes: incoming_bytes,
        } = &incoming.event
        else {
            return false;
        };
        let Some(previous) = self.events.back_mut() else {
            return false;
        };
        if previous.generation != incoming.generation
            || previous.sequence.checked_add(1) != Some(incoming.sequence)
        {
            return false;
        }
        let BackendEvent::Output { tab_id, bytes } = &mut previous.event else {
            return false;
        };
        if tab_id != incoming_tab
            || bytes.len().saturating_add(incoming_bytes.len()) > MAX_COALESCED_PENDING_OUTPUT_BYTES
        {
            return false;
        }
        bytes.extend_from_slice(incoming_bytes);
        previous.sequence = incoming.sequence;
        true
    }

    fn pop_front(&mut self) -> Option<BackendEventEnvelope> {
        let event = self.events.pop_front()?;
        self.output_bytes = self.output_bytes.saturating_sub(output_bytes(&event));
        Some(event)
    }

    fn drain(&mut self, limit: usize) -> Vec<BackendEventEnvelope> {
        let count = limit.min(self.events.len());
        let mut drained = Vec::with_capacity(count);
        for _ in 0..count {
            if let Some(event) = self.pop_front() {
                drained.push(event);
            }
        }
        drained
    }

    fn retain(&mut self, mut keep: impl FnMut(&BackendEventEnvelope) -> bool) {
        let mut retained = Self::default();
        while let Some(event) = self.pop_front() {
            if keep(&event) {
                retained.push_preserving(event);
            }
        }
        *self = retained;
    }
}

struct UnroutedEventQueue {
    pending: EventQueue,
    last_seen: Instant,
}

fn output_bytes(event: &BackendEventEnvelope) -> usize {
    match &event.event {
        BackendEvent::Output { bytes, .. } => bytes.len(),
        _ => 0,
    }
}

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
    pending_events: HashMap<WindowOwnerId, EventQueue>,
    unrouted_events: HashMap<String, UnroutedEventQueue>,
    route_tombstones: HashMap<String, Instant>,
    events_tx: BackendEventSender,
    events_rx: BackendEventReceiver,
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
            route_tombstones: HashMap::new(),
            events_tx,
            events_rx,
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
        let now = Instant::now();
        self.prune_route_lifecycle(now);
        self.route_tombstones.remove(&route_id);
        self.event_routes.insert(route_id.clone(), owner_id);
        if let Some(mut unrouted) = self.unrouted_events.remove(&route_id) {
            let pending = self.pending_events.entry(owner_id).or_default();
            while let Some(event) = unrouted.pending.pop_front() {
                if pending
                    .push_bounded(
                        event,
                        MAX_PENDING_EVENTS_PER_OWNER,
                        MAX_PENDING_OUTPUT_BYTES_PER_OWNER,
                    )
                    .is_err()
                {
                    self.deferred_events.fetch_add(1, Ordering::Relaxed);
                }
            }
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
        self.route_tombstones
            .insert(route_id.to_string(), Instant::now());
        self.trim_route_tombstones();
        for pending in self.pending_events.values_mut() {
            pending.retain(|event| event_route_id(event) != Some(route_id));
        }
        self.pending_events.retain(|_, events| !events.is_empty());
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
            let mut retained = EventQueue::default();
            let mut moved = EventQueue::default();
            while let Some(event) = source_events.pop_front() {
                if event_route_id(&event).is_some_and(|route_id| route_ids.contains(route_id)) {
                    moved.push_preserving(event);
                } else {
                    retained.push_preserving(event);
                }
            }
            if !retained.is_empty() {
                self.pending_events.insert(source_id, retained);
            }
            if !moved.is_empty() {
                let target = self.pending_events.entry(target_id).or_default();
                while let Some(event) = moved.pop_front() {
                    if target
                        .push_bounded(
                            event,
                            MAX_PENDING_EVENTS_PER_OWNER,
                            MAX_PENDING_OUTPUT_BYTES_PER_OWNER,
                        )
                        .is_err()
                    {
                        self.deferred_events.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        true
    }

    pub(crate) fn queue_stats(&self) -> SessionQueueStats {
        let pending = self
            .pending_events
            .values()
            .map(EventQueue::len)
            .sum::<usize>()
            + self
                .unrouted_events
                .values()
                .map(|events| events.pending.len())
                .sum::<usize>();
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
        let started = Instant::now();
        self.prune_route_lifecycle(started);
        let mut routed = 0;
        while routed < MAX_ROUTED_EVENTS_PER_TICK {
            let Ok(event) = self.events_rx.try_recv() else {
                break;
            };
            routed += 1;
            let Some(route_id) = event_route_id(&event).map(str::to_owned) else {
                continue;
            };
            if let Some(owner_id) = self.event_routes.get(&route_id).copied() {
                let pending = self.pending_events.entry(owner_id).or_default();
                if pending
                    .push_bounded(
                        event,
                        MAX_PENDING_EVENTS_PER_OWNER,
                        MAX_PENDING_OUTPUT_BYTES_PER_OWNER,
                    )
                    .is_err()
                {
                    self.deferred_events.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                self.routed_events.fetch_add(1, Ordering::Relaxed);
            } else if self.route_tombstones.contains_key(&route_id) {
                // A backend may race with tab teardown. Tombstoned routes are
                // terminal: late events must not be resurrected as unrouted
                // work that can attach to a future window.
                self.deferred_events.fetch_add(1, Ordering::Relaxed);
            } else {
                if !self.enqueue_unrouted(route_id, event, started) {
                    self.deferred_events.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                self.routed_events.fetch_add(1, Ordering::Relaxed);
            }
        }
        let pending_total = self
            .pending_events
            .values()
            .map(EventQueue::len)
            .sum::<usize>()
            + self
                .unrouted_events
                .values()
                .map(|events| events.pending.len())
                .sum::<usize>();
        self.peak_pending_events
            .fetch_max(saturating_u64(pending_total), Ordering::Relaxed);
        let drained = self
            .pending_events
            .get_mut(&owner_id)
            .map_or_else(Vec::new, |pending| pending.drain(limit));
        if self
            .pending_events
            .get(&owner_id)
            .is_some_and(EventQueue::is_empty)
        {
            self.pending_events.remove(&owner_id);
        }
        let count = drained.len();
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

    fn enqueue_unrouted(
        &mut self,
        route_id: String,
        event: BackendEventEnvelope,
        now: Instant,
    ) -> bool {
        if !self.unrouted_events.contains_key(&route_id)
            && self.unrouted_events.len() >= MAX_UNROUTED_ROUTES
        {
            if let Some(dropped) = self.evict_oldest_unrouted_route() {
                self.deferred_events
                    .fetch_add(saturating_u64(dropped), Ordering::Relaxed);
            }
        }

        let incoming_output_bytes = output_bytes(&event);
        while self.unrouted_event_count() >= MAX_UNROUTED_EVENTS
            || (incoming_output_bytes > 0
                && self
                    .unrouted_output_bytes()
                    .saturating_add(incoming_output_bytes)
                    > MAX_UNROUTED_OUTPUT_BYTES)
        {
            let Some(dropped) = self.evict_oldest_unrouted_route() else {
                return false;
            };
            self.deferred_events
                .fetch_add(saturating_u64(dropped), Ordering::Relaxed);
        }

        let unrouted = self
            .unrouted_events
            .entry(route_id)
            .or_insert_with(|| UnroutedEventQueue {
                pending: EventQueue::default(),
                last_seen: now,
            });
        unrouted.last_seen = now;
        unrouted
            .pending
            .push_bounded(
                event,
                MAX_UNROUTED_EVENTS_PER_ROUTE,
                MAX_UNROUTED_OUTPUT_BYTES_PER_ROUTE,
            )
            .is_ok()
    }

    fn prune_route_lifecycle(&mut self, now: Instant) {
        self.unrouted_events.retain(|_, events| {
            now.saturating_duration_since(events.last_seen) <= UNROUTED_EVENT_TTL
        });
        self.trim_route_tombstones();
    }

    fn trim_route_tombstones(&mut self) {
        while self.route_tombstones.len() > MAX_ROUTE_TOMBSTONES {
            let Some(oldest) = self
                .route_tombstones
                .iter()
                .min_by_key(|(_, removed_at)| **removed_at)
                .map(|(route_id, _)| route_id.clone())
            else {
                break;
            };
            self.route_tombstones.remove(&oldest);
        }
    }

    fn evict_oldest_unrouted_route(&mut self) -> Option<usize> {
        let oldest = self
            .unrouted_events
            .iter()
            .min_by_key(|(_, events)| events.last_seen)
            .map(|(route_id, _)| route_id.clone())?;
        self.unrouted_events
            .remove(&oldest)
            .map(|events| events.pending.len())
    }

    fn unrouted_event_count(&self) -> usize {
        self.unrouted_events
            .values()
            .map(|events| events.pending.len())
            .sum()
    }

    fn unrouted_output_bytes(&self) -> usize {
        self.unrouted_events
            .values()
            .map(|events| events.pending.output_bytes)
            .sum()
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
        | BackendEvent::DockerResult { tab_id, .. }
        | BackendEvent::SftpHome { tab_id, .. }
        | BackendEvent::TransferProgress { tab_id, .. }
        | BackendEvent::TransferStarted { tab_id, .. }
        | BackendEvent::Closed { tab_id, .. }
        | BackendEvent::RemoteDesktopClipboard { tab_id, .. }
        | BackendEvent::RemoteDesktopClosed { tab_id, .. }
        | BackendEvent::TerminalTitleChanged { tab_id, .. }
        | BackendEvent::RemoteDesktopFrameReady { tab_id, .. } => Some(tab_id),
        BackendEvent::RemoteDesktopCertificateRequest(request) => Some(&request.tab_id),
        BackendEvent::SyncFinished { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{EventQueue, SessionStore};
    use crate::terminal::{BackendEvent, BackendEventEnvelope};

    fn envelope(event: BackendEvent, sequence: u64) -> BackendEventEnvelope {
        BackendEventEnvelope {
            event,
            generation: 1,
            sequence,
        }
    }

    #[test]
    fn frame_wakeup_evicts_ordinary_event_from_full_owner_queue() {
        let mut queue = EventQueue::default();
        queue
            .push_bounded(
                envelope(
                    BackendEvent::Status {
                        tab_id: "rdp-a".to_string(),
                        text: "waiting".to_string(),
                    },
                    1,
                ),
                1,
                usize::MAX,
            )
            .unwrap();

        queue
            .push_bounded(
                envelope(
                    BackendEvent::RemoteDesktopFrameReady {
                        tab_id: "rdp-a".to_string(),
                        sequence: 1,
                    },
                    2,
                ),
                1,
                usize::MAX,
            )
            .unwrap();

        let queued = queue.pop_front().unwrap();
        assert!(matches!(
            queued.event,
            BackendEvent::RemoteDesktopFrameReady { .. }
        ));
        assert!(queue.is_empty());
    }

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
    fn batch_move_is_atomic_when_one_route_changed_owner() {
        let mut store = SessionStore::new();
        store.register_event_route("session-a".to_string(), 1);
        store.register_event_route("session-b".to_string(), 2);

        assert!(!store.move_event_routes(
            &["session-a".to_string(), "session-b".to_string()],
            1,
            3,
        ));
        for tab_id in ["session-a", "session-b"] {
            assert!(
                store
                    .events_sender()
                    .send(BackendEvent::Status {
                        tab_id: tab_id.to_string(),
                        text: "still routed".to_string(),
                    })
                    .is_ok()
            );
        }

        assert_eq!(store.drain_events_for(1, usize::MAX).len(), 1);
        assert_eq!(store.drain_events_for(2, usize::MAX).len(), 1);
        assert!(store.drain_events_for(3, usize::MAX).is_empty());
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
