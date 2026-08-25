use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use directories::BaseDirs;
use russh::{
    ChannelMsg, Disconnect,
    client::{self, Handler},
    keys::{PrivateKey, decode_secret_key, load_secret_key},
};
use rust_i18n::t;
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};

use crate::{
    session::{
        config::{AuthMethod, ConfigStore, Session},
        host_key::HostKeyVerifier,
        ssh_keys::{
            authenticate_with_default_keys, normalize_inline_private_key, private_keys_with_algs,
            session_has_explicit_key,
        },
    },
    system::{SystemSnapshot, remote_snapshot_from_kv},
    terminal::{BackendCommand, BackendEvent, BackendEventSender, BackendTx},
};

pub(crate) struct SshTerminalRequest {
    tab_id: String,
    session: Session,
    proxy_config: ConfigStore,
    cols: u16,
    rows: u16,
    generation: u64,
}

impl SshTerminalRequest {
    pub(crate) fn new(
        tab_id: String,
        session: Session,
        proxy_config: ConfigStore,
        cols: u16,
        rows: u16,
        generation: u64,
    ) -> Self {
        Self {
            tab_id,
            session,
            proxy_config,
            cols,
            rows,
            generation,
        }
    }
}

pub(crate) fn spawn_ssh_terminal(
    runtime: &tokio::runtime::Handle,
    request: SshTerminalRequest,
    events: BackendEventSender,
) -> BackendTx {
    let SshTerminalRequest {
        tab_id,
        session,
        proxy_config,
        cols,
        rows,
        generation,
    } = request;
    let events = events.with_generation(generation);
    let (cmd_tx, cmd_rx) =
        mpsc::channel::<BackendCommand>(crate::terminal::BACKEND_COMMAND_QUEUE_CAPACITY);
    let (close_tx, close_rx) = watch::channel(false);
    let (resize_tx, resize_rx) = watch::channel(None);
    let task_tab = tab_id.clone();
    runtime.spawn(async move {
        if let Err(err) = run_ssh(
            task_tab.clone(),
            session,
            proxy_config,
            cols,
            rows,
            cmd_rx,
            close_rx,
            resize_rx,
            events.clone(),
        )
        .await
        {
            let _ = events.send(BackendEvent::Closed {
                tab_id: task_tab,
                reason: format!("{err:#}"),
            });
        }
    });
    BackendTx::Ssh {
        commands: cmd_tx,
        close: close_tx,
        resize: resize_tx,
    }
}

async fn wait_for_close(close: &mut watch::Receiver<bool>) {
    let already_requested = *close.borrow_and_update();
    if !already_requested {
        let _ = close.changed().await;
    }
}

async fn sample_remote_system_with_handle(
    handle: Arc<tokio::sync::Mutex<russh::client::Handle<ClientHandler>>>,
) -> Result<SystemSnapshot> {
    const MAX_METRICS_OUTPUT_BYTES: usize = 512 * 1024;
    let mut channel = handle
        .lock()
        .await
        .channel_open_session()
        .await
        .context("open metrics session")?;
    channel
        .exec(true, REMOTE_SYSTEM_PROBE)
        .await
        .context("exec remote metrics probe")?;

    let mut stdout = Vec::new();
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, ext: _ } => {
                if stdout.len().saturating_add(data.len()) > MAX_METRICS_OUTPUT_BYTES {
                    return Err(anyhow!(
                        "remote metrics output exceeded {MAX_METRICS_OUTPUT_BYTES} bytes"
                    ));
                }
                stdout.extend_from_slice(&data);
            }
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }

    let output = String::from_utf8_lossy(&stdout);
    remote_snapshot_from_kv(&output)
}

async fn execute_remote_docker(
    handle: Arc<tokio::sync::Mutex<russh::client::Handle<ClientHandler>>>,
    request: crate::docker::DockerRequest,
) -> crate::docker::DockerResponse {
    let request_timeout = crate::docker::command_spec(&request.operation)
        .map(|spec| spec.timeout)
        .unwrap_or(crate::docker::LIST_TIMEOUT);
    let result = async {
        tokio::time::timeout(request_timeout, async {
            let spec = crate::docker::command_spec(&request.operation)?;
            let stdout = execute_remote_docker_command(&handle, &spec).await?;
            let mut payload =
                crate::docker::parse_output(&request.operation, &String::from_utf8_lossy(&stdout))?;
            if let crate::docker::DockerPayload::Containers(containers) = &mut payload
                && let Some(spec) = crate::docker::restart_policy_command_spec(containers)?
            {
                let stdout = execute_remote_docker_command(&handle, &spec).await?;
                crate::docker::apply_restart_policies(
                    containers,
                    &String::from_utf8_lossy(&stdout),
                )?;
            }
            Ok(payload)
        })
        .await
        .map_err(|_| {
            anyhow!(
                "Docker request timed out after {}s",
                request_timeout.as_secs()
            )
        })?
    }
    .await
    .map_err(|error: anyhow::Error| format!("{error:#}"));

    crate::docker::DockerResponse {
        request_id: request.request_id,
        result,
    }
}

