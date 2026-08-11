use std::{
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use rust_i18n::t;

use crate::terminal::{BackendCommand, BackendEvent, BackendEventSender, BackendTx};

fn send_closed_once(
    events: &BackendEventSender,
    notified: &AtomicBool,
    tab_id: &str,
    reason: String,
) {
    if !notified.swap(true, Ordering::AcqRel) {
        let _ = events.send(BackendEvent::Closed {
            tab_id: tab_id.to_string(),
            reason,
        });
    }
}

pub(crate) fn spawn_local_terminal(
    tab_id: String,
    cols: u16,
    rows: u16,
    events: BackendEventSender,
    generation: u64,
) -> Result<BackendTx> {
    let events = events.with_generation(generation);
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("open local PTY")?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) {
            "powershell.exe".into()
        } else {
            "/bin/zsh".into()
        }
    });

    let mut cmd = CommandBuilder::new(&shell);
    cmd.env(
        "TERM",
        std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into()),
    );
    cmd.env(
        "COLORTERM",
        std::env::var("COLORTERM").unwrap_or_else(|_| "truecolor".into()),
    );
    cmd.env("TERM_PROGRAM", "tiny-shell");
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    if let Ok(lang) = std::env::var("LANG") {
        cmd.env("LANG", lang);
    } else {
        cmd.env("LANG", "en_US.UTF-8");
    }
    if let Ok(home) = std::env::var("HOME") {
        cmd.env("HOME", home);
    }
    cmd.env("SHELL", shell);
    let mut child = pair.slave.spawn_command(cmd).context("spawn local shell")?;
    drop(pair.slave);

    let master = pair.master;
    let mut reader = master.try_clone_reader().context("clone PTY reader")?;
    let mut writer = master.take_writer().context("take PTY writer")?;
    let (cmd_tx, cmd_rx) =
        mpsc::sync_channel::<BackendCommand>(crate::terminal::BACKEND_COMMAND_QUEUE_CAPACITY);
    let close_notified = Arc::new(AtomicBool::new(false));
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let output_overloaded = Arc::new(AtomicBool::new(false));
    let resize_requested = Arc::new(std::sync::Mutex::new(None));

    let read_tab = tab_id.clone();
    let read_events = events.clone();
    let read_close_notified = close_notified.clone();
    let read_shutdown_requested = shutdown_requested.clone();
    let read_output_overloaded = output_overloaded.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if read_events
                        .send(BackendEvent::Output {
                            tab_id: read_tab.clone(),
                            bytes: buf[..n].to_vec(),
                        })
                        .is_err()
                    {
                        send_closed_once(
                            &read_events,
                            &read_close_notified,
                            &read_tab,
                            t!("terminal_output_overloaded").to_string(),
                        );
                        read_output_overloaded.store(true, Ordering::Release);
                        read_shutdown_requested.store(true, Ordering::Release);
                        return;
                    }
                }
                Err(err) => {
                    send_closed_once(
                        &read_events,
                        &read_close_notified,
                        &read_tab,
                        format!("local read error: {err}"),
                    );
                    read_shutdown_requested.store(true, Ordering::Release);
                    return;
                }
            }
        }
        send_closed_once(
            &read_events,
            &read_close_notified,
            &read_tab,
            "local shell closed".into(),
        );
        read_shutdown_requested.store(true, Ordering::Release);
    });

    let write_tab = tab_id.clone();
    let write_events = events.clone();
    let write_close_notified = close_notified;
    let write_shutdown_requested = shutdown_requested.clone();
    let write_output_overloaded = output_overloaded.clone();
    let write_resize_requested = resize_requested.clone();
    thread::spawn(move || {
        let exit_reason = loop {
            if write_output_overloaded.load(Ordering::Acquire) {
                break t!("terminal_output_overloaded").to_string();
            }
            if write_shutdown_requested.load(Ordering::Acquire) {
                break "local shell closed".to_string();
            }
            if let Some((cols, rows)) = write_resize_requested
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                let _ = master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            }
            match cmd_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(command) => match command {
                    BackendCommand::Input(bytes) => {
                        if let Err(err) = writer.write_all(&bytes) {
                            break format!("local write error: {err}");
                        }
                        let _ = writer.flush();
                    }
                    BackendCommand::Resize { cols, rows } => {
                        let _ = master.resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                    BackendCommand::Close => {
                        break "local shell closed".to_string();
                    }
                    BackendCommand::SampleMetrics => {}
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Ok(Some(status)) = child.try_wait() {
                        send_closed_once(
                            &write_events,
                            &write_close_notified,
                            &write_tab,
                            format!("local shell exited: {status}"),
                        );
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break "local shell command channel closed".to_string();
                }
            }
        };
        let _ = child.kill();
        if let Err(err) = child.wait() {
            tracing::debug!("failed to reap local shell process: {err:#}");
        }
        send_closed_once(
            &write_events,
            &write_close_notified,
            &write_tab,
            exit_reason,
        );
    });

    let _ = events.send(BackendEvent::Status {
        tab_id,
        text: "local shell ready".into(),
    });

    Ok(BackendTx::Local {
        commands: cmd_tx,
        close: shutdown_requested,
        resize: resize_requested,
    })
}
