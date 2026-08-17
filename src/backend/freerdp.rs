//! Rust side of the optional FreeRDP C shim.
//!
//! The shim owns every FreeRDP object and invokes these callbacks from its
//! worker thread. Rust only receives copied BGRA frames, so no FreeRDP pointer
//! is allowed to escape into the UI or outlive the native client.

use std::{
    ffi::{CString, c_char, c_void},
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result, anyhow};
use rust_i18n::t;
use tokio::sync::mpsc;

use super::remote_desktop::{
    CertificateDecision, CertificateDecisionKind, CertificateRequest, DecodedFrame, FrameMailbox,
    FrameSize, RemoteDesktopExit, is_certificate_trusted, remember_certificate,
};
use crate::{
    session::config::Session,
    terminal::{BackendCommand, BackendEvent, BackendEventSender, RemoteDesktopInput},
};

const STATE_CONNECTING: u32 = 1;
const STATE_CONNECTED: u32 = 2;
const STATE_DISCONNECTED: u32 = 3;
const STATE_FAILED: u32 = 4;
const MAX_CLIPBOARD_BYTES: usize = 8 * 1024 * 1024;
const MAX_FRAME_DIAGNOSTIC_SAMPLES: usize = 4096;

#[repr(C)]
struct NativeConfig {
    host: *const c_char,
    port: u16,
    username: *const c_char,
    password: *const c_char,
    domain: *const c_char,
    width: u32,
    height: u32,
}

type StateCallback = unsafe extern "C" fn(*mut c_void, u32, u32, *const c_char);
type FrameCallback = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const u8, usize);
type ShouldStopCallback = unsafe extern "C" fn(*mut c_void) -> i32;
type PollCallback = unsafe extern "C" fn(*mut c_void);
type CertificateCallback = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    u16,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    u32,
) -> u32;
type ChangedCertificateCallback = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    u16,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    u32,
) -> u32;
type ClipboardCallback = unsafe extern "C" fn(*mut c_void, *const u8, usize);

#[repr(C)]
struct NativeCallbacks {
    user_data: *mut c_void,
    on_state: Option<StateCallback>,
    on_frame: Option<FrameCallback>,
    should_stop: Option<ShouldStopCallback>,
    on_poll: Option<PollCallback>,
    on_certificate: Option<CertificateCallback>,
    on_changed_certificate: Option<ChangedCertificateCallback>,
    on_clipboard: Option<ClipboardCallback>,
}

#[repr(C)]
struct NativeClient {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn tiny_shell_rdp_client_new(
        config: *const NativeConfig,
        callbacks: *const NativeCallbacks,
    ) -> *mut NativeClient;
    fn tiny_shell_rdp_client_run(client: *mut NativeClient) -> i32;
    fn tiny_shell_rdp_client_resize(client: *mut NativeClient, width: u32, height: u32) -> i32;
    fn tiny_shell_rdp_client_keyboard(
        client: *mut NativeClient,
        down: i32,
        extended: i32,
        scancode: u32,
    ) -> i32;
    fn tiny_shell_rdp_client_clipboard(
        client: *mut NativeClient,
        text: *const u16,
        length: usize,
    ) -> i32;
    fn tiny_shell_rdp_client_text(
        client: *mut NativeClient,
        text: *const u16,
        length: usize,
    ) -> i32;
    fn tiny_shell_rdp_client_mouse(client: *mut NativeClient, flags: u16, x: u16, y: u16) -> i32;
    fn tiny_shell_rdp_client_stop(client: *mut NativeClient);
    fn tiny_shell_rdp_client_should_retry(client: *mut NativeClient, result: i32) -> i32;
    fn tiny_shell_rdp_client_free(client: *mut NativeClient);
}