async fn execute_remote_docker_command(
    handle: &Arc<tokio::sync::Mutex<russh::client::Handle<ClientHandler>>>,
    spec: &crate::docker::DockerCommandSpec,
) -> Result<Vec<u8>> {
    let command = crate::docker::shell_command_from_spec(spec);
    let execute = async {
        let mut channel = handle
            .lock()
            .await
            .channel_open_session()
            .await
            .context("open Docker command session")?;
        channel
            .exec(true, command)
            .await
            .context("execute remote Docker command")?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_status = None;
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => {
                    if stdout
                        .len()
                        .saturating_add(stderr.len())
                        .saturating_add(data.len())
                        > crate::docker::MAX_OUTPUT_BYTES
                    {
                        return Err(anyhow!("Docker command output exceeded limit"));
                    }
                    stdout.extend_from_slice(&data);
                }
                ChannelMsg::ExtendedData { data, ext: _ } => {
                    if stdout
                        .len()
                        .saturating_add(stderr.len())
                        .saturating_add(data.len())
                        > crate::docker::MAX_OUTPUT_BYTES
                    {
                        return Err(anyhow!("Docker command error output exceeded limit"));
                    }
                    stderr.extend_from_slice(&data);
                }
                ChannelMsg::ExitStatus {
                    exit_status: status,
                } => exit_status = Some(status),
                ChannelMsg::Close => break,
                ChannelMsg::Eof => {}
                _ => {}
            }
        }
        if exit_status.unwrap_or(1) != 0 {
            let message = String::from_utf8_lossy(&stderr).trim().to_owned();
            return Err(if message.is_empty() {
                anyhow!("remote Docker command failed")
            } else {
                anyhow!(message)
            });
        }
        Ok(stdout)
    };
    tokio::time::timeout(spec.timeout, execute)
        .await
        .map_err(|_| anyhow!("Docker command timed out after {}s", spec.timeout.as_secs()))?
}

