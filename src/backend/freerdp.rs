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
    FrameSize, is_certificate_trusted, remember_certificate,
};
use crate::{
    session::config::Session,
    terminal::{BackendCommand, BackendEvent, BackendEventSender, RemoteDesktopInput},
};

const STATE_CONNECTING: u32 = 1;
const STATE_CONNECTED: u32 = 2;
const STATE_DISCONNECTED: u32 = 3;
const STATE_FAILED: u32 = 4;

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

#[repr(C)]
struct NativeCallbacks {
    user_data: *mut c_void,
    on_state: Option<StateCallback>,
    on_frame: Option<FrameCallback>,
    should_stop: Option<ShouldStopCallback>,
    on_poll: Option<PollCallback>,
    on_certificate: Option<CertificateCallback>,
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
    fn tiny_shell_rdp_client_text(
        client: *mut NativeClient,
        text: *const u16,
        length: usize,
    ) -> i32;
    fn tiny_shell_rdp_client_mouse(client: *mut NativeClient, flags: u16, x: u16, y: u16) -> i32;
    fn tiny_shell_rdp_client_free(client: *mut NativeClient);
}

struct CallbackBridge {
    tab_id: String,
    events: BackendEventSender,
    mailbox: Arc<FrameMailbox>,
    stop_requested: Arc<AtomicBool>,
    sequence: AtomicU64,
    command_rx: Mutex<mpsc::Receiver<BackendCommand>>,
    client: std::sync::atomic::AtomicUsize,
}

pub(crate) fn run(
    tab_id: String,
    session: Session,
    width: u32,
    height: u32,
    events: BackendEventSender,
    mailbox: Arc<FrameMailbox>,
    stop_requested: Arc<AtomicBool>,
    command_rx: mpsc::Receiver<BackendCommand>,
) -> Result<()> {
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
        stop_requested,
        sequence: AtomicU64::new(0),
        command_rx: Mutex::new(command_rx),
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
    let result = unsafe { tiny_shell_rdp_client_run(client) };
    unsafe {
        (*bridge_ptr).client.store(0, Ordering::Release);
    }
    unsafe {
        tiny_shell_rdp_client_free(client);
    }
    unsafe {
        drop(Box::from_raw(bridge_ptr));
    }
    if result != 0 {
        return Err(anyhow!("FreeRDP connection failed with code {result}"));
    }
    Ok(())
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
            let _ = bridge.events.send(BackendEvent::Status {
                tab_id: bridge.tab_id.clone(),
                text: t!("rdp_connecting").to_string(),
            });
        }
        STATE_CONNECTED => {
            let _ = bridge.events.send(BackendEvent::Connected {
                tab_id: bridge.tab_id.clone(),
            });
        }
        STATE_DISCONNECTED => {
            let _ = bridge.events.send(BackendEvent::Status {
                tab_id: bridge.tab_id.clone(),
                text: t!("rdp_connection_closed").to_string(),
            });
        }
        STATE_FAILED => {
            let _ = bridge.events.send(BackendEvent::Status {
                tab_id: bridge.tab_id.clone(),
                text: t!(
                    "rdp_connection_failed",
                    error_code = error_code,
                    message = message
                )
                .to_string(),
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
    if let Ok(frame) = DecodedFrame::new(sequence, size, slice.to_vec()) {
        bridge.mailbox.publish(frame);
        let _ = bridge.events.send(BackendEvent::RemoteDesktopFrameReady {
            tab_id: bridge.tab_id.clone(),
            sequence,
        });
    }
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
            BackendCommand::RemoteDesktopResize { width, height } => unsafe {
                tiny_shell_rdp_client_resize(client, width, height) != 0
            },
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
            BackendCommand::RemoteDesktopText(text) => {
                let utf16: Vec<u16> = text.encode_utf16().collect();
                unsafe { tiny_shell_rdp_client_text(client, utf16.as_ptr(), utf16.len()) != 0 }
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
    let Some(bridge) = (unsafe { user_data.cast::<CallbackBridge>().as_ref() }) else {
        return 0;
    };
    let _ = flags;
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
                decision: decision.clone(),
            },
        )))
        .is_err()
    {
        return 0;
    }
    match decision.wait() {
        CertificateDecisionKind::TrustOnce => 2,
        CertificateDecisionKind::TrustAlways => {
            remember_certificate(&host, port, &fingerprint);
            2
        }
        CertificateDecisionKind::Reject => 0,
    }
}
