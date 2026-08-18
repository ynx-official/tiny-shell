//! Protocol-independent primitives shared by the FreeRDP backend and UI.
//!
//! FreeRDP callbacks can arrive faster than the GPUI compositor presents
//! frames. A bounded latest-frame mailbox is deliberately used here instead
//! of the terminal event queue: dropping an old desktop frame reduces latency
//! while preserving the newest view of the remote machine.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use tokio::sync::{mpsc, watch};

use crate::{
    session::config::Session,
    terminal::{BackendCommand, BackendEvent, BackendEventSender, BackendTx},
};

pub(crate) const DEFAULT_REMOTE_DESKTOP_WIDTH: u32 = 1280;
pub(crate) const DEFAULT_REMOTE_DESKTOP_HEIGHT: u32 = 720;

static TRUSTED_CERTIFICATES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CertificateDecisionKind {
    TrustOnce,
    TrustAlways,
    Reject,
}

pub(crate) fn is_certificate_trusted(host: &str, port: u16, fingerprint: &str) -> bool {
    TRUSTED_CERTIFICATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&format!("{host}:{port}"))
        .is_some_and(|known| known == fingerprint)
}

pub(crate) fn remember_certificate(host: &str, port: u16, fingerprint: &str) {
    TRUSTED_CERTIFICATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(format!("{host}:{port}"), fingerprint.to_owned());
}

/// A certificate decision shared between the FreeRDP worker and the UI.
///
/// The native verification callback runs on the RDP thread, so it must wait
/// without touching GPUI. A timeout is deliberately fail-closed.
#[derive(Clone, Debug)]
pub(crate) struct CertificateDecision {
    state: Arc<(Mutex<Option<CertificateDecisionKind>>, Condvar)>,
}