#[allow(clippy::too_many_arguments)]
async fn run_ssh(
    tab_id: String,
    session: Session,
    proxy_config: ConfigStore,
    cols: u16,
    rows: u16,
    mut commands: mpsc::Receiver<BackendCommand>,
    mut close: watch::Receiver<bool>,
    mut resize: watch::Receiver<Option<(u16, u16)>>,
    events: BackendEventSender,
) -> Result<()> {
    let _ = events.send(BackendEvent::Status {
        tab_id: tab_id.clone(),
        text: format!(
            "connecting {}@{}:{}...",
            session.user, session.host, session.port
        ),
    });

    let handle = {
        let connection = connect_and_authenticate(&tab_id, &session, &events, &proxy_config);
        tokio::pin!(connection);
        tokio::select! {
            biased;
            _ = wait_for_close(&mut close) => {
                let _ = events.send(BackendEvent::Closed {
                    tab_id: tab_id.clone(),
                    reason: "ssh connection cancelled".to_string(),
                });
                return Ok(());
            }
            result = &mut connection => result?,
        }
    };
    let handle = Arc::new(tokio::sync::Mutex::new(handle));

    let mut channel = {
        let open_shell = async {
            let channel = handle
                .lock()
                .await
                .channel_open_session()
                .await
                .context("open session")?;
            channel
                .request_pty(true, "xterm-256color", cols.into(), rows.into(), 0, 0, &[])
                .await
                .context("request pty")?;
            channel.request_shell(true).await.context("request shell")?;
            Ok::<_, anyhow::Error>(channel)
        };
        tokio::pin!(open_shell);
        tokio::select! {
            biased;
            _ = wait_for_close(&mut close) => {
                let _ = events.send(BackendEvent::Closed {
                    tab_id: tab_id.clone(),
                    reason: "ssh connection cancelled".to_string(),
                });
                return Ok(());
            }
            result = &mut open_shell => result?,
        }
    };

    let _ = events.send(BackendEvent::Status {
        tab_id: tab_id.clone(),
        text: format!("connected {}@{}", session.user, session.host),
    });
    let _ = events.send(BackendEvent::Connected {
        tab_id: tab_id.clone(),
    });

    let exit_reason;
    let mut is_graceful_close = false;
    let mut metrics_tasks: JoinSet<Result<SystemSnapshot>> = JoinSet::new();
    let mut metrics_in_flight = false;
    let mut docker_tasks: JoinSet<crate::docker::DockerResponse> = JoinSet::new();

    loop {
        tokio::select! {
            biased;
            _ = wait_for_close(&mut close) => {
                tracing::info!("[ssh] local client cancelled the session for tab {}", tab_id);
                exit_reason = "ssh session closed".to_string();
                break;
            }
            changed = resize.changed() => {
                if changed.is_err() {
                    continue;
                }
                let dimensions = resize.borrow_and_update().as_ref().copied();
                if let Some((cols, rows)) = dimensions {
                    let _ = channel.window_change(cols.into(), rows.into(), 0, 0).await;
                }
            }
            completed = metrics_tasks.join_next(), if !metrics_tasks.is_empty() => {
                metrics_in_flight = false;
                match completed {
                    Some(Ok(Ok(snapshot))) => {
                        let _ = events.send(BackendEvent::RemoteSystem {
                            tab_id: tab_id.clone(),
                            snapshot: Box::new(snapshot),
                        });
                    }
                    Some(Ok(Err(error))) => {
                        let _ = events.send(BackendEvent::RemoteSystemUnavailable {
                            tab_id: tab_id.clone(),
                            reason: format!("remote metrics unavailable: {error:#}"),
                        });
                    }
                    Some(Err(error)) => {
                        let _ = events.send(BackendEvent::RemoteSystemUnavailable {
                            tab_id: tab_id.clone(),
                            reason: format!("remote metrics task stopped: {error}"),
                        });
                    }
                    None => {}
                }
            }
            completed = docker_tasks.join_next(), if !docker_tasks.is_empty() => {
                if let Some(Ok(response)) = completed {
                    let _ = events.send(BackendEvent::DockerResult {
                        tab_id: tab_id.clone(),
                        response,
                    });
                }
            }
            command = commands.recv() => {
                match command {
                    Some(BackendCommand::Input(bytes)) => {
                        if let Err(err) = channel.data(bytes.as_slice()).await {
                            tracing::error!("[ssh] write error on tab {}: {}", tab_id, err);
                            exit_reason = format!("ssh write error: {err}");
                            break;
                        }
                    }
                    Some(BackendCommand::Resize { cols, rows }) => {
                        let _ = channel.window_change(cols.into(), rows.into(), 0, 0).await;
                    }
                    Some(BackendCommand::RemoteDesktopResize { .. })
                    | Some(BackendCommand::RemoteDesktopInput(_))
                    | Some(BackendCommand::RemoteDesktopClipboard(_))
                    | Some(BackendCommand::RemoteDesktopClipboardFiles(_)) => {}
                    Some(BackendCommand::SampleMetrics) => {
                        if !metrics_in_flight {
                            const METRICS_TIMEOUT: std::time::Duration =
                                std::time::Duration::from_secs(8);
                            metrics_in_flight = true;
                            let handle_clone = handle.clone();
                            metrics_tasks.spawn(async move {
                                tokio::time::timeout(
                                    METRICS_TIMEOUT,
                                    sample_remote_system_with_handle(handle_clone),
                                )
                                .await
                                .context("remote metrics probe timed out")?
                            });
                        }
                    }
                    Some(BackendCommand::Docker(request)) => {
                        docker_tasks.spawn(execute_remote_docker(handle.clone(), request));
                    }
                    Some(BackendCommand::Close) | None => {
                        tracing::info!("[ssh] local client closed the session for tab {}", tab_id);
                        exit_reason = "ssh session closed".to_string();
                        break;
                    }
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, ext: _ }) => {
                        if events
                            .send(BackendEvent::Output {
                                tab_id: tab_id.clone(),
                                bytes: data.to_vec(),
                            })
                            .is_err()
                        {
                            exit_reason = t!("terminal_output_overloaded").to_string();
                            break;
                        }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status: _ }) | Some(ChannelMsg::Eof) => {
                        is_graceful_close = true;
                    }
                    Some(ChannelMsg::Close) => {
                        if is_graceful_close {
                            tracing::info!("[ssh] session gracefully closed by server for tab {}", tab_id);
                            exit_reason = "ssh session closed".to_string();
                        } else {
                            tracing::warn!("[ssh] connection abruptly closed by server for tab {}", tab_id);
                            exit_reason = "ssh connection lost (abrupt close)".to_string();
                        }
                        break;
                    }
                    None => {
                        if is_graceful_close {
                            tracing::info!("[ssh] network stream ended gracefully for tab {}", tab_id);
                            exit_reason = "ssh session closed".to_string();
                        } else {
                            tracing::warn!("[ssh] network drop detected for tab {}", tab_id);
                            exit_reason = "ssh connection lost (network drop)".to_string();
                        }
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    metrics_tasks.abort_all();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), channel.eof()).await;
    let disconnect = async {
        handle
            .lock()
            .await
            .disconnect(Disconnect::ByApplication, "bye", "")
            .await
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), disconnect).await;
    let _ = events.send(BackendEvent::Closed {
        tab_id,
        reason: exit_reason,
    });
    Ok(())
}

