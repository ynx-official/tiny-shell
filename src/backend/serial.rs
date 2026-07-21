use crate::session::config::Session;
use crate::terminal::{BackendCommand, BackendEvent};
use std::io::{Read, Write};

/// Spawn the serial port backend threads.
/// Returns a sender to send commands (like keyboard inputs) to the serial port.
pub fn spawn_serial_client(
    _handle: &tokio::runtime::Handle,
    tab_id: String,
    session: Session,
    events_tx: std::sync::mpsc::Sender<BackendEvent>,
) -> tokio::sync::mpsc::UnboundedSender<BackendCommand> {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<BackendCommand>();

    let tab_id_clone = tab_id.clone();
    let events_tx_clone = events_tx.clone();

    std::thread::spawn(move || {
        let _ = events_tx_clone.send(BackendEvent::Status {
            tab_id: tab_id_clone.clone(),
            text: rust_i18n::t!("starting_connection").to_string(),
        });

        let port_name = session.host;
        let baud_rate = session.baud_rate;

        tracing::info!(
            "[serial] opening port {} at baud rate {}",
            port_name,
            baud_rate
        );

        let mut port_result = serialport::new(&port_name, baud_rate)
            .timeout(std::time::Duration::from_millis(100))
            .open();

        if port_result.is_err() && baud_rate != 0 {
            tracing::info!(
                "[serial] failed to open port with baud rate {}, retrying with 0 (virtual port mode)",
                baud_rate
            );
            port_result = serialport::new(&port_name, 0)
                .timeout(std::time::Duration::from_millis(100))
                .open();
        }

        let mut port = match port_result {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("[serial] failed to open port {}: {}", port_name, e);
                let _ = events_tx_clone.send(BackendEvent::Closed {
                    tab_id: tab_id_clone,
                    reason: format!("Failed to open serial port {port_name}: {e}"),
                });
                return;
            }
        };

        let mut port_write = match port.try_clone() {
            Ok(pw) => pw,
            Err(e) => {
                tracing::error!("[serial] failed to clone port: {}", e);
                let _ = events_tx_clone.send(BackendEvent::Closed {
                    tab_id: tab_id_clone,
                    reason: format!("Failed to clone serial port: {e}"),
                });
                return;
            }
        };

        // Notify connected
        let _ = events_tx_clone.send(BackendEvent::Connected {
            tab_id: tab_id_clone.clone(),
        });

        // Spawn write thread
        let tab_id_write = tab_id_clone.clone();
        let events_tx_write = events_tx_clone.clone();
        std::thread::spawn(move || {
            while let Some(cmd) = cmd_rx.blocking_recv() {
                match cmd {
                    BackendCommand::Input(bytes) => {
                        if let Err(e) = port_write.write_all(&bytes) {
                            tracing::error!("[serial] write error: {}", e);
                            let _ = events_tx_write.send(BackendEvent::Closed {
                                tab_id: tab_id_write.clone(),
                                reason: format!("Serial write error: {e}"),
                            });
                            break;
                        }
                        let _ = port_write.flush();
                    }
                    BackendCommand::Close => break,
                    _ => {}
                }
            }
        });

        // Read loop in current thread
        let mut buf = [0u8; 1024];
        let mut last_was_cr = false;
        loop {
            match port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    let mut processed = Vec::with_capacity(n * 2);
                    let read_bytes = &buf[..n];
                    for i in 0..n {
                        let b = read_bytes[i];
                        if b == b'\n' {
                            let prev_was_cr = if i > 0 {
                                read_bytes[i - 1] == b'\r'
                            } else {
                                last_was_cr
                            };
                            if !prev_was_cr {
                                processed.push(b'\r');
                            }
                        }
                        processed.push(b);
                    }
                    last_was_cr = read_bytes[n - 1] == b'\r';

                    let _ = events_tx_clone.send(BackendEvent::Output {
                        tab_id: tab_id_clone.clone(),
                        bytes: processed,
                    });
                }
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => {
                    tracing::info!("[serial] port read error/closed: {}", e);
                    let _ = events_tx_clone.send(BackendEvent::Closed {
                        tab_id: tab_id_clone,
                        reason: format!("Serial read error: {e}"),
                    });
                    break;
                }
            }
        }
    });

    cmd_tx
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use portable_pty::{NativePtySystem, PtySize, PtySystem};

    #[tokio::test]
    async fn test_serial_read_write_simulation() {
        // 1. Create a PTY pair using portable-pty to simulate a serial device
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();

        // On macOS/Linux, the slave name path behaves like a TTY device.
        let fd = pair.master.as_raw_fd().unwrap();
        let slave_name = unsafe {
            let ptr = libc::ptsname(fd);
            assert!(!ptr.is_null());
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        };
        println!("Simulating serial device on PTY slave path: {}", slave_name);

        // 2. Spawn the serial backend targeting the PTY slave path
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let handle = tokio::runtime::Handle::current();
        let session = Session::serial(slave_name, 0);
        let cmd_tx = spawn_serial_client(&handle, "test-tab".to_string(), session, events_tx);

        // Wait for the Status event
        let status_event = events_rx.recv_timeout(std::time::Duration::from_secs(2));
        assert!(status_event.is_ok(), "Failed to receive Status event");
        if let Ok(BackendEvent::Status { tab_id, .. }) = status_event {
            assert_eq!(tab_id, "test-tab");
        } else {
            panic!("Expected Status event, got: {:?}", status_event);
        }

        // Wait for the Connected event
        let connected_event = events_rx.recv_timeout(std::time::Duration::from_secs(2));
        assert!(connected_event.is_ok(), "Failed to receive Connected event");
        if let Ok(BackendEvent::Connected { tab_id }) = connected_event {
            assert_eq!(tab_id, "test-tab");
        } else {
            panic!("Expected Connected event, got: {:?}", connected_event);
        }

        // 3. Test Reading: Write to PTY master, verify serial backend outputs it to UI
        let mut master_writer = pair.master.take_writer().unwrap();
        master_writer.write_all(b"hello serial simulator").unwrap();
        master_writer.flush().unwrap();

        let output_event = events_rx.recv_timeout(std::time::Duration::from_secs(2));
        assert!(output_event.is_ok(), "Failed to receive Output event");
        if let Ok(BackendEvent::Output { tab_id, bytes }) = output_event {
            assert_eq!(tab_id, "test-tab");
            assert_eq!(bytes, b"hello serial simulator");
        } else {
            panic!("Expected Output event");
        }

        // 4. Test Writing: Send BackendCommand::Input to backend, verify PTY master reads it
        cmd_tx
            .send(BackendCommand::Input(b"world serial simulator".to_vec()))
            .unwrap();

        let mut master_reader = pair.master.try_clone_reader().unwrap();
        let mut read_buf = [0u8; 128];
        let bytes_read = master_reader.read(&mut read_buf).unwrap();
        assert_eq!(&read_buf[..bytes_read], b"world serial simulator");

        // 5. Clean up
        cmd_tx.send(BackendCommand::Close).unwrap();
    }
}
