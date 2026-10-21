use super::*;
use chrono::TimeZone;

#[test]
fn detects_named_zones_without_interpreting_banners_or_abbreviations() {
    assert_eq!(
        parse_probe_output(b"Welcome\r\nTINYSHELL_TZ=Asia/Shanghai\r\n"),
        Some(chrono_tz::Asia::Shanghai)
    );
    assert_eq!(
        parse_probe_output(b"TINYSHELL_TZ=:/usr/share/zoneinfo/America/New_York\n"),
        Some(chrono_tz::America::New_York)
    );
    for output in [
        "Asia/Shanghai\n",
        "TINYSHELL_TZ=CST\n",
        "TINYSHELL_TZ=+0800\n",
        "TINYSHELL_TZ=unknown\n",
        "TINYSHELL_TZ=\n",
        "TINYSHELL_TZ=UTC\nTINYSHELL_TZ=Asia/Shanghai\n",
    ] {
        assert_eq!(parse_probe_output(output.as_bytes()), None, "{output}");
    }
}

#[test]
fn formats_server_time_using_the_rules_for_the_file_date() -> anyhow::Result<()> {
    let shanghai = SftpTimeZone::Server(chrono_tz::Asia::Shanghai);
    assert_eq!(shanghai.format_mtime(0), "1970-01-01 08:00");
    let new_york = SftpTimeZone::Server(chrono_tz::America::New_York);
    for (month, expected) in [(1, "2026-01-15 07:00"), (7, "2026-07-15 08:00")] {
        let timestamp = Utc
            .with_ymd_and_hms(2026, month, 15, 12, 0, 0)
            .single()
            .ok_or_else(|| anyhow!("invalid test date"))?
            .timestamp() as u32;
        assert_eq!(new_york.format_mtime(timestamp), expected);
    }
    Ok(())
}

#[test]
fn pending_and_failed_detection_use_local_time() -> anyhow::Result<()> {
    let timestamp = 1_700_000_000;
    let expected = DateTime::from_timestamp(i64::from(timestamp), 0)
        .ok_or_else(|| anyhow!("invalid test timestamp"))?
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string();
    assert_eq!(SftpTimeZone::default(), SftpTimeZone::Detecting);
    assert_eq!(SftpTimeZone::Detecting.format_mtime(timestamp), expected);
    assert_eq!(SftpTimeZone::Local.format_mtime(timestamp), expected);
    Ok(())
}

#[test]
fn labels_show_server_name_and_offset() -> anyhow::Result<()> {
    let now = Utc
        .with_ymd_and_hms(2026, 1, 15, 12, 0, 0)
        .single()
        .ok_or_else(|| anyhow!("invalid test date"))?;
    assert_eq!(
        SftpTimeZone::Server(chrono_tz::Asia::Kathmandu).zone_label(now),
        "Asia/Kathmandu (UTC+05:45)"
    );
    assert!(SftpTimeZone::Local.zone_label(now).starts_with("UTC"));
    Ok(())
}

#[test]
fn powershell_probe_is_encoded_for_the_outer_ssh_shell() -> anyhow::Result<()> {
    let command = windows_probe_command();
    let encoded = command
        .split_whitespace()
        .last()
        .ok_or_else(|| anyhow!("missing encoded command"))?;
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)?;
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    assert_eq!(String::from_utf16(&units)?, WINDOWS_PROBE_SCRIPT);
    assert!(!command.contains('$'));
    Ok(())
}

