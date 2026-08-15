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
        let (lock, signal) = &*self.state;
        let guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (guard, _) = signal
            .wait_timeout_while(guard, std::time::Duration::from_secs(60), |value| {
                value.is_none()
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    /// BGRA pixels owned by the frame. Keeping this allocation behind an
    /// `Arc` allows the compositor to hold a frame while the decoder reuses
    /// its own working buffer for the next callback.
    pub(crate) pixels: Arc<[u8]>,
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
            pixels: pixels.into(),
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct FrameMailbox {
    latest: Mutex<Option<DecodedFrame>>,
    published: AtomicU64,
    replaced: AtomicU64,
    consumed: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FrameMailboxStats {
    pub(crate) published: u64,
    pub(crate) replaced: u64,
    pub(crate) consumed: u64,
}

pub(crate) struct RemoteDesktopRequest {
    pub(crate) tab_id: String,
    pub(crate) session: Session,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) generation: u64,
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
/// The native adapter is kept behind this boundary. Builds without the
/// `freerdp` feature report a clear status while retaining the same lifecycle
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
    let stop_requested = Arc::new(AtomicBool::new(false));
    let task_tab_id = tab_id.clone();
    let mailbox = Arc::new(FrameMailbox::default());
    #[cfg(not(feature = "freerdp"))]
    let _ = (width, height);
    #[cfg(feature = "freerdp")]
    let worker_mailbox = Arc::clone(&mailbox);
    #[cfg(feature = "freerdp")]
    {
        let worker_tab_id = task_tab_id.clone();
        let worker_events = events.clone();
        let worker_stop_requested = Arc::clone(&stop_requested);
        let blocking_runtime = runtime.clone();
        let join = blocking_runtime.spawn_blocking(move || {
            crate::backend::freerdp::run(
                worker_tab_id.clone(),
                session,
                width,
                height,
                worker_events.clone(),
                worker_mailbox,
                worker_stop_requested,
                command_rx,
            )
        });
        runtime.spawn(async move {
            let reason = match join.await {
                Ok(Ok(())) => "RDP connection closed".to_string(),
                Ok(Err(error)) => format!("{error:#}"),
                Err(error) => format!("RDP worker stopped: {error}"),
            };
            let _ = events.send(BackendEvent::Closed {
                tab_id: task_tab_id,
                reason,
            });
        });
    }

    #[cfg(not(feature = "freerdp"))]
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
            stop_requested,
        },
        mailbox,
    )
}

impl FrameMailbox {
    pub(crate) fn publish(&self, frame: DecodedFrame) {
        self.published.fetch_add(1, Ordering::Relaxed);
        let mut latest = self
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if latest.replace(frame).is_some() {
            self.replaced.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn take_latest(&self) -> Option<DecodedFrame> {
        let frame = self
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if frame.is_some() {
            self.consumed.fetch_add(1, Ordering::Relaxed);
        }
        frame
    }

    pub(crate) fn stats(&self) -> FrameMailboxStats {
        FrameMailboxStats {
            published: self.published.load(Ordering::Relaxed),
            replaced: self.replaced.load(Ordering::Relaxed),
            consumed: self.consumed.load(Ordering::Relaxed),
        }
    }

    /// Read metadata without removing the frame. The UI uses this to expose
    /// connection/decoder health while a future texture renderer consumes the
    /// owned pixel buffer.
    pub(crate) fn latest_metadata(&self) -> Option<(u64, FrameSize)> {
        self.latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|frame| (frame.sequence, frame.size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(sequence: u64) -> DecodedFrame {
        let size = FrameSize::new(2, 1, 8).unwrap();
        DecodedFrame::new(sequence, size, vec![0; 8]).unwrap()
    }

    #[test]
    fn mailbox_keeps_only_the_latest_frame() {
        let mailbox = FrameMailbox::default();
        mailbox.publish(frame(1));
        mailbox.publish(frame(2));

        assert_eq!(mailbox.take_latest().unwrap().sequence, 2);
        assert!(mailbox.take_latest().is_none());
        assert_eq!(
            mailbox.stats(),
            FrameMailboxStats {
                published: 2,
                replaced: 1,
                consumed: 1,
            }
        );
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
}