async fn connect_and_authenticate(
    tab_id: &str,
    session: &Session,
    events: &BackendEventSender,
    proxy_config: &ConfigStore,
) -> Result<russh::client::Handle<ClientHandler>> {
    const CONNECTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

    tokio::time::timeout(CONNECTION_TIMEOUT, async move {
        if session.requires_credential_prompt() {
            return Err(anyhow!(t!("session_credentials_required").to_string()));
        }

        let config = Arc::new(client::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(600)),
            keepalive_interval: Some(std::time::Duration::from_secs(3)),
            keepalive_max: 2,
            ..Default::default()
        });
        let addr = format!("{}:{}", session.host, session.port);
        tracing::info!(
            "[ssh] initiating tcp connection to {} (user: {})",
            addr,
            session.user
        );
    let status_text = if let Some((ptype, phost, pport)) =
        crate::session::config::active_proxy(session, proxy_config)
    {
        let pport_val = pport.unwrap_or_else(|| if ptype == "http" { 8080 } else { 1080 });
        format!(
            "connecting to {addr} via {} proxy {}:{}",
            ptype.to_uppercase(),
            phost,
            pport_val
        )
    } else {
        format!("opening tcp connection to {addr}")
    };
    let _ = events.send(BackendEvent::Status {
        tab_id: tab_id.to_string(),
        text: status_text,
    });
    let stream = crate::session::config::connect_proxy(session, proxy_config).await?;
    let handler = ClientHandler::new(&session.host, session.port)?;
    let mut handle = client::connect_stream(config, stream, handler)
        .await
        .with_context(|| format!("connect {addr} failed"))?;

    tracing::debug!("[ssh] tcp connected to {}", addr);

    let authed = match session.auth {
        AuthMethod::Password => {
            tracing::info!(
                "[ssh] sending password authentication for {}@{}",
                session.user,
                addr
            );
            let _ = events.send(BackendEvent::Status {
                tab_id: tab_id.to_string(),
                text: format!(
                    "connected to {addr}, sending password authentication for {}",
                    session.user
                ),
            });
            handle
                .authenticate_password(&session.user, &session.password)
                .await
                .context("password authentication failed")?
        }
        AuthMethod::Key => {
            let has_explicit_key = session_has_explicit_key(session);
            let source = if has_explicit_key {
                key_source_label(session)
            } else {
                "~/.ssh/ default keys".to_string()
            };
            tracing::info!(
                "[ssh] sending key authentication for {}@{} (key source: {})",
                session.user,
                addr,
                source
            );
            let _ = events.send(BackendEvent::Status {
                tab_id: tab_id.to_string(),
                text: if has_explicit_key {
                    format!("connected to {addr}, loading private key from {source}")
                } else {
                    format!(
                        "connected to {addr}, trying default keys from ~/.ssh/ for {}",
                        session.user
                    )
                },
            });

            let passphrase = session.passphrase.trim();
            let passphrase = (!passphrase.is_empty()).then_some(passphrase);

            if has_explicit_key {
                let keypair = load_session_private_key(session)?;
                let algorithm = format!("{:?}", keypair.algorithm());
                let _ = events.send(BackendEvent::Status {
                    tab_id: tab_id.to_string(),
                    text: format!("private key loaded from {source}, algorithm {algorithm}, sending public key authentication for {}", session.user),
                });
                let keys = private_keys_with_algs(keypair).context("invalid private key")?;
                let mut success = false;
                for key in keys {
                    match handle.authenticate_publickey(&session.user, key).await {
                        Ok(true) => {
                            success = true;
                            break;
                        }
                        Ok(false) => {
                            tracing::debug!(
                                "[ssh] public key auth failed with algorithm, trying next"
                            );
                            continue;
                        }
                        Err(e) => {
                            tracing::debug!("[ssh] public key auth error: {:?}, trying next", e);
                            continue;
                        }
                    }
                }
                if !success {
                    return Err(anyhow::anyhow!(
                        "public key authentication failed for {}@{}:{} using {} ({})",
                        session.user,
                        session.host,
                        session.port,
                        source,
                        algorithm
                    ));
                }
                success
            } else {
                let success =
                    authenticate_with_default_keys(&mut handle, &session.user, passphrase).await?;
                if !success {
                    return Err(anyhow::anyhow!(
                        "public key authentication failed for {}@{}:{} - no valid default key found in ~/.ssh/",
                        session.user,
                        session.host,
                        session.port
                    ));
                }
                success
            }
        }
        AuthMethod::KeyPending => {
            return Err(anyhow!(t!("session_credentials_required").to_string()));
        }
        AuthMethod::Config => {
            // SSH Config auth: try the identity file from config, or default keys
            let source = key_source_label(session);
            tracing::info!(
                "[ssh] sending ssh-config authentication for {}@{} (key source: {})",
                session.user,
                addr,
                source
            );
            let _ = events.send(BackendEvent::Status {
                tab_id: tab_id.to_string(),
                text: format!("connected to {addr}, loading private key from {source}"),
            });

            // If an explicit key path is set from the SSH config IdentityFile, use it;
            // otherwise try default keys from ~/.ssh/
            // Note: for Config auth, we never use inline key content
            let has_explicit_key = !session.private_key_path.trim().is_empty();
            if has_explicit_key {
                let keypair = load_session_private_key(session)?;
                let algorithm = format!("{:?}", keypair.algorithm());
                let keys = private_keys_with_algs(keypair).context("invalid private key")?;
                let mut success = false;
                for key in keys {
                    match handle.authenticate_publickey(&session.user, key).await {
                        Ok(true) => {
                            success = true;
                            break;
                        }
                        Ok(false) => {
                            continue;
                        }
                        Err(_) => {
                            continue;
                        }
                    }
                }
                if !success {
                    return Err(anyhow::anyhow!(
                        "ssh-config key authentication failed for {}@{}:{} using {} ({})",
                        session.user,
                        session.host,
                        session.port,
                        source,
                        algorithm
                    ));
                }
                success
            } else {
                let passphrase = session.passphrase.trim();
                let passphrase = (!passphrase.is_empty()).then_some(passphrase);
                let _ = events.send(BackendEvent::Status {
                    tab_id: tab_id.to_string(),
                    text: format!(
                        "connected to {addr}, trying default keys from ~/.ssh/ for {}",
                        session.user
                    ),
                });
                let success =
                    authenticate_with_default_keys(&mut handle, &session.user, passphrase).await?;
                if !success {
                    return Err(anyhow::anyhow!(
                        "ssh-config authentication failed for {}@{}:{} - no valid default key found",
                        session.user,
                        session.host,
                        session.port
                    ));
                }
                success
            }
        }
    };

    if !authed {
        tracing::warn!("[ssh] authentication failed for {}@{}", session.user, addr);
        let _ = handle
            .disconnect(Disconnect::ByApplication, "auth failed", "")
            .await;
        return Err(anyhow!(
            "{}",
            match session.auth {
                AuthMethod::Password => format!(
                    "authentication failed: server rejected password authentication for {}@{}:{}",
                    session.user, session.host, session.port
                ),
                AuthMethod::Key | AuthMethod::KeyPending => format!(
                    "authentication failed: server rejected public key authentication for {}@{}:{} using {}",
                    session.user,
                    session.host,
                    session.port,
                    key_source_label(session)
                ),
                AuthMethod::Config => format!(
                    "authentication failed: server rejected ssh-config authentication for {}@{}:{}",
                    session.user, session.host, session.port
                ),
            }
        ));
    }

    tracing::info!(
        "[ssh] authentication successful for {}@{}",
        session.user,
        addr
    );

    let _ = events.send(BackendEvent::Status {
        tab_id: tab_id.to_string(),
        text: format!(
            "authentication accepted, opening shell for {}@{}",
            session.user, session.host
        ),
    });

        Ok(handle)
    })
    .await
    .context("connection timed out")?
}