impl CertificateDecision {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new((Mutex::new(None), Condvar::new())),
        }
    }

    pub(crate) fn accept(&self) {
        self.set(Some(CertificateDecisionKind::TrustOnce));
    }

    pub(crate) fn accept_always(&self) {
        self.set(Some(CertificateDecisionKind::TrustAlways));
    }

    pub(crate) fn reject(&self) {
        self.set(Some(CertificateDecisionKind::Reject));
    }

    pub(crate) fn wait(&self) -> CertificateDecisionKind {
        self.wait_until_cancelled(&AtomicBool::new(false))
    }

    pub(crate) fn wait_until_cancelled(&self, cancelled: &AtomicBool) -> CertificateDecisionKind {
        let (lock, signal) = &*self.state;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while guard.is_none() && !cancelled.load(Ordering::Acquire) {
            let now = std::time::Instant::now();
            if now >= deadline {
                break;
            }
            let timeout = (deadline - now).min(std::time::Duration::from_millis(100));
            (guard, _) = signal
                .wait_timeout(guard, timeout)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        (*guard).unwrap_or(CertificateDecisionKind::Reject)
    }

    fn set(&self, value: Option<CertificateDecisionKind>) {
        let (lock, signal) = &*self.state;
        let mut guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_none() {
            *guard = value;
            signal.notify_all();
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CertificateRequest {
    pub(crate) tab_id: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) common_name: String,
    pub(crate) subject: String,
    pub(crate) issuer: String,
    pub(crate) fingerprint: String,
    pub(crate) previous_subject: Option<String>,
    pub(crate) previous_issuer: Option<String>,
    pub(crate) previous_fingerprint: Option<String>,
    pub(crate) decision: CertificateDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteDesktopState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Closing,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) stride: u32,
}

impl FrameSize {
    pub(crate) fn new(width: u32, height: u32, stride: u32) -> Result<Self, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::InvalidSize);
        }
        let minimum_stride = width.checked_mul(4).ok_or(FrameError::InvalidSize)?;
        if stride < minimum_stride {
            return Err(FrameError::InvalidStride);
        }
        Ok(Self {
            width,
            height,
            stride,
        })
    }

    fn required_bytes(self) -> Result<usize, FrameError> {
        (self.stride as usize)
            .checked_mul(self.height as usize)
            .ok_or(FrameError::InvalidSize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameError {
    InvalidSize,
    InvalidStride,
    IncompletePixels { expected: usize, actual: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodedFrame {
    pub(crate) sequence: u64,
    pub(crate) size: FrameSize,
    /// Tightly or natively-strided BGRA pixels owned by the mailbox entry.
    /// Ownership moves into the renderer so a tightly packed frame does not
    /// require a second full-frame copy.
    pub(crate) pixels: Vec<u8>,
}

impl DecodedFrame {
    pub(crate) fn new(sequence: u64, size: FrameSize, pixels: Vec<u8>) -> Result<Self, FrameError> {
        let expected = size.required_bytes()?;
        if pixels.len() < expected {
            return Err(FrameError::IncompletePixels {
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self {
            sequence,
            size,
            pixels,
        })
    }
}

#[derive(Debug, Default)]
struct FrameMailboxSlot {
    latest: Option<DecodedFrame>,
    wake_pending: bool,
}

#[derive(Debug, Default)]
pub(crate) struct FrameMailbox {
    slot: Mutex<FrameMailboxSlot>,
    published: AtomicU64,
    replaced: AtomicU64,
    consumed: AtomicU64,
    published_rate: Mutex<FrameRateCounter>,
    consumed_rate: Mutex<FrameRateCounter>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FrameMailboxStats {
    pub(crate) published: u64,
    pub(crate) replaced: u64,
    pub(crate) consumed: u64,
    pub(crate) published_fps: u16,
    pub(crate) consumed_fps: u16,
}

#[derive(Debug)]
struct FrameRateCounter {
    started: std::time::Instant,
    count: u32,
    fps: u16,
}

impl Default for FrameRateCounter {
    fn default() -> Self {
        Self {
            started: std::time::Instant::now(),
            count: 0,
            fps: 0,
        }
    }
}

impl FrameRateCounter {
    fn record(&mut self) {
        self.count = self.count.saturating_add(1);
        if self.count < 30 {
            return;
        }
        let elapsed = self.started.elapsed();
        if !elapsed.is_zero() {
            self.fps = (f64::from(self.count) / elapsed.as_secs_f64())
                .round()
                .clamp(0.0, f64::from(u16::MAX)) as u16;
        }
        self.started = std::time::Instant::now();
        self.count = 0;
    }
}

pub(crate) struct RemoteDesktopRequest {
    pub(crate) tab_id: String,
    pub(crate) session: Session,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) generation: u64,
}

#[cfg(tiny_shell_freerdp_backend)]
pub(crate) struct RemoteDesktopExit {
    pub(crate) reason: String,
    pub(crate) retryable: bool,
}

impl RemoteDesktopRequest {
    pub(crate) fn new(
        tab_id: String,
        session: Session,
        width: u32,
        height: u32,
        generation: u64,
    ) -> Self {
        Self {
            tab_id,
            session,
            width,
            height,
            generation,
        }
    }
}

/// Start the protocol worker boundary used by the optional FreeRDP adapter.
///
/// The native adapter is kept behind this boundary. Builds where FreeRDP was
/// not discovered report a clear status while retaining the same lifecycle
/// and cancellation contract used by SSH.
pub(crate) fn spawn_remote_desktop_terminal(
    runtime: &tokio::runtime::Handle,
    request: RemoteDesktopRequest,
    events: BackendEventSender,
) -> (BackendTx, Arc<FrameMailbox>) {
    let RemoteDesktopRequest {
        tab_id,
        session,
        width,
        height,
        generation,
    } = request;
    let events = events.with_generation(generation);
    let (commands, command_rx) =
        mpsc::channel::<BackendCommand>(crate::terminal::BACKEND_COMMAND_QUEUE_CAPACITY);
    let (close, _close_rx) = watch::channel(false);
    let (resize, _resize_rx) = watch::channel(None);
    let mouse_move = Arc::new(Mutex::new(None));
    let stop_requested = Arc::new(AtomicBool::new(false));
    let task_tab_id = tab_id.clone();
    let mailbox = Arc::new(FrameMailbox::default());
    #[cfg(not(tiny_shell_freerdp_backend))]
    let _ = (width, height);
    #[cfg(tiny_shell_freerdp_backend)]
    let worker_mailbox = Arc::clone(&mailbox);
    #[cfg(tiny_shell_freerdp_backend)]
    {
        let worker_tab_id = task_tab_id.clone();
        let worker_events = events.clone();
        let worker_stop_requested = Arc::clone(&stop_requested);
        let blocking_runtime = runtime.clone();
        let join = blocking_runtime.spawn_blocking(move || {
            crate::backend::freerdp::run(crate::backend::freerdp::RunRequest {
                tab_id: worker_tab_id.clone(),
                session,
                width,
                height,
                events: worker_events.clone(),
                mailbox: worker_mailbox,
                stop_requested: worker_stop_requested,
                command_rx,
                mouse_move: Arc::clone(&mouse_move),
            })
        });
        runtime.spawn(async move {
            let exit = match join.await {
                Ok(exit) => exit,
                Err(error) => RemoteDesktopExit {
                    reason: format!("RDP worker stopped: {error}"),
                    retryable: true,
                },
            };
            let _ = events.send(BackendEvent::RemoteDesktopClosed {
                tab_id: task_tab_id,
                reason: exit.reason,
                retryable: exit.retryable,
            });
        });
    }

    #[cfg(not(tiny_shell_freerdp_backend))]
    runtime.spawn(async move {
        let mut command_rx = command_rx;
        let mut close_rx = _close_rx;
        let mut resize_rx = _resize_rx;
        let reason = rust_i18n::t!("rdp_backend_not_linked").to_string();
        let _ = events.send(BackendEvent::Status {
            tab_id: task_tab_id.clone(),
            text: format!("{}: {}", reason, session.host),
        });
        let _ = events.send(BackendEvent::Closed {
            tab_id: task_tab_id,
            reason,
        });

        loop {
            tokio::select! {
                changed = close_rx.changed() => {
                    if changed.is_err() || *close_rx.borrow() {
                        break;
                    }
                }
                changed = resize_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                command = command_rx.recv() => {
                    if matches!(command, Some(BackendCommand::Close) | None) {
                        break;
                    }
                }
            }
        }
    });
    (
        BackendTx::Rdp {
            commands,
            close,
            resize,
            mouse_move,
            stop_requested,
        },
        mailbox,
    )
}

impl FrameMailbox {
    /// Publishes a frame and returns whether the UI needs a wake-up event.
    pub(crate) fn publish(&self, frame: DecodedFrame) -> bool {
        self.published.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut rate) = self.published_rate.lock() {
            rate.record();
        }
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let needs_wakeup = !slot.wake_pending;
        slot.wake_pending = true;
        if slot.latest.replace(frame).is_some() {
            self.replaced.fetch_add(1, Ordering::Relaxed);
        }
        needs_wakeup
    }

    /// Re-arms notification delivery when the UI event cannot be delivered.
    /// The latest frame stays available so the next frame can retry the
    /// lightweight notification without copying it again.
    pub(crate) fn wakeup_failed(&self) {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .wake_pending = false;
    }

    pub(crate) fn take_latest(&self) -> Option<DecodedFrame> {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let frame = slot.latest.take();
        if frame.is_some() {
            slot.wake_pending = false;
            self.consumed.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut rate) = self.consumed_rate.lock() {
                rate.record();
            }
        }
        frame
    }

    pub(crate) fn stats(&self) -> FrameMailboxStats {
        FrameMailboxStats {
            published: self.published.load(Ordering::Relaxed),
            replaced: self.replaced.load(Ordering::Relaxed),
            consumed: self.consumed.load(Ordering::Relaxed),
            published_fps: self
                .published_rate
                .lock()
                .map(|rate| rate.fps)
                .unwrap_or_default(),
            consumed_fps: self
                .consumed_rate
                .lock()
                .map(|rate| rate.fps)
                .unwrap_or_default(),
        }
    }

    /// Read metadata without removing the frame. The UI uses this to expose
    /// connection/decoder health while a future texture renderer consumes the
    /// owned pixel buffer.
    pub(crate) fn latest_metadata(&self) -> Option<(u64, FrameSize)> {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .latest
            .as_ref()
            .map(|frame| (frame.sequence, frame.size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(tiny_shell_freerdp_backend))]
    use crate::terminal::backend_event_channel;

    fn frame(sequence: u64) -> DecodedFrame {
        let size = FrameSize::new(2, 1, 8).unwrap();
        DecodedFrame::new(sequence, size, vec![0; 8]).unwrap()
    }

    #[test]
    fn mailbox_keeps_only_the_latest_frame() {
        let mailbox = FrameMailbox::default();
        assert!(mailbox.publish(frame(1)));
        assert!(!mailbox.publish(frame(2)));

        assert_eq!(mailbox.take_latest().unwrap().sequence, 2);
        assert!(mailbox.take_latest().is_none());
        assert_eq!(
            mailbox.stats(),
            FrameMailboxStats {
                published: 2,
                replaced: 1,
                consumed: 1,
                published_fps: 0,
                consumed_fps: 0,
            }
        );
    }

    #[cfg(not(tiny_shell_freerdp_backend))]
    #[test]
    fn no_backend_build_reports_status_and_closes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (events, mut receiver) = backend_event_channel();
        let session = Session::rdp(
            "rdp.example.test".to_string(),
            3389,
            "tester".to_string(),
            "secret".to_string(),
        );
        let request = RemoteDesktopRequest::new(
            "rdp-fallback".to_string(),
            session.clone(),
            DEFAULT_REMOTE_DESKTOP_WIDTH,
            DEFAULT_REMOTE_DESKTOP_HEIGHT,
            7,
        );

        let (_backend, _mailbox) = spawn_remote_desktop_terminal(runtime.handle(), request, events);
        runtime.block_on(async { tokio::task::yield_now().await });

        let reason = rust_i18n::t!("rdp_backend_not_linked").to_string();
        assert!(matches!(
            receiver.try_recv().unwrap().event,
            BackendEvent::Status { tab_id, text }
                if tab_id == "rdp-fallback"
                    && text == format!("{reason}: {}", session.host)
        ));
        assert!(matches!(
            receiver.try_recv().unwrap().event,
            BackendEvent::Closed { tab_id, reason: closed_reason }
                if tab_id == "rdp-fallback" && closed_reason == reason
        ));
    }

    #[test]
    fn mailbox_retries_wakeup_after_event_delivery_fails() {
        let mailbox = FrameMailbox::default();
        assert!(mailbox.publish(frame(1)));

        mailbox.wakeup_failed();

        assert!(mailbox.publish(frame(2)));
        assert_eq!(mailbox.take_latest().unwrap().sequence, 2);
    }

    #[test]
    fn frame_validation_rejects_invalid_pixel_buffers() {
        let size = FrameSize::new(2, 2, 8).unwrap();
        assert_eq!(
            DecodedFrame::new(1, size, vec![0; 7]),
            Err(FrameError::IncompletePixels {
                expected: 16,
                actual: 7,
            })
        );
        assert_eq!(FrameSize::new(0, 1, 4), Err(FrameError::InvalidSize));
        assert_eq!(FrameSize::new(2, 1, 4), Err(FrameError::InvalidStride));
    }

    #[test]
    fn certificate_decision_unblocks_and_accepts_once() {
        let decision = CertificateDecision::new();
        let waiter = decision.clone();
        let thread = std::thread::spawn(move || waiter.wait());
        decision.accept();
        assert_eq!(
            thread.join().unwrap_or(CertificateDecisionKind::Reject),
            CertificateDecisionKind::TrustOnce
        );
        decision.reject();
        assert_eq!(decision.wait(), CertificateDecisionKind::TrustOnce);
    }

    #[test]
    fn certificate_decision_stops_waiting_when_connection_is_cancelled() {
        let decision = CertificateDecision::new();
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            decision.wait_until_cancelled(&cancelled),
            CertificateDecisionKind::Reject
        );
    }
}