mod transport {
    use super::*;
    use async_trait::async_trait;
    use russh::{Channel, ChannelId, client, server, server::Server as _};
    use std::{
        collections::HashMap,
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    #[derive(Clone, Copy)]
    enum Reply {
        Zone,
        Unknown,
        Denied,
        Windows,
        Oversized,
        Hanging,
    }

    #[derive(Clone)]
    struct Server {
        reply: Reply,
        executions: Arc<AtomicUsize>,
    }

    struct Handler {
        server: Server,
        channels: HashMap<ChannelId, Channel<server::Msg>>,
    }

    impl server::Server for Server {
        type Handler = Handler;

        fn new_client(&mut self, _: Option<SocketAddr>) -> Handler {
            Handler {
                server: self.clone(),
                channels: HashMap::new(),
            }
        }
    }

    #[async_trait]
    impl server::Handler for Handler {
        type Error = anyhow::Error;

        async fn auth_password(&mut self, _: &str, _: &str) -> Result<server::Auth> {
            Ok(server::Auth::Accept)
        }

        async fn channel_open_session(
            &mut self,
            channel: Channel<server::Msg>,
            _: &mut server::Session,
        ) -> Result<bool> {
            self.channels.insert(channel.id(), channel);
            Ok(true)
        }

        async fn exec_request(
            &mut self,
            id: ChannelId,
            command: &[u8],
            session: &mut server::Session,
        ) -> Result<()> {
            self.server.executions.fetch_add(1, Ordering::SeqCst);
            let unix_probe = command == UNIX_PROBE.as_bytes();
            if matches!(self.server.reply, Reply::Denied) {
                session.channel_failure(id)?;
                return Ok(());
            }
            session.channel_success(id)?;
            if matches!(self.server.reply, Reply::Hanging) && unix_probe {
                return Ok(());
            }
            let (output, status) = match self.server.reply {
                Reply::Unknown => (b"TINYSHELL_TZ=CST\n".to_vec(), 0),
                Reply::Windows if unix_probe => (Vec::new(), 127),
                Reply::Hanging => (Vec::new(), 127),
                Reply::Oversized => (vec![b'x'; MAX_PROBE_OUTPUT + 1], 0),
                _ => (
                    b"login banner\r\nTINYSHELL_TZ=Asia/Shanghai\r\n".to_vec(),
                    0,
                ),
            };
            session.data(id, output.as_slice().into())?;
            session.exit_status_request(id, status)?;
            session.eof(id)?;
            session.close(id)?;
            Ok(())
        }
    }

    async fn detect(reply: Reply) -> Result<(SftpTimeZone, usize)> {
        let workspace = crate::session::config::TempWorkspace::initialize_in(
            std::env::temp_dir().join("tiny-shell-timezone-tests"),
        )?;
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = socket.local_addr()?.port();
        let config = Arc::new(server::Config {
            keys: vec![russh::keys::PrivateKey::random(
                &mut rand::thread_rng(),
                ssh_key::Algorithm::Ed25519,
            )?],
            auth_rejection_time: Duration::ZERO,
            ..Default::default()
        });
        let executions = Arc::new(AtomicUsize::new(0));
        let mut server = Server {
            reply,
            executions: executions.clone(),
        };
        // Unlike a detached JoinHandle, JoinSet aborts the server on early errors.
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move { server.run_on_socket(config, &socket).await });
        let handler =
            SftpClientHandler::in_path("127.0.0.1", port, workspace.path().join("known_hosts"))?;
        let mut client = client::connect(
            Arc::new(client::Config::default()),
            ("127.0.0.1", port),
            handler,
        )
        .await?;
        assert!(client.authenticate_password("test", "test").await?);
        let result =
            tokio::time::timeout(Duration::from_secs(8), detect_server_timezone(&client)).await?;
        // Applying the result to many displayed files never issues another command.
        for timestamp in [0, 1_700_000_000, u32::MAX] {
            assert!(!result.format_mtime(timestamp).is_empty());
        }
        client
            .disconnect(russh::Disconnect::ByApplication, "done", "")
            .await?;
        Ok((result, executions.load(Ordering::SeqCst)))
    }

    #[tokio::test]
    async fn successful_detection_is_reused_and_platform_fallback_is_bounded() -> Result<()> {
        let server = SftpTimeZone::Server(chrono_tz::Asia::Shanghai);
        assert_eq!(detect(Reply::Zone).await?, (server, 1));
        assert_eq!(detect(Reply::Windows).await?, (server, 2));
        assert_eq!(detect(Reply::Unknown).await?, (SftpTimeZone::Local, 1));
        Ok(())
    }

    #[tokio::test]
    async fn denied_oversized_and_hanging_probes_fall_back_to_local() -> Result<()> {
        for reply in [Reply::Denied, Reply::Oversized, Reply::Hanging] {
            assert_eq!(detect(reply).await?, (SftpTimeZone::Local, 2));
        }
        Ok(())
    }
}