fn load_session_private_key(session: &Session) -> Result<PrivateKey> {
    let inline_key = normalize_inline_private_key(&session.private_key_inline);
    let key_path = expand_key_path(session.private_key_path.trim());
    let passphrase = session.passphrase.trim();
    let passphrase = (!passphrase.is_empty()).then_some(passphrase);
    let has_inline = !inline_key.is_empty();
    let has_path = key_path.is_some();

    if !has_inline && !has_path {
        return Err(anyhow!("private key content or path is required"));
    }

    let mut errors = Vec::new();

    if has_inline {
        match decode_secret_key(&inline_key, passphrase) {
            Ok(key) => return Ok(key),
            Err(err) => errors.push(format!("decode private key content: {err}")),
        }
    }

    if let Some(path) = key_path {
        match load_secret_key(path.as_path(), passphrase) {
            Ok(key) => return Ok(key),
            Err(err) => errors.push(format!("load key {}: {err}", path.display())),
        }
    }

    Err(anyhow!(errors.join("; ")))
}

fn expand_key_path(value: &str) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    if value == "~" {
        return BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return BaseDirs::new().map(|dirs| dirs.home_dir().join(rest));
    }
    Some(Path::new(value).to_path_buf())
}

fn key_source_label(session: &Session) -> String {
    let path = session.private_key_path.trim();
    let has_inline = !session.private_key_inline.trim().is_empty();
    match (!path.is_empty(), has_inline) {
        (true, true) => format!("inline key or {}", path),
        (true, false) => path.to_string(),
        (false, true) => "inline key text".to_string(),
        (false, false) => "unknown key source".to_string(),
    }
}

const REMOTE_SYSTEM_PROBE: &str = r#"sh -lc '
os=$(uname -s 2>/dev/null || echo unknown)

if [ "$os" = "Linux" ] && [ -r /proc/stat ]; then
  cpu_stat() { awk '"'"'/^cpu / { print ($2+$3+$4+$5+$6+$7+$8), $5 }'"'"' /proc/stat 2>/dev/null; }
  net_stat() { awk -F"[: ]+" '"'"'/:/ && $1!="Inter" && $1!="face" { rx += $3; tx += $11 } END { print rx+0, tx+0 }'"'"' /proc/net/dev 2>/dev/null; }

  read cpu_total_1 cpu_idle_1 <<EOF