struct CallbackBridge {
    tab_id: String,
    events: BackendEventSender,
    mailbox: Arc<FrameMailbox>,
    stop_requested: Arc<AtomicBool>,
    sequence: AtomicU64,
    last_frame_dimensions: AtomicU64,
    last_frame_diagnostic_sequence: AtomicU64,
    command_rx: Mutex<mpsc::Receiver<BackendCommand>>,
    pending_resize: Mutex<Option<(u32, u32, std::time::Instant)>>,
    failure_reason: Mutex<Option<String>>,
    client: std::sync::atomic::AtomicUsize,
}

pub(crate) struct RunRequest {
    pub(crate) tab_id: String,
    pub(crate) session: Session,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) events: BackendEventSender,
    pub(crate) mailbox: Arc<FrameMailbox>,
    pub(crate) stop_requested: Arc<AtomicBool>,
    pub(crate) command_rx: mpsc::Receiver<BackendCommand>,
}

pub(crate) fn run(request: RunRequest) -> RemoteDesktopExit {
    run_inner(request).unwrap_or_else(|error| RemoteDesktopExit {
        reason: format!("{error:#}"),
        retryable: false,
    })
}

fn run_inner(request: RunRequest) -> Result<RemoteDesktopExit> {
    let RunRequest {
        tab_id,
        session,
        width,
        height,
        events,
        mailbox,
        stop_requested,
        command_rx,
    } = request;
    let Session {
        host: host_value,
        port,
        user: user_value,
        password: password_value,
        ..
    } = session;
    let host = CString::new(host_value).context("RDP host contains a NUL byte")?;
    let (domain_value, username_value) = user_value
        .split_once('\\')
        .map_or(("", user_value.as_str()), |(domain, username)| {
            (domain, username)
        });
    let username = CString::new(username_value).context("RDP username contains a NUL byte")?;
    let password = CString::new(password_value).context("RDP password contains a NUL byte")?;
    let domain = CString::new(domain_value).context("RDP domain contains a NUL byte")?;
    let bridge = Box::new(CallbackBridge {
        tab_id,
        events,
        mailbox,
        stop_requested: Arc::clone(&stop_requested),
        sequence: AtomicU64::new(0),
        last_frame_dimensions: AtomicU64::new(0),
        last_frame_diagnostic_sequence: AtomicU64::new(0),
        command_rx: Mutex::new(command_rx),
        pending_resize: Mutex::new(None),
        failure_reason: Mutex::new(None),
        client: std::sync::atomic::AtomicUsize::new(0),
    });
    let bridge_ptr = Box::into_raw(bridge);
    let config = NativeConfig {
        host: host.as_ptr(),
        port,
        username: username.as_ptr(),
        password: password.as_ptr(),
        domain: domain.as_ptr(),
        width,
        height,
    };
    let callbacks = NativeCallbacks {
        user_data: bridge_ptr.cast(),
        on_state: Some(on_state),
        on_frame: Some(on_frame),
        should_stop: Some(should_stop),
        on_poll: Some(on_poll),
        on_certificate: Some(on_certificate),
        on_changed_certificate: Some(on_changed_certificate),
        on_clipboard: Some(on_clipboard),
    };

    // The native client copies all configuration strings during `new`, and
    // does not retain the callback descriptor after the call returns.
    let client = unsafe { tiny_shell_rdp_client_new(&config, &callbacks) };
    if client.is_null() {
        unsafe {
            drop(Box::from_raw(bridge_ptr));
        }
        return Err(anyhow!("FreeRDP client initialization failed"));
    }
    unsafe {
        (*bridge_ptr)
            .client
            .store(client as usize, Ordering::Release);
    }
    let worker_finished = Arc::new(AtomicBool::new(false));
    let result = std::thread::scope(|scope| {
        let monitor_finished = Arc::clone(&worker_finished);
        let monitor_stop = Arc::clone(&stop_requested);
        let client_address = client as usize;
        scope.spawn(move || {
            while !monitor_finished.load(Ordering::Acquire) {
                if monitor_stop.load(Ordering::Acquire) {
                    unsafe { tiny_shell_rdp_client_stop(client_address as *mut NativeClient) };
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        });
        let result = unsafe { tiny_shell_rdp_client_run(client) };
        worker_finished.store(true, Ordering::Release);
        result
    });
    let retryable = unsafe { tiny_shell_rdp_client_should_retry(client, result) != 0 };
    let failure_reason = unsafe {
        (*bridge_ptr)
            .failure_reason
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    };
    unsafe {
        (*bridge_ptr).client.store(0, Ordering::Release);
    }
    unsafe {
        tiny_shell_rdp_client_free(client);
    }
    unsafe {
        drop(Box::from_raw(bridge_ptr));
    }
    let reason = if let Some(reason) = failure_reason {
        reason
    } else if result == 0 {
        "RDP connection closed".to_string()
    } else {
        format!("FreeRDP connection failed with code {result}")
    };
    Ok(RemoteDesktopExit { reason, retryable })
}

unsafe extern "C" fn on_state(
    user_data: *mut c_void,
    state: u32,
    error_code: u32,
    message: *const c_char,
) {
    let Some(bridge) = (unsafe { user_data.cast::<CallbackBridge>().as_ref() }) else {
        return;
    };
    let message = if message.is_null() {
        String::new()
    } else {
        unsafe { std::ffi::CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    };
    match state {
        STATE_CONNECTING => {
            tracing::info!(tab_id = %bridge.tab_id, "RDP connection is starting");
            let _ = bridge.events.send(BackendEvent::Status {
                tab_id: bridge.tab_id.clone(),
                text: t!("rdp_connecting").to_string(),
            });
        }
        STATE_CONNECTED => {
            tracing::info!(tab_id = %bridge.tab_id, "RDP connection established");
            let _ = bridge.events.send(BackendEvent::Connected {
                tab_id: bridge.tab_id.clone(),
            });
        }
        STATE_DISCONNECTED => {
            tracing::info!(tab_id = %bridge.tab_id, "RDP connection disconnected");
            let _ = bridge.events.send(BackendEvent::Status {
                tab_id: bridge.tab_id.clone(),
                text: t!("rdp_connection_closed").to_string(),
            });
        }
        STATE_FAILED => {
            let failure_reason = t!(
                "rdp_connection_failed",
                error_code = error_code,
                message = message
            )
            .to_string();
            *bridge
                .failure_reason
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(failure_reason.clone());
            tracing::warn!(
                tab_id = %bridge.tab_id,
                error_code,
                message = %message,
                "RDP connection failed"
            );
            let _ = bridge.events.send(BackendEvent::Status {
                tab_id: bridge.tab_id.clone(),
                text: failure_reason,
            });
        }
        _ => {}
    }
}

unsafe extern "C" fn on_frame(
    user_data: *mut c_void,
    width: u32,
    height: u32,
    stride: u32,
    pixels: *const u8,
    length: usize,
) {
    let Some(bridge) = (unsafe { user_data.cast::<CallbackBridge>().as_ref() }) else {
        return;
    };
    if pixels.is_null() || length == 0 {
        return;
    }
    let Ok(size) = FrameSize::new(width, height, stride) else {
        return;
    };
    let Some(slice) = (unsafe { ptr::slice_from_raw_parts(pixels, length).as_ref() }) else {
        return;
    };
    let sequence = bridge.sequence.fetch_add(1, Ordering::Relaxed) + 1;
    if sequence == 1 {
        tracing::info!(
            tab_id = %bridge.tab_id,
            width,
            height,
            stride,
            "received first RDP frame"
        );
    }
    let Ok(frame) = DecodedFrame::new(sequence, size, slice.to_vec()) else {
        return;
    };
    let dimensions = (u64::from(width) << 32) | u64::from(height);
    let previous_dimensions = bridge
        .last_frame_dimensions
        .swap(dimensions, Ordering::Relaxed);
    let previous_diagnostic_sequence = bridge
        .last_frame_diagnostic_sequence
        .load(Ordering::Relaxed);
    let dimension_log_is_due = previous_dimensions != dimensions
        && sequence.saturating_sub(previous_diagnostic_sequence) >= 30;
    if sequence <= 3 || sequence % 120 == 0 || dimension_log_is_due {
        bridge
            .last_frame_diagnostic_sequence
            .store(sequence, Ordering::Relaxed);
        let stats = frame_pixel_stats(&frame);
        tracing::info!(
            tab_id = %bridge.tab_id,
            sequence,
            width,
            height,
            stride,
            visible_pixels = stats.visible_pixels,
            sampled_pixels = stats.sampled_pixels,
            rgb_non_white_pixels = stats.rgb_non_white_pixels,
            rgb_min = stats.rgb_min,
            rgb_max = stats.rgb_max,
            alpha_min = stats.alpha_min,
            alpha_max = stats.alpha_max,
            alpha_zero_pixels = stats.alpha_zero_pixels,
            "sampled RDP frame pixels"
        );
    }
    if bridge.mailbox.publish(frame)
        && bridge
            .events
            .send(BackendEvent::RemoteDesktopFrameReady {
                tab_id: bridge.tab_id.clone(),
                sequence,
            })
            .is_err()
    {
        bridge.mailbox.wakeup_failed();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FramePixelStats {
    visible_pixels: u64,
    sampled_pixels: u64,
    rgb_non_white_pixels: u64,
    alpha_zero_pixels: u64,
    rgb_min: u8,
    rgb_max: u8,
    alpha_min: u8,
    alpha_max: u8,
}

fn frame_pixel_stats(frame: &DecodedFrame) -> FramePixelStats {
    let width = frame.size.width as usize;
    let height = frame.size.height as usize;
    let stride = frame.size.stride as usize;
    let visible_pixels = width * height;
    let sample_count = visible_pixels.min(MAX_FRAME_DIAGNOSTIC_SAMPLES);
    let mut stats = FramePixelStats {
        visible_pixels: visible_pixels as u64,
        sampled_pixels: sample_count as u64,
        rgb_non_white_pixels: 0,
        alpha_zero_pixels: 0,
        rgb_min: u8::MAX,
        rgb_max: u8::MIN,
        alpha_min: u8::MAX,
        alpha_max: u8::MIN,
    };
    for sample_index in 0..sample_count {
        let pixel_index =
            ((sample_index as u128 * visible_pixels as u128) / sample_count as u128) as usize;
        let row = pixel_index / width;
        let column = pixel_index % width;
        let offset = row * stride + column * 4;
        let pixel = &frame.pixels[offset..offset + 4];
        if pixel[..3] != [u8::MAX; 3] {
            stats.rgb_non_white_pixels += 1;
        }
        for channel in &pixel[..3] {
            stats.rgb_min = stats.rgb_min.min(*channel);
            stats.rgb_max = stats.rgb_max.max(*channel);
        }
        stats.alpha_min = stats.alpha_min.min(pixel[3]);
        stats.alpha_max = stats.alpha_max.max(pixel[3]);
        if pixel[3] == 0 {
            stats.alpha_zero_pixels += 1;
        }
    }
    stats
}

unsafe extern "C" fn should_stop(user_data: *mut c_void) -> i32 {
    let Some(bridge) = (unsafe { user_data.cast::<CallbackBridge>().as_ref() }) else {
        return 1;
    };
    i32::from(bridge.stop_requested.load(Ordering::Acquire))
}

unsafe extern "C" fn on_poll(user_data: *mut c_void) {
    let Some(bridge) = (unsafe { user_data.cast::<CallbackBridge>().as_ref() }) else {
        return;
    };
    let client = bridge.client.load(Ordering::Acquire) as *mut NativeClient;
    if client.is_null() {
        return;
    }
    let Ok(mut commands) = bridge.command_rx.lock() else {
        return;
    };
    let mut pending_move = None;
    while let Ok(command) = commands.try_recv() {
        if let BackendCommand::RemoteDesktopInput(RemoteDesktopInput::MouseMove { x, y }) = &command
        {
            pending_move = Some((*x, *y));
            continue;
        }
        if let Some((x, y)) = pending_move.take() {
            let success = unsafe { tiny_shell_rdp_client_mouse(client, 0x0800, x, y) != 0 };
            if !success {
                tracing::debug!("FreeRDP rejected a coalesced mouse move");
            }
        }
        let success = match command {
            BackendCommand::RemoteDesktopResize { width, height } => {
                if let Ok(mut pending) = bridge.pending_resize.lock() {
                    *pending = Some((width, height, std::time::Instant::now()));
                }
                true
            }
            BackendCommand::RemoteDesktopInput(input) => match input {
                RemoteDesktopInput::Key {
                    scancode,
                    down,
                    extended,
                } => unsafe {
                    tiny_shell_rdp_client_keyboard(
                        client,
                        i32::from(down),
                        i32::from(extended),
                        scancode,
                    ) != 0
                },
                RemoteDesktopInput::MouseButton { flags, x, y }
                | RemoteDesktopInput::MouseWheel { flags, x, y } => unsafe {
                    tiny_shell_rdp_client_mouse(client, flags, x, y) != 0
                },
                RemoteDesktopInput::MouseMove { .. } => true,
            },
            BackendCommand::RemoteDesktopClipboard(text) => {
                let utf16: Vec<u16> = text.encode_utf16().collect();
                utf16.len() <= MAX_CLIPBOARD_BYTES / size_of::<u16>()
                    && unsafe {
                        tiny_shell_rdp_client_clipboard(client, utf16.as_ptr(), utf16.len()) != 0
                            || tiny_shell_rdp_client_text(client, utf16.as_ptr(), utf16.len()) != 0
                    }
            }
            BackendCommand::Close => {
                bridge.stop_requested.store(true, Ordering::Release);
                true
            }
            _ => true,
        };
        if !success {
            tracing::debug!("FreeRDP rejected an input or resize event");
        }
    }
    if let Some((x, y)) = pending_move {
        let success = unsafe { tiny_shell_rdp_client_mouse(client, 0x0800, x, y) != 0 };
        if !success {
            tracing::debug!("FreeRDP rejected a coalesced mouse move");
        }
    }
    if let Ok(mut pending) = bridge.pending_resize.lock()
        && pending.as_ref().is_some_and(|(_, _, changed)| {
            changed.elapsed() >= std::time::Duration::from_millis(200)
        })
        && let Some((width, height, _)) = pending.take()
    {
        let success = unsafe { tiny_shell_rdp_client_resize(client, width, height) != 0 };
        if !success {
            tracing::debug!(width, height, "FreeRDP rejected a dynamic resize event");
        }
    }
}

unsafe extern "C" fn on_certificate(
    user_data: *mut c_void,
    host: *const c_char,
    port: u16,
    common_name: *const c_char,
    subject: *const c_char,
    issuer: *const c_char,
    fingerprint: *const c_char,
    flags: u32,
) -> u32 {
    handle_certificate(
        user_data,
        host,
        port,
        common_name,
        subject,
        issuer,
        fingerprint,
        None,
        flags,
    )
}

unsafe extern "C" fn on_changed_certificate(
    user_data: *mut c_void,
    host: *const c_char,
    port: u16,
    common_name: *const c_char,
    subject: *const c_char,
    issuer: *const c_char,
    fingerprint: *const c_char,
    old_subject: *const c_char,
    old_issuer: *const c_char,
    old_fingerprint: *const c_char,
    flags: u32,
) -> u32 {
    handle_certificate(
        user_data,
        host,
        port,
        common_name,
        subject,
        issuer,
        fingerprint,
        Some((old_subject, old_issuer, old_fingerprint)),
        flags,
    )
}

#[allow(clippy::too_many_arguments)]
fn handle_certificate(
    user_data: *mut c_void,
    host: *const c_char,
    port: u16,
    common_name: *const c_char,
    subject: *const c_char,
    issuer: *const c_char,
    fingerprint: *const c_char,
    previous: Option<(*const c_char, *const c_char, *const c_char)>,
    _flags: u32,
) -> u32 {
    let Some(bridge) = (unsafe { user_data.cast::<CallbackBridge>().as_ref() }) else {
        return 0;
    };
    let text = |value: *const c_char| {
        if value.is_null() {
            String::new()
        } else {
            unsafe { std::ffi::CStr::from_ptr(value) }
                .to_string_lossy()
                .into_owned()
        }
    };
    let host = text(host);
    let common_name = text(common_name);
    let subject = text(subject);
    let issuer = text(issuer);
    let fingerprint = text(fingerprint);
    let (previous_subject, previous_issuer, previous_fingerprint) =
        previous.map_or((None, None, None), |(subject, issuer, fingerprint)| {
            (
                Some(text(subject)),
                Some(text(issuer)),
                Some(text(fingerprint)),
            )
        });
    if is_certificate_trusted(&host, port, &fingerprint) {
        return 1;
    }
    let decision = CertificateDecision::new();
    if bridge
        .events
        .send(BackendEvent::RemoteDesktopCertificateRequest(Box::new(
            CertificateRequest {
                tab_id: bridge.tab_id.clone(),
                host: host.clone(),
                port,
                common_name,
                subject,
                issuer,
                fingerprint: fingerprint.clone(),
                previous_subject,
                previous_issuer,
                previous_fingerprint,
                decision: decision.clone(),
            },
        )))
        .is_err()
    {
        return 0;
    }
    match decision.wait_until_cancelled(&bridge.stop_requested) {
        CertificateDecisionKind::TrustOnce => 2,
        CertificateDecisionKind::TrustAlways => {
            remember_certificate(&host, port, &fingerprint);
            2
        }
        CertificateDecisionKind::Reject => 0,
    }
}

unsafe extern "C" fn on_clipboard(user_data: *mut c_void, text: *const u8, length: usize) {
    let Some(bridge) = (unsafe { user_data.cast::<CallbackBridge>().as_ref() }) else {
        return;
    };
    if text.is_null() || length == 0 || length > MAX_CLIPBOARD_BYTES {
        return;
    }
    let Some(bytes) = (unsafe { ptr::slice_from_raw_parts(text, length).as_ref() }) else {
        return;
    };
    let mut utf16 = bytes
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
        .collect::<Vec<_>>();
    while utf16.last() == Some(&0) {
        utf16.pop();
    }
    let text = String::from_utf16_lossy(&utf16);
    let _ = bridge.events.send(BackendEvent::RemoteDesktopClipboard {
        tab_id: bridge.tab_id.clone(),
        text,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_pixel_stats_ignore_stride_padding() {
        let size = FrameSize::new(2, 1, 12).unwrap();
        let frame = DecodedFrame::new(1, size, vec![10, 20, 30, 0, 255, 255, 255, 127, 1, 2, 3, 4])
            .unwrap();

        assert_eq!(
            frame_pixel_stats(&frame),
            FramePixelStats {
                visible_pixels: 2,
                sampled_pixels: 2,
                rgb_non_white_pixels: 1,
                alpha_zero_pixels: 1,
                rgb_min: 10,
                rgb_max: 255,
                alpha_min: 0,
                alpha_max: 127,
            }
        );
    }

    #[test]
    fn frame_pixel_stats_bound_work_for_large_frames() {
        let size = FrameSize::new(100, 100, 400).unwrap();
        let frame = DecodedFrame::new(1, size, vec![0; 40_000]).unwrap();

        let stats = frame_pixel_stats(&frame);

        assert_eq!(stats.visible_pixels, 10_000);
        assert_eq!(stats.sampled_pixels, MAX_FRAME_DIAGNOSTIC_SAMPLES as u64);
        assert_eq!(stats.rgb_non_white_pixels, stats.sampled_pixels);
        assert_eq!(stats.alpha_zero_pixels, stats.sampled_pixels);
    }
}
