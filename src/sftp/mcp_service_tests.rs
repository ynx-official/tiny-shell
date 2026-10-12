//! Real loopback SSH/SFTP transport with an isolated in-memory filesystem and temporary known_hosts.
use super::*;
use russh::{
    Channel, ChannelId,
    server::{self, Auth, Msg, Server as _},
};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode,
};
use std::net::SocketAddr;

#[derive(Clone)]
struct Entry {
    bytes: Vec<u8>,
    permissions: u32,
    modified: u32,
}
type Files = Arc<Mutex<HashMap<String, Entry>>>;
#[derive(Clone)]
struct TestServer {
    files: Files,
    executions: Arc<std::sync::atomic::AtomicUsize>,
}
struct SshHandler {
    server: TestServer,
    channels: HashMap<ChannelId, Channel<Msg>>,
}
impl server::Server for TestServer {
    type Handler = SshHandler;
    fn new_client(&mut self, _address: Option<SocketAddr>) -> Self::Handler {
        SshHandler {
            server: self.clone(),
            channels: HashMap::new(),
        }
    }
}
#[async_trait]
impl server::Handler for SshHandler {
    type Error = anyhow::Error;
    async fn auth_password(&mut self, user: &str, password: &str) -> anyhow::Result<Auth> {
        Ok(if user == "mcp-test" && password == "isolated-test" {
            Auth::Accept
        } else {
            Auth::Reject {
                proceed_with_methods: None,
            }
        })
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut server::Session,
    ) -> anyhow::Result<bool> {
        self.channels.insert(channel.id(), channel);
        Ok(true)
    }
    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> anyhow::Result<()> {
        if name != "sftp" {
            session.channel_failure(id)?;
            return Ok(());
        }
        let channel = self
            .channels
            .remove(&id)
            .ok_or_else(|| anyhow::anyhow!("missing SFTP channel"))?;
        session.channel_success(id)?;
        let handler = Filesystem {
            files: self.server.files.clone(),
            listed: false,
        };
        tokio::spawn(async move {
            russh_sftp::server::run(channel.into_stream(), handler).await;
        });
        Ok(())
    }
    async fn exec_request(
        &mut self,
        id: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> anyhow::Result<()> {
        self.server
            .executions
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        session.channel_success(id)?;
        if data == b"wait" {
            return Ok(());
        }
        session.data(id, b"loopback stdout\n".as_slice().into())?;
        session.extended_data(id, 1, b"loopback stderr\n".as_slice().into())?;
        session.exit_status_request(id, if data == b"fail" { 7 } else { 0 })?;
        session.eof(id)?;
        session.close(id)?;
        Ok(())
    }
}
struct Filesystem {
    files: Files,
    listed: bool,
}
fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: String::new(),
        language_tag: String::new(),
    }
}
fn attrs(entry: &Entry) -> FileAttributes {
    let mut attributes = FileAttributes::empty();
    attributes.size = Some(entry.bytes.len() as u64);
    attributes.permissions = Some(entry.permissions);
    attributes.mtime = Some(entry.modified);
    attributes
}
impl russh_sftp::server::Handler for Filesystem {
    type Error = StatusCode;
    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }
    async fn stat(&mut self, id: u32, path: String) -> std::result::Result<Attrs, StatusCode> {
        let files = self.files.lock().map_err(|_| StatusCode::Failure)?;
        let entry = files.get(&path).ok_or(StatusCode::NoSuchFile)?;
        Ok(Attrs {
            id,
            attrs: attrs(entry),
        })
    }
    async fn lstat(&mut self, id: u32, path: String) -> std::result::Result<Attrs, StatusCode> {
        self.stat(id, path).await
    }
    async fn fstat(&mut self, id: u32, handle: String) -> std::result::Result<Attrs, StatusCode> {
        self.stat(id, handle).await
    }
    async fn open(
        &mut self,
        id: u32,
        path: String,
        flags: OpenFlags,
        _attrs: FileAttributes,
    ) -> std::result::Result<Handle, StatusCode> {
        let mut files = self.files.lock().map_err(|_| StatusCode::Failure)?;
        if flags.contains(OpenFlags::EXCLUDE) && files.contains_key(&path) {
            return Err(StatusCode::Failure);
        }
        if flags.contains(OpenFlags::CREATE) {
            files.entry(path.clone()).or_insert(Entry {
                bytes: Vec::new(),
                permissions: 0o100644,
                modified: 1,
            });
        }
        let entry = files.get_mut(&path).ok_or(StatusCode::NoSuchFile)?;
        if flags.contains(OpenFlags::TRUNCATE) {
            entry.bytes.clear();
        }
        Ok(Handle { id, handle: path })
    }
    async fn close(&mut self, id: u32, _handle: String) -> std::result::Result<Status, StatusCode> {
        Ok(ok(id))
    }
    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> std::result::Result<Data, StatusCode> {
        let files = self.files.lock().map_err(|_| StatusCode::Failure)?;
        let entry = files.get(&handle).ok_or(StatusCode::NoSuchFile)?;
        let start = (offset as usize).min(entry.bytes.len());
        if start == entry.bytes.len() {
            return Err(StatusCode::Eof);
        }
        Ok(Data {
            id,
            data: entry.bytes[start..start.saturating_add(len as usize).min(entry.bytes.len())]
                .to_vec(),
        })
    }
    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> std::result::Result<Status, StatusCode> {
        let mut files = self.files.lock().map_err(|_| StatusCode::Failure)?;
        let entry = files.get_mut(&handle).ok_or(StatusCode::NoSuchFile)?;
        let start = offset as usize;
        entry
            .bytes
            .resize(entry.bytes.len().max(start + data.len()), 0);
        entry.bytes[start..start + data.len()].copy_from_slice(&data);
        entry.modified += 1;
        Ok(ok(id))
    }
    async fn setstat(
        &mut self,
        id: u32,
        path: String,
        attributes: FileAttributes,
    ) -> std::result::Result<Status, StatusCode> {
        let mut files = self.files.lock().map_err(|_| StatusCode::Failure)?;
        let entry = files.get_mut(&path).ok_or(StatusCode::NoSuchFile)?;
        if let Some(permissions) = attributes.permissions {
            entry.permissions = permissions;
        }
        Ok(ok(id))
    }
    async fn rename(
        &mut self,
        id: u32,
        old: String,
        new: String,
    ) -> std::result::Result<Status, StatusCode> {
        let mut files = self.files.lock().map_err(|_| StatusCode::Failure)?;
        let entry = files.remove(&old).ok_or(StatusCode::NoSuchFile)?;
        files.insert(new, entry);
        Ok(ok(id))
    }
    async fn remove(&mut self, id: u32, path: String) -> std::result::Result<Status, StatusCode> {
        self.files
            .lock()
            .map_err(|_| StatusCode::Failure)?
            .remove(&path)
            .ok_or(StatusCode::NoSuchFile)?;
        Ok(ok(id))
    }
    async fn opendir(&mut self, id: u32, _path: String) -> std::result::Result<Handle, StatusCode> {
        self.listed = false;
        Ok(Handle {
            id,
            handle: "/".into(),
        })
    }
    async fn readdir(&mut self, id: u32, _handle: String) -> std::result::Result<Name, StatusCode> {
        if self.listed {
            return Err(StatusCode::Eof);
        }
        self.listed = true;
        let files = self.files.lock().map_err(|_| StatusCode::Failure)?;
        Ok(Name {
            id,
            files: files
                .iter()
                .filter(|(name, _)| name.as_str() != "/")
                .map(|(path, e)| File::new(path.trim_start_matches('/'), attrs(e)))
                .collect(),
        })
    }
}
fn call(name: &str, args: Value) -> anyhow::Result<Call> {
    Call::parse(name, args).map_err(|e| anyhow::anyhow!(e.code))
}
fn failure(error: Failure) -> anyhow::Error {
    anyhow::anyhow!(error.code)
}
#[tokio::test]
async fn real_ssh_commands_and_sftp_conflicts_preserve_remote_data() -> anyhow::Result<()> {
    let workspace = crate::session::config::TempWorkspace::initialize_in(
        std::env::temp_dir().join("tiny-shell-mcp-tests"),
    )?;
    let files = Arc::new(Mutex::new(HashMap::from([
        (
            "/".into(),
            Entry {
                bytes: Vec::new(),
                permissions: 0o40755,
                modified: 1,
            },
        ),
        (
            "/text".into(),
            Entry {
                bytes: b"first\r\n".to_vec(),
                permissions: 0o100640,
                modified: 1,
            },
        ),
        (
            "/link".into(),
            Entry {
                bytes: b"/text".to_vec(),
                permissions: 0o120777,
                modified: 1,
            },
        ),
    ])));
    let executions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = socket.local_addr()?.port();
    let server_config = Arc::new(server::Config {
        keys: vec![russh::keys::PrivateKey::random(
            &mut rand::thread_rng(),
            ssh_key::Algorithm::Ed25519,
        )?],
        auth_rejection_time: Duration::ZERO,
        ..Default::default()
    });
    let mut server = TestServer {
        files: files.clone(),
        executions: executions.clone(),
    };
    let listener = tokio::spawn(async move { server.run_on_socket(server_config, &socket).await });
    let mut config = ConfigStore::in_memory();
    let saved = Session::password(
        "127.0.0.1".into(),
        port,
        "mcp-test".into(),
        "isolated-test".into(),
    );
    config.upsert(saved.clone());
    let selected = target(&saved, &config);
    let mut remote = McpRemote::new();
    remote.test_config = Some(config);
    remote.test_known_hosts = Some(workspace.path().join("known_hosts"));
    let remote = Arc::new(remote);
    let session = uuid::Uuid::new_v4().to_string();
    let request = uuid::Uuid::new_v4().to_string();
    let (_cancel, rx) = watch::channel(false);
    remote
        .execute(
            &selected,
            &session,
            &call(
                "open_session",
                json!({"connection_id":selected.id,"request_id":request}),
            )?,
            rx.clone(),
        )
        .await
        .map_err(failure)?;
    assert_eq!(executions.load(std::sync::atomic::Ordering::SeqCst), 0);
    let command = call(
        "run_command",
        json!({"session_id":session,"command":"fail","request_id":request}),
    )?;
    let output = remote
        .execute(&selected, &session, &command, rx.clone())
        .await
        .map_err(failure)?;
    assert_eq!(output["exit_code"], 7);
    assert_eq!(output["stdout"], "loopback stdout\n");
    assert_eq!(output["stderr"], "loopback stderr\n");
    let read_call = call("read_file", json!({"session_id":session,"path":"/text"}))?;
    let file = remote
        .execute(&selected, &session, &read_call, rx.clone())
        .await
        .map_err(failure)?;
    assert_eq!(file["content"], "first\n");
    let directory = remote
        .execute(
            &selected,
            &session,
            &call("list_directory", json!({"session_id":session,"path":"/"}))?,
            rx.clone(),
        )
        .await
        .map_err(failure)?;
    assert!(directory["entries"].as_array().is_some_and(|entries| {
        entries
            .iter()
            .any(|e| e["name"] == "link" && e["type"] == "symlink")
    }));
    let unsupported = call(
        "run_command",
        json!({"session_id":session,"command":"pwd","cwd":"/tmp","request_id":request}),
    )?;
    let before = executions.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        remote
            .execute(&selected, &session, &unsupported, rx.clone())
            .await
            .err()
            .map(|e| e.code),
        Some("UNSUPPORTED_SHELL".into())
    );
    assert_eq!(executions.load(std::sync::atomic::Ordering::SeqCst), before);
    let version = file["version"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing version"))?;
    let update = call(
        "write_file",
        json!({"session_id":session,"path":"/text","mode":"update","content":"second\n","expected_version":version,"request_id":request}),
    )?;
    remote
        .prepare(&selected, &session, &update)
        .await
        .map_err(failure)?;
    remote
        .execute(&selected, &session, &update, rx.clone())
        .await
        .map_err(failure)?;
    {
        let entries = files.lock().map_err(|_| anyhow::anyhow!("files lock"))?;
        assert_eq!(entries["/text"].bytes, b"second\r\n");
        assert_eq!(entries["/text"].permissions, 0o100640);
    }
    assert_eq!(
        remote
            .execute(&selected, &session, &update, rx.clone())
            .await
            .err()
            .map(|e| e.code),
        Some("FILE_CONFLICT".into())
    );
    let create = call(
        "write_file",
        json!({"session_id":session,"path":"/new","mode":"create","content":"created\n","request_id":request}),
    )?;
    remote
        .execute(&selected, &session, &create, rx.clone())
        .await
        .map_err(failure)?;
    assert_eq!(
        remote
            .execute(&selected, &session, &create, rx.clone())
            .await
            .err()
            .map(|e| e.code),
        Some("FILE_CONFLICT".into())
    );
    let link = call(
        "write_file",
        json!({"session_id":session,"path":"/link","mode":"create","content":"blocked","request_id":request}),
    )?;
    assert_eq!(
        remote
            .execute(&selected, &session, &link, rx.clone())
            .await
            .err()
            .map(|e| e.code),
        Some("SYMLINK_NOT_ALLOWED".into())
    );
    let timeout = call(
        "run_command",
        json!({"session_id":session,"command":"wait","request_id":request,"timeout_ms":1000}),
    )?;
    assert_eq!(
        remote
            .execute(&selected, &session, &timeout, rx)
            .await
            .err()
            .map(|e| e.code),
        Some("TIMEOUT".into())
    );
    remote.close(&session).await;
    listener.abort();
    let _ = listener.await;
    Ok(())
}