$(cpu_stat)
EOF
  read net_rx_1 net_tx_1 <<EOF
$(net_stat)
EOF
  net_snapshot_1=$(cat /proc/net/dev 2>/dev/null)
  sleep 1
  read cpu_total_2 cpu_idle_2 <<EOF
$(cpu_stat)
EOF
  read net_rx_2 net_tx_2 <<EOF
$(net_stat)
EOF
  net_snapshot_2=$(cat /proc/net/dev 2>/dev/null)

  cpu_delta=$((cpu_total_2 - cpu_total_1))
  idle_delta=$((cpu_idle_2 - cpu_idle_1))
  cpu_percent=$(awk -v total="$cpu_delta" -v idle="$idle_delta" '"'"'BEGIN { if (total <= 0) print "0.00"; else printf "%.2f", ((total-idle)/total)*100 }'"'"')
  mem_total=$(awk '"'"'/^MemTotal:/ {print $2 * 1024}'"'"' /proc/meminfo 2>/dev/null)
  mem_available=$(awk '"'"'/^MemAvailable:/ {print $2 * 1024}'"'"' /proc/meminfo 2>/dev/null)
  swap_total=$(awk '"'"'/^SwapTotal:/ {print $2 * 1024}'"'"' /proc/meminfo 2>/dev/null)
  swap_free=$(awk '"'"'/^SwapFree:/ {print $2 * 1024}'"'"' /proc/meminfo 2>/dev/null)
  os_name=$(awk -F= '"'"'/^PRETTY_NAME=/ { value=$2; gsub(/^"|"$/, "", value); print value; exit }'"'"' /etc/os-release 2>/dev/null)
  cpu_model=$(awk -F: '"'"'/^(model name|Hardware)/ { value=$2; sub(/^[[:space:]]+/, "", value); print value; exit }'"'"' /proc/cpuinfo 2>/dev/null)
  cpu_frequency=$(awk -F: '"'"'/^cpu MHz/ { value=$2; sub(/^[[:space:]]+/, "", value); printf "%.0f", value; exit }'"'"' /proc/cpuinfo 2>/dev/null)
  ip_addresses=$(hostname -I 2>/dev/null | xargs 2>/dev/null)

  echo "OS_NAME=${os_name:-Linux}"
  echo "KERNEL_NAME=$(uname -s 2>/dev/null)"
  echo "KERNEL_VERSION=$(uname -r 2>/dev/null)"
  echo "ARCHITECTURE=$(uname -m 2>/dev/null)"
  echo "HOSTNAME=$(hostname 2>/dev/null)"
  echo "IP_ADDRESS=$(printf "%s\n" "$ip_addresses" | awk '"'"'{print $1}'"'"')"
  echo "IP_ADDRESSES=$ip_addresses"
  ip -o addr show scope global 2>/dev/null | awk '"'"'{ address=$4; sub(/\/.*/, "", address); printf "IP_ENTRY=%s\t%s\n", $2, address }'"'"'
  echo "UPTIME_SECONDS=$(cut -d. -f1 /proc/uptime 2>/dev/null)"
  echo "LOAD_AVERAGE=$(cut -d" " -f1-3 /proc/loadavg 2>/dev/null)"
  echo "CPU_MODEL=${cpu_model:-unknown}"
  echo "CPU_CORES=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 0)"
  echo "CPU_FREQUENCY_MHZ=${cpu_frequency:-0}"
  echo "CPU_PERCENT=${cpu_percent:-0.00}"
  echo "MEM_TOTAL=${mem_total:-0}"
  echo "MEM_USED=$(( ${mem_total:-0} - ${mem_available:-0} ))"
  echo "SWAP_TOTAL=${swap_total:-0}"
  echo "SWAP_USED=$(( ${swap_total:-0} - ${swap_free:-0} ))"
  echo "NET_RX=$(( ${net_rx_2:-0} - ${net_rx_1:-0} ))"
  echo "NET_TX=$(( ${net_tx_2:-0} - ${net_tx_1:-0} ))"
  printf "%s\n__TINY_SHELL_SPLIT__\n%s\n" "$net_snapshot_1" "$net_snapshot_2" | awk -F"[: ]+" '"'"'
    $0 == "__TINY_SHELL_SPLIT__" { second=1; next }
    /:/ && $2 != "Inter" && $2 != "face" {
      name=$2
      if (!second) { rx1[name]=$3; tx1[name]=$11 }
      else { printf "NETIF=%s\t%s\t%s\t%s\t%s\n", name, $3, $11, $3-rx1[name], $11-tx1[name] }
    }'"'"'
  { ps -eo pid=,rss=,pcpu=,comm= --sort=-rss 2>/dev/null | head -n 16; ps -eo pid=,rss=,pcpu=,comm= --sort=-pcpu 2>/dev/null | head -n 16; } | awk '"'"'!seen[$1]++ { pid=$1; mem=$2*1024; cpu=$3; $1=""; $2=""; $3=""; sub(/^[[:space:]]+/, ""); printf "PROCESS=%s\t%s\t%s\t%s\n", pid, mem, cpu, $0 }'"'"'
  df -kP 2>/dev/null | awk "NR > 1 && \$1 !~ /^(tmpfs|devtmpfs|ramfs|overlay|aufs)\$/ { printf \"DISK=%s\t%s\t%s\n\", \$6, \$4 * 1024, \$2 * 1024 }" | head -n 6
  df -kP 2>/dev/null | awk "NR > 1 { printf \"FILESYSTEM=%s\t%s\t%s\n\", \$6, \$4 * 1024, \$2 * 1024 }" | head -n 128
  exit 0
