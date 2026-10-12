//! MCP uses independent SSH channels and reuses the existing authentication and text codecs.
use super::{
    SftpClientHandler,
    text_file::{self, RemoteTextFile, RemoteTextSave, SaveRemoteTextOutcome},
};
use crate::session::config::{ConfigStore, ConnectionType, Session};
use async_trait::async_trait;
use russh::{ChannelMsg, Disconnect};
use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tiny_shell_mcp::{
    contracts::{Call, Failure, MAX_OUTPUT, MAX_TEXT, Result, WriteMode},
    engine::{Remote, Target},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
};

type Ssh = Arc<russh::client::Handle<SftpClientHandler>>;
#[cfg(test)]
#[path = "mcp_service_tests.rs"]
mod tests;
struct Connection {
    ssh: Ssh,
    sftp: Arc<SftpSession>,
    write: tokio::sync::Mutex<()>,
}
pub(crate) struct McpRemote {
    sessions: Mutex<HashMap<String, Arc<Connection>>>,
    credentials: Mutex<HashMap<String, Session>>,
    #[cfg(test)]
    test_config: Option<ConfigStore>,
    #[cfg(test)]
    test_known_hosts: Option<std::path::PathBuf>,
}
impl McpRemote {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            credentials: Mutex::new(HashMap::new()),
            #[cfg(test)]
            test_config: None,
            #[cfg(test)]
            test_known_hosts: None,
        }
    }
    pub fn credentials(&self, id: String, session: Session) -> Result<()> {
        self.credentials
            .lock()
            .map_err(|_| Failure::new("INTERNAL_ERROR"))?
            .insert(id, session);
        Ok(())
    }
    pub fn discard_credentials(&self, id: &str) {
        if let Ok(mut credentials) = self.credentials.lock() {
            credentials.remove(id);
        }
    }
    async fn config(&self) -> Result<ConfigStore> {
        #[cfg(test)]
        if let Some(config) = &self.test_config {
            return Ok(config.clone());
        }
        tokio::task::spawn_blocking(ConfigStore::load)
            .await
            .map_err(|_| Failure::new("CONFIG_ERROR"))?
            .map_err(|_| Failure::new("CONFIG_ERROR"))
    }
    fn connection(&self, id: &str) -> Result<Arc<Connection>> {
        self.sessions
            .lock()
            .map_err(|_| Failure::new("INTERNAL_ERROR"))?
            .get(id)
            .cloned()
            .ok_or_else(|| Failure::new("SESSION_NOT_FOUND"))
    }
}
pub(crate) fn target(session: &Session, config: &ConfigStore) -> Target {
    let key = session
        .managed_key_id
        .as_deref()
        .and_then(|id| config.get_managed_key(id))
        .map(|k| (&k.inline_content, &k.passphrase));
    let fields = json!([
        session.host,
        session.port,
        session.user,
        session.auth,
        session.password,
        session.private_key_path,
        session.private_key_inline,
        session.passphrase,
        session.managed_key_id,
        key,
        session.proxy_type,
        session.proxy_host,
        session.proxy_port,
        session.proxy_user,
        session.proxy_password,
        config.global_proxy_type(),
        config.global_proxy_host(),
        config.global_proxy_port(),
        config.global_proxy_user(),
        config.global_proxy_password()
    ]);
    Target {
        id: session.id.clone(),
        name: session.name.clone(),
        group: session.group.clone(),
        host: session.host.clone(),
        port: session.port,
        user: session.user.clone(),
        digest: hex::encode(Sha256::digest(fields.to_string().as_bytes())),
    }
}
fn revision(file: &RemoteTextFile) -> String {
    hex::encode(Sha256::digest(
        format!(
            "{}:{:?}:{:?}",
            file.revision.sha256, file.revision.modified, file.revision.permissions
        )
        .as_bytes(),
    ))
}
async fn no_links(sftp: &SftpSession, path: &str, allow_missing: bool) -> Result<()> {
    let pieces = path
        .split('/')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    if pieces.iter().any(|p| matches!(*p, "." | "..")) {
        return Err(Failure::new("INVALID_ARGUMENT"));
    }
    let mut current = String::new();
    for (i, piece) in pieces.iter().enumerate() {
        current.push('/');
        current.push_str(piece);
        match sftp.symlink_metadata(&current).await {
            Ok(metadata) if metadata.is_symlink() => {
                return Err(Failure::new("SYMLINK_NOT_ALLOWED"));
            }
            Ok(_) => {}
            Err(_)
                if allow_missing
                    && i + 1 == pieces.len()
                    && matches!(sftp.try_exists(&current).await, Ok(false)) => {}
            Err(_) => return Err(Failure::new("FILE_ERROR")),
        }
    }
    Ok(())
}
async fn read(sftp: &SftpSession, path: &str) -> Result<RemoteTextFile> {
    let meta = sftp
        .metadata(path)
        .await
        .map_err(|_| Failure::new("FILE_ERROR"))?;
    if !meta.is_regular() || meta.size.is_some_and(|s| s > MAX_TEXT as u64) {
        return Err(Failure::new("FILE_LIMIT"));
    }
    let file = sftp
        .open(path)
        .await
        .map_err(|_| Failure::new("FILE_ERROR"))?;
    let mut bytes = Vec::new();
    file.take(MAX_TEXT as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| Failure::new("FILE_ERROR"))?;
    if bytes.len() > MAX_TEXT {
        return Err(Failure::new("FILE_LIMIT"));
    }
    text_file::decode_remote_text(bytes, meta.mtime, meta.permissions)
        .map_err(|_| Failure::new("UNSUPPORTED_ENCODING"))
}
#[async_trait]
impl Remote for McpRemote {
    fn app_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    async fn targets(&self) -> Result<Vec<Target>> {
        let config = self.config().await?;
        Ok(config
            .sessions()
            .iter()
            .filter(|s| s.connection_type == ConnectionType::Ssh)
            .map(|s| target(s, &config))
            .collect())
    }
    async fn prepare(&self, _target: &Target, session: &str, call: &Call) -> Result<Value> {
        if let Call::WriteFile(p) = call {
            let connection = self.connection(session)?;
            no_links(&connection.sftp, &p.path, p.mode == WriteMode::Create).await?;
            if p.mode == WriteMode::Update {
                let file = read(&connection.sftp, &p.path).await?;
                if p.expected_version.as_deref() != Some(&revision(&file)) {
                    return Err(Failure::new("FILE_CONFLICT"));
                }
                return Ok(json!({"old_content":file.content,"new_content":p.content}));
            }
            if connection
                .sftp
                .try_exists(&p.path)
                .await
                .map_err(|_| Failure::new("FILE_ERROR"))?
            {
                return Err(Failure::new("FILE_CONFLICT"));
            }
            return Ok(json!({"old_content":"","new_content":p.content}));
        }
        Ok(json!({}))
    }
    async fn execute(
        &self,
        selected: &Target,
        id: &str,
        call: &Call,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<Value> {
        // Revalidate the actual persisted target at the last service boundary.
        let config = self.config().await?;
        let original = config
            .get(&selected.id)
            .filter(|s| {
                s.connection_type == ConnectionType::Ssh
                    && target(s, &config).digest == selected.digest
            })
            .cloned()
            .ok_or_else(|| Failure::new("CONNECTION_NOT_ALLOWED"))?;
        if *cancel.borrow() {
            return Err(Failure::new("OUTCOME_UNKNOWN"));
        }
        if matches!(call, Call::OpenSession(_)) {
            let provided = self
                .credentials
                .lock()
                .map_err(|_| Failure::new("INTERNAL_ERROR"))?
                .remove(id);
            let mut session = original;
            if let Some(provided) = provided {
                // Credential UI may supply authentication fields, never a new host/user/proxy target.
                session.password = provided.password;
                session.passphrase = provided.passphrase;
                session.auth = provided.auth;
                session.managed_key_id = provided.managed_key_id;
                session.private_key_inline = provided.private_key_inline;
                session.private_key_path = provided.private_key_path;
            }
            if let Some(key) = session
                .managed_key_id
                .as_deref()
                .and_then(|k| config.get_managed_key(k))
            {
                session.private_key_inline = key.inline_content.clone();
                session.passphrase = key.passphrase.clone();
                session.private_key_path.clear();
            }
            if session.requires_credential_prompt() {
                return Err(Failure::new("AUTH_REQUIRED"));
            }
            let handler = SftpClientHandler::new(&session.host, session.port)
                .map_err(|_| Failure::new("SSH_CONNECT_FAILED"))?;
            #[cfg(test)]
            let handler = if let Some(path) = &self.test_known_hosts {
                SftpClientHandler::in_path(&session.host, session.port, path.clone())
                    .map_err(|_| Failure::new("SSH_CONNECT_FAILED"))?
            } else {
                handler
            };
            let ssh = super::connection::connect_with_handler(&session, &config, handler)
                .await
                .map_err(|_| Failure::new("SSH_CONNECT_FAILED"))?;
            let channel = ssh
                .channel_open_session()
                .await
                .map_err(|_| Failure::new("SSH_CONNECT_FAILED"))?;
            channel
                .request_subsystem(true, "sftp")
                .await
                .map_err(|_| Failure::new("SFTP_UNAVAILABLE"))?;
            let sftp = SftpSession::new(channel.into_stream())
                .await
                .map_err(|_| Failure::new("SFTP_UNAVAILABLE"))?;
            self.sessions
                .lock()
                .map_err(|_| Failure::new("INTERNAL_ERROR"))?
                .insert(
                    id.to_owned(),
                    Arc::new(Connection {
                        ssh,
                        sftp: Arc::new(sftp),
                        write: tokio::sync::Mutex::new(()),
                    }),
                );
            return Ok(json!({"session_id":id}));
        }
        if matches!(call, Call::CloseSession(_)) {
            self.close(id).await;
            return Ok(json!({"closed":true}));
        }
        let connection = self.connection(id)?;
        match call {
            Call::RunCommand(p) => {
                if p.cwd.is_some() {
                    return Err(Failure::new("UNSUPPORTED_SHELL"));
                }
                let mut channel = connection
                    .ssh
                    .channel_open_session()
                    .await
                    .map_err(|_| Failure::new("SSH_ERROR"))?;
                channel
                    .exec(true, p.command.as_bytes())
                    .await
                    .map_err(|_| Failure::new("OUTCOME_UNKNOWN"))?;
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                let mut exit_code = None;
                let mut truncated = false;
                let deadline = tokio::time::sleep(Duration::from_millis(p.timeout_ms));
                tokio::pin!(deadline);
                loop {
                    let message = tokio::select! {
                        message=channel.wait()=>message,
                        _=cancel.changed()=>{let _=channel.close().await;return Err(Failure::new("OUTCOME_UNKNOWN").with_result(json!({"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr),"exit_code":exit_code,"truncated":truncated})));},
                        _=&mut deadline=>{let _=channel.close().await;return Err(Failure::new("TIMEOUT").with_result(json!({"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr),"exit_code":exit_code,"truncated":truncated})));},
                    };
                    match message {
                        Some(ChannelMsg::Data { data }) => {
                            let left = MAX_OUTPUT.saturating_sub(stdout.len() + stderr.len());
                            truncated |= data.len() > left;
                            stdout.extend_from_slice(&data[..data.len().min(left)]);
                        }
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            let left = MAX_OUTPUT.saturating_sub(stdout.len() + stderr.len());
                            truncated |= data.len() > left;
                            stderr.extend_from_slice(&data[..data.len().min(left)]);
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            exit_code = Some(exit_status)
                        }
                        Some(ChannelMsg::Close) | None => break,
                        _ => {}
                    }
                }
                if exit_code.is_none() {
                    return Err(Failure::new("OUTCOME_UNKNOWN"));
                }
                Ok(
                    json!({"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr),"exit_code":exit_code,"truncated":truncated}),
                )
            }
            Call::ListDirectory(p) => {
                // High-level read_dir eagerly collects entries; a raw channel enforces our cap while reading.
                let channel = connection
                    .ssh
                    .channel_open_session()
                    .await
                    .map_err(|_| Failure::new("FILE_ERROR"))?;
                channel
                    .request_subsystem(true, "sftp")
                    .await
                    .map_err(|_| Failure::new("SFTP_UNAVAILABLE"))?;
                let raw = russh_sftp::client::RawSftpSession::new(channel.into_stream());
                raw.init().await.map_err(|_| Failure::new("FILE_ERROR"))?;
                let directory = raw
                    .opendir(&p.path)
                    .await
                    .map_err(|_| Failure::new("FILE_ERROR"))?
                    .handle;
                let mut entries = Vec::new();
                let mut size = 0;
                let result = async {
                    loop {
                        match raw.readdir(&directory).await {
                            Ok(batch) => {
                                if batch.files.is_empty() {
                                    return Err(Failure::new("DIRECTORY_LIMIT"));
                                }
                                for file in batch
                                    .files
                                    .into_iter()
                                    .filter(|f| !matches!(f.filename.as_str(), "." | ".."))
                                {
                                    size += file.filename.len();
                                    if entries.len() >= 10000 || size > MAX_OUTPUT {
                                        return Err(Failure::new("DIRECTORY_LIMIT"));
                                    }
                                    entries.push(file);
                                }
                            }
                            Err(russh_sftp::client::error::Error::Status(status))
                                if status.status_code == russh_sftp::protocol::StatusCode::Eof =>
                            {
                                break;
                            }
                            Err(_) => return Err(Failure::new("FILE_ERROR")),
                        }
                    }
                    Ok(())
                }
                .await;
                let _ = raw.close(directory).await;
                let _ = raw.close_session();
                result?;
                entries.sort_by(|a, b| a.filename.cmp(&b.filename));
                let page=entries.iter().skip(p.offset).take(p.limit).map(|e|{let meta=&e.attrs;json!({"name":e.filename,"type":if meta.is_symlink(){"symlink"}else if meta.is_dir(){"directory"}else{"file"},"size":meta.size,"modified":meta.mtime})}).collect::<Vec<_>>();
                Ok(
                    json!({"entries":page,"next_offset":(p.offset.saturating_add(p.limit)<entries.len()).then_some(p.offset.saturating_add(p.limit))}),
                )
            }
            Call::ReadFile(p) => {
                let file = read(&connection.sftp, &p.path).await?;
                Ok(
                    json!({"content":file.content,"encoding":file.format.encoding.label(),"line_ending":format!("{:?}",file.format.line_ending),"version":revision(&file),"size":file.revision.size}),
                )
            }
            Call::WriteFile(p) => {
                let _saving = connection.write.lock().await;
                no_links(&connection.sftp, &p.path, p.mode == WriteMode::Create).await?;
                if p.mode == WriteMode::Create {
                    let content = p.content.replace("\r\n", "\n");
                    let mut file = connection
                        .sftp
                        .open_with_flags(
                            &p.path,
                            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                        )
                        .await
                        .map_err(|_| Failure::new("FILE_CONFLICT"))?;
                    file.write_all(content.as_bytes())
                        .await
                        .map_err(|_| Failure::new("OUTCOME_UNKNOWN"))?;
                    file.flush()
                        .await
                        .map_err(|_| Failure::new("OUTCOME_UNKNOWN"))?;
                    drop(file);
                } else {
                    let file = read(&connection.sftp, &p.path).await?;
                    if p.expected_version.as_deref() != Some(&revision(&file)) {
                        return Err(Failure::new("FILE_CONFLICT"));
                    }
                    let bytes = text_file::encode_remote_text(&p.content, file.format)
                        .map_err(|_| Failure::new("UNSUPPORTED_ENCODING"))?;
                    if bytes.len() > MAX_TEXT {
                        return Err(Failure::new("FILE_LIMIT"));
                    }
                    let save = RemoteTextSave {
                        remote_path: p.path.clone(),
                        content: p.content.clone(),
                        format: file.format,
                        expected_revision: file.revision,
                        force: false,
                    };
                    // Do not abort during replacement: existing save performs rollback and read-back validation.
                    match text_file::save_remote_text_file_limited(
                        &connection.sftp,
                        save,
                        MAX_TEXT as u64,
                        true,
                    )
                    .await
                    .map_err(|_| Failure::new("OUTCOME_UNKNOWN"))?
                    {
                        SaveRemoteTextOutcome::Conflict(_) => {
                            return Err(Failure::new("FILE_CONFLICT"));
                        }
                        SaveRemoteTextOutcome::Saved(_) => {}
                    }
                }
                let file = read(&connection.sftp, &p.path)
                    .await
                    .map_err(|_| Failure::new("OUTCOME_UNKNOWN"))?;
                if file.content != p.content.replace("\r\n", "\n") {
                    return Err(Failure::new("OUTCOME_UNKNOWN"));
                }
                Ok(json!({"version":revision(&file),"size":file.revision.size}))
            }
            _ => Err(Failure::new("INVALID_ARGUMENT")),
        }
    }
    async fn close(&self, id: &str) {
        let connection = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(id));
        if let Some(connection) = connection {
            let _saving = connection.write.lock().await;
            let _ = connection
                .ssh
                .disconnect(Disconnect::ByApplication, "MCP session closed", "")
                .await;
        }
    }
}
