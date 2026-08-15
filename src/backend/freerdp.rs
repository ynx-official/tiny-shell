//! Rust side of the optional FreeRDP C shim.
//!
//! The shim owns every FreeRDP object and invokes these callbacks from its
//! worker thread. Rust only receives copied BGRA frames, so no FreeRDP pointer
//! is allowed to escape into the UI or outlive the native client.

use std::{
    ffi::{CString, c_char, c_void},
    ptr,
    sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}},
};

use anyhow::{Context, Result, anyhow};
use rust_i18n::t;

use super::{DecodedFrame, FrameMailbox, FrameSize};
use crate::{
    session::config::Session,
    terminal::{BackendEvent, BackendEventSender},
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

#[repr(C)]
struct NativeCallbacks {
    user_data: *mut c_void,
    on_state: Option<StateCallback>,
    on_frame: Option<FrameCallback>,
    should_stop: Option<ShouldStopCallback>,
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
    fn tiny_shell_rdp_client_free(client: *mut NativeClient);
}

struct CallbackBridge {
    tab_id: String,
    events: BackendEventSender,
    mailbox: Arc<FrameMailbox>,
    stop_requested: Arc<AtomicBool>,
    sequence: AtomicU64,
}

pub(crate) fn run(
    tab_id: String,
    session: Session,
    width: u32,
    height: u32,
    events: BackendEventSender,
    mailbox: Arc<FrameMailbox>,
    stop_requested: Arc<AtomicBool>,
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
    };

    // The native client copies all configuration strings during `new`, and
    // does not retain the callback descriptor after the call returns.
    let client = unsafe { tiny_shell_rdp_client_new(&config, &callbacks) };
    if client.is_null() {
        unsafe { drop(Box::from_raw(bridge_ptr)); }
        return Err(anyhow!("FreeRDP client initialization failed"));
    }
    let result = unsafe { tiny_shell_rdp_client_run(client) };
    unsafe { tiny_shell_rdp_client_free(client); }
    unsafe { drop(Box::from_raw(bridge_ptr)); }
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