fi

if [ "$os" = "Darwin" ]; then
  net_stat() { netstat -ibn 2>/dev/null | awk '"'"'NR > 1 && $7 ~ /^[0-9]+$/ && $10 ~ /^[0-9]+$/ { rx += $7; tx += $10 } END { print rx+0, tx+0 }'"'"'; }

  read net_rx_1 net_tx_1 <<EOF
$(net_stat)
EOF
  net_snapshot_1=$(netstat -ibn 2>/dev/null)
  sleep 1
  read net_rx_2 net_tx_2 <<EOF
$(net_stat)
EOF
  net_snapshot_2=$(netstat -ibn 2>/dev/null)

  cpu_percent=$(top -l 2 -n 0 -s 1 2>/dev/null | awk -F"[:,% ]+" '"'"'/CPU usage:/ { user=$3; sys=$5 } END { if (user == "" && sys == "") print "0.00"; else printf "%.2f", user + sys }'"'"')
  mem_total=$(sysctl -n hw.memsize 2>/dev/null || echo 0)
  pagesize=$(sysctl -n hw.pagesize 2>/dev/null || echo 4096)
  vm_output=$(vm_stat 2>/dev/null)
  pages_active=$(printf "%s\n" "$vm_output" | awk '"'"'/Pages active/ { gsub("\\.","",$3); print $3+0 }'"'"')
  pages_wired=$(printf "%s\n" "$vm_output" | awk '"'"'/Pages wired down/ { gsub("\\.","",$4); print $4+0 }'"'"')
  pages_compressed=$(printf "%s\n" "$vm_output" | awk '"'"'/Pages occupied by compressor/ { gsub("\\.","",$5); print $5+0 }'"'"')
  pages_speculative=$(printf "%s\n" "$vm_output" | awk '"'"'/Pages speculative/ { gsub("\\.","",$3); print $3+0 }'"'"')
  mem_used=$(( (${pages_active:-0} + ${pages_wired:-0} + ${pages_compressed:-0} + ${pages_speculative:-0}) * ${pagesize:-4096} ))
  swap_line=$(sysctl vm.swapusage 2>/dev/null || true)
  swap_used=$(printf "%s\n" "$swap_line" | awk -F"[= ,]+" '"'"'
    function mult(unit) { return unit=="K"?1024:(unit=="M"?1048576:(unit=="G"?1073741824:(unit=="T"?1099511627776:1))) }
    /used/ { value=$4; unit=substr(value, length(value), 1); sub(/[A-Za-z]+$/, "", value); printf "%.0f", value * mult(unit) }'"'"')
  swap_total=$(printf "%s\n" "$swap_line" | awk -F"[= ,]+" '"'"'
    function mult(unit) { return unit=="K"?1024:(unit=="M"?1048576:(unit=="G"?1073741824:(unit=="T"?1099511627776:1))) }
    /used/ && /free/ { used=$4; free=$8; unit1=substr(used, length(used), 1); unit2=substr(free, length(free), 1); sub(/[A-Za-z]+$/, "", used); sub(/[A-Za-z]+$/, "", free); printf "%.0f", (used * mult(unit1)) + (free * mult(unit2)) }'"'"')
  ip_addresses=$(ifconfig 2>/dev/null | awk '"'"'/inet / && $2 != "127.0.0.1" { printf "%s%s", separator, $2; separator=" " }'"'"')

  echo "OS_NAME=$(sw_vers -productName 2>/dev/null) $(sw_vers -productVersion 2>/dev/null)"
  echo "KERNEL_NAME=$(uname -s 2>/dev/null)"
  echo "KERNEL_VERSION=$(uname -r 2>/dev/null)"
  echo "ARCHITECTURE=$(uname -m 2>/dev/null)"
  echo "HOSTNAME=$(hostname 2>/dev/null)"
  echo "IP_ADDRESS=$(printf "%s\n" "$ip_addresses" | awk '"'"'{print $1}'"'"')"
  echo "IP_ADDRESSES=$ip_addresses"
  ifconfig 2>/dev/null | awk '"'"'
    /^[[:alnum:]][^[:space:]]*:/ { interface=$1; sub(/:$/, "", interface) }
    /inet / && $2 != "127.0.0.1" { printf "IP_ENTRY=%s\t%s\n", interface, $2 }
    /inet6 / && $2 !~ /^fe80:/ && $2 != "::1" { address=$2; sub(/%.*/, "", address); printf "IP_ENTRY=%s\t%s\n", interface, address }'"'"'
  echo "UPTIME_SECONDS=$(sysctl -n kern.boottime 2>/dev/null | awk -F"[=,]" '"'"'{ gsub(/ /, "", $2); print systime()-$2 }'"'"')"
  echo "LOAD_AVERAGE=$(sysctl -n vm.loadavg 2>/dev/null | tr -d "{}")"
  echo "CPU_MODEL=$(sysctl -n machdep.cpu.brand_string 2>/dev/null)"
  echo "CPU_CORES=$(sysctl -n hw.logicalcpu 2>/dev/null || echo 0)"
  echo "CPU_FREQUENCY_MHZ=$(($(sysctl -n hw.cpufrequency 2>/dev/null || echo 0) / 1000000))"
  echo "CPU_PERCENT=${cpu_percent:-0.00}"
  echo "MEM_TOTAL=${mem_total:-0}"
  echo "MEM_USED=${mem_used:-0}"
  echo "SWAP_TOTAL=${swap_total:-0}"
  echo "SWAP_USED=${swap_used:-0}"
  echo "NET_RX=$(( ${net_rx_2:-0} - ${net_rx_1:-0} ))"
  echo "NET_TX=$(( ${net_tx_2:-0} - ${net_tx_1:-0} ))"
  printf "%s\n__TINY_SHELL_SPLIT__\n%s\n" "$net_snapshot_1" "$net_snapshot_2" | awk '"'"'
    $0 == "__TINY_SHELL_SPLIT__" { second=1; next }
    NR > 1 && $7 ~ /^[0-9]+$/ && $10 ~ /^[0-9]+$/ {
      if (!second) { rx1[$1]+=$7; tx1[$1]+=$10 }
      else { rx2[$1]+=$7; tx2[$1]+=$10 }
    }
    END { for (name in rx2) printf "NETIF=%s\t%s\t%s\t%s\t%s\n", name, rx2[name], tx2[name], rx2[name]-rx1[name], tx2[name]-tx1[name] }'"'"'
  { ps -axo pid=,rss=,pcpu=,comm= 2>/dev/null | sort -k2,2nr | head -n 16; ps -axo pid=,rss=,pcpu=,comm= 2>/dev/null | sort -k3,3nr | head -n 16; } | awk '"'"'!seen[$1]++ { pid=$1; mem=$2*1024; cpu=$3; $1=""; $2=""; $3=""; sub(/^[[:space:]]+/, ""); printf "PROCESS=%s\t%s\t%s\t%s\n", pid, mem, cpu, $0 }'"'"'
  df -kP 2>/dev/null | awk "NR > 1 && \$1 !~ /^(devfs|tmpfs|devtmpfs|ramfs|overlay|aufs)\$/ { printf \"DISK=%s\t%s\t%s\n\", \$6, \$4 * 1024, \$2 * 1024 }" | head -n 6
  df -kP 2>/dev/null | awk "NR > 1 { printf \"FILESYSTEM=%s\t%s\t%s\n\", \$6, \$4 * 1024, \$2 * 1024 }" | head -n 128
  exit 0
fi

echo "CPU_PERCENT=0.00"
echo "OS_NAME=${os:-unknown}"
echo "KERNEL_NAME=$(uname -s 2>/dev/null)"
echo "KERNEL_VERSION=$(uname -r 2>/dev/null)"
echo "ARCHITECTURE=$(uname -m 2>/dev/null)"
echo "HOSTNAME=$(hostname 2>/dev/null)"
echo "IP_ADDRESS="
echo "IP_ADDRESSES="
echo "UPTIME_SECONDS=0"
echo "LOAD_AVERAGE="
echo "CPU_MODEL="
echo "CPU_CORES=0"
echo "CPU_FREQUENCY_MHZ=0"
echo "MEM_TOTAL=0"
echo "MEM_USED=0"
echo "SWAP_TOTAL=0"
echo "SWAP_USED=0"
echo "NET_RX=0"
echo "NET_TX=0"
'"#;

#[derive(Clone)]
struct ClientHandler {
    host_key_verifier: HostKeyVerifier,
}

impl ClientHandler {
    fn new(host: &str, port: u16) -> Result<Self> {
        Ok(Self {
            host_key_verifier: HostKeyVerifier::new(host, port)?,
        })
    }
}

#[async_trait]
impl Handler for ClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        self.host_key_verifier.verify(server_public_key).await
    }
}
