//! Protocol-independent primitives shared by the FreeRDP backend and UI.
//!
//! FreeRDP callbacks can arrive faster than the GPUI compositor presents
//! frames. A bounded latest-frame mailbox is deliberately used here instead
//! of the terminal event queue: dropping an old desktop frame reduces latency
//! while preserving the newest view of the remote machine.

#![allow(dead_code)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use image::{Frame, ImageBuffer, Rgba};
use tokio::sync::{mpsc, watch};

use crate::{
    session::config::Session,
    terminal::{BackendCommand, BackendEvent, BackendEventSender, BackendTx},
};

pub(crate) const DEFAULT_REMOTE_DESKTOP_WIDTH: u32 = 1280;
pub(crate) const DEFAULT_REMOTE_DESKTOP_HEIGHT: u32 = 720;

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

    /// Convert the native BGRA frame into the format consumed by GPUI.
    ///
    /// FreeRDP may pad every row to a larger stride. `RenderImage` requires a
    /// tightly packed image buffer, so the padding is removed here while the
    /// frame is already on the UI side of the mailbox. Both FreeRDP's GDI
    /// backend and GPUI use BGRA byte order, therefore no channel swap is
    /// needed.
    pub(crate) fn into_render_image(self) -> Result<Arc<gpui::RenderImage>, FrameError> {
        let row_bytes = self
            .size
            .width
            .checked_mul(4)
            .ok_or(FrameError::InvalidSize)? as usize;
        let height = self.size.height as usize;
        let stride = self.size.stride as usize;
        let expected = row_bytes
            .checked_mul(height)
            .ok_or(FrameError::InvalidSize)?;
        let mut tightly_packed = Vec::with_capacity(expected);
        for row in self.pixels.chunks_exact(stride).take(height) {
            tightly_packed.extend_from_slice(&row[..row_bytes]);
        }
        if tightly_packed.len() != expected {
            return Err(FrameError::IncompletePixels {
                expected,
                actual: tightly_packed.len(),
            });
        }
        let buffer = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
            self.size.width,
            self.size.height,
            tightly_packed,
        )
        .ok_or(FrameError::InvalidSize)?;
        let frame = Frame::new(buffer);
        Ok(Arc::new(gpui::RenderImage::new([frame])))
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
    let (commands, _command_rx) =
        mpsc::channel::<BackendCommand>(crate::terminal::BACKEND_COMMAND_QUEUE_CAPACITY);
    let (close, _close_rx) = watch::channel(false);
    let (resize, _resize_rx) = watch::channel(None);
    let stop_requested = Arc::new(AtomicBool::new(false));
    let task_tab_id = tab_id.clone();
    let mailbox = Arc::new(FrameMailbox::default());
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
        let mut command_rx = _command_rx;
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
    /// connection/decoder health while the latest owned pixel buffer is
    /// converted into a persistent GPUI image.
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
    fn render_image_removes_stride_padding_without_swapping_bgra() {
        let size = FrameSize::new(2, 2, 12).unwrap();
        let pixels = vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, // row 1 + padding
            13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, // row 2 + padding
        ];
        let image = DecodedFrame::new(1, size, pixels)
            .unwrap()
            .into_render_image()
            .unwrap();

        assert_eq!(image.size(0).width, 2);
        assert_eq!(image.size(0).height, 2);
        assert_eq!(
            image.as_bytes(0),
            Some(&[1, 2, 3, 4, 5, 6, 7, 8, 13, 14, 15, 16, 17, 18, 19, 20][..])
        );
    }
}
