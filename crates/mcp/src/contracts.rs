use rmcp::model::{Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub const MAX_TEXT: usize = 256 * 1024;
pub const MAX_OUTPUT: usize = 1024 * 1024;
pub const MAX_FRAME: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Failure {
    pub code: String,
    pub message: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub result: Option<Value>,
}
impl Failure {
    pub fn new(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: match code {
                "APP_NOT_RUNNING" => "Start TinyShell and enable MCP.",
                "MCP_DISABLED" => "MCP access is disabled in TinyShell.",
                "CONNECTION_NOT_ALLOWED" => {
                    "The connection is not authorized or its target changed."
                }
                "PERMISSION_DENIED" => "The current permission mode does not allow this operation.",
                "AUTH_REQUIRED" => "Provide credentials in TinyShell.",
                "FILE_CONFLICT" => "Read the current file and review changes before retrying.",
                "OUTCOME_UNKNOWN" => "The remote outcome is unknown. Check it before retrying.",
                _ => "The operation could not be completed. Review the error code in TinyShell.",
            }
            .to_owned(),
            result: None,
        }
    }
    pub fn with_result(mut self, result: Value) -> Self {
        self.result = Some(result);
        self
    }
    pub fn envelope(&self) -> Value {
        json!({"ok":false,"error":self})
    }
}
pub type Result<T> = std::result::Result<T, Failure>;
pub fn success(data: Value) -> Value {
    json!({"ok":true,"data":data})
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Connections {
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "page_size")]
    #[schemars(range(min = 1, max = 100))]
    pub limit: usize,
}
fn page_size() -> usize {
    50
}
fn output_size() -> usize {
    65536
}
fn timeout() -> u64 {
    30000
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenSession {
    pub connection_id: String,
    pub request_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseSession {
    pub session_id: String,
    pub request_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunCommand {
    pub session_id: String,
    pub command: String,
    pub request_id: String,
    pub cwd: Option<String>,
    #[serde(default = "timeout")]
    #[schemars(range(min = 1000, max = 300000))]
    pub timeout_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTask {
    pub task_id: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "output_size")]
    #[schemars(range(min = 4, max = 65536))]
    pub limit: usize,
    #[serde(default)]
    #[schemars(range(min = 0, max = 10000))]
    pub wait_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelTask {
    pub task_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Directory {
    pub session_id: String,
    pub path: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "page_size")]
    pub limit: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadFile {
    pub session_id: String,
    pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    Create,
    Update,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteFile {
    pub session_id: String,
    pub path: String,
    pub content: String,
    pub mode: WriteMode,
    pub expected_version: Option<String>,
    pub request_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "tool", content = "arguments", rename_all = "snake_case")]
pub enum Call {
    GetStatus(Empty),
    ListConnections(Connections),
    OpenSession(OpenSession),
    CloseSession(CloseSession),
    RunCommand(RunCommand),
    GetTask(GetTask),
    CancelTask(CancelTask),
    ListDirectory(Directory),
    ReadFile(ReadFile),
    WriteFile(WriteFile),
}
pub fn valid_id(id: &str) -> Result<()> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| Failure::new("INVALID_ARGUMENT"))
}
pub fn valid_path(path: &str) -> Result<()> {
    if !path.starts_with('/') || path.contains('\0') || path.len() > 4096 {
        return Err(Failure::new("INVALID_ARGUMENT"));
    }
    Ok(())
}
impl Call {
    pub fn parse(name: &str, arguments: Value) -> Result<Self> {
        let call: Self = serde_json::from_value(json!({"tool":name,"arguments":arguments}))
            .map_err(|_| Failure::new("INVALID_ARGUMENT"))?;
        let invalid = || Failure::new("INVALID_ARGUMENT");
        match &call {
            Self::GetStatus(_) => {}
            Self::ListConnections(p) => {
                if p.limit == 0 || p.limit > 100 || p.search.len() > 256 {
                    return Err(invalid());
                }
            }
            Self::OpenSession(p) => {
                valid_id(&p.connection_id)?;
                valid_id(&p.request_id)?;
            }
            Self::CloseSession(p) => {
                valid_id(&p.session_id)?;
                valid_id(&p.request_id)?;
            }
            Self::RunCommand(p) => {
                valid_id(&p.session_id)?;
                valid_id(&p.request_id)?;
                if p.command.trim().is_empty()
                    || p.command.len() > 65536
                    || p.command.contains('\0')
                    || !(1000..=300000).contains(&p.timeout_ms)
                {
                    return Err(invalid());
                }
                if let Some(cwd) = &p.cwd {
                    valid_path(cwd)?;
                }
            }
            Self::GetTask(p) => {
                valid_id(&p.task_id)?;
                if p.limit < 4 || p.limit > 65536 || p.wait_ms > 10000 {
                    return Err(invalid());
                }
            }
            Self::CancelTask(p) => valid_id(&p.task_id)?,
            Self::ListDirectory(p) => {
                valid_id(&p.session_id)?;
                valid_path(&p.path)?;
                if p.limit == 0 || p.limit > 100 {
                    return Err(invalid());
                }
            }
            Self::ReadFile(p) => {
                valid_id(&p.session_id)?;
                valid_path(&p.path)?;
            }
            Self::WriteFile(p) => {
                valid_id(&p.session_id)?;
                valid_id(&p.request_id)?;
                valid_path(&p.path)?;
                if p.content.len() > MAX_TEXT
                    || p.content.contains('\0')
                    || (p.mode == WriteMode::Update
                        && p.expected_version
                            .as_ref()
                            .is_none_or(|s| s.len() != 64 || hex::decode(s).is_err()))
                    || (p.mode == WriteMode::Create && p.expected_version.is_some())
                {
                    return Err(invalid());
                }
            }
        }
        Ok(call)
    }
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::CloseSession(p) => Some(&p.session_id),
            Self::RunCommand(p) => Some(&p.session_id),
            Self::ListDirectory(p) => Some(&p.session_id),
            Self::ReadFile(p) => Some(&p.session_id),
            Self::WriteFile(p) => Some(&p.session_id),
            _ => None,
        }
    }
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::OpenSession(p) => Some(&p.request_id),
            Self::CloseSession(p) => Some(&p.request_id),
            Self::RunCommand(p) => Some(&p.request_id),
            Self::WriteFile(p) => Some(&p.request_id),
            _ => None,
        }
    }
    pub fn mutation(&self) -> bool {
        matches!(self, Self::RunCommand(_) | Self::WriteFile(_))
    }
}
fn tool<T: JsonSchema>(name: &'static str, description: &'static str, read: bool) -> Tool {
    let mut tool = Tool::new(
        name,
        description,
        schemars::schema_for!(T)
            .as_object()
            .cloned()
            .unwrap_or_default(),
    );
    tool.output_schema=Some(std::sync::Arc::new(json!({"type":"object","properties":{"ok":{"type":"boolean"},"data":{"type":"object"},"error":{"type":"object","properties":{"code":{"type":"string"},"message":{"type":"string"}},"required":["code","message"]}},"required":["ok"],"additionalProperties":false}).as_object().cloned().unwrap_or_default()));
    tool.annotations = Some(
        ToolAnnotations::new()
            .read_only(read)
            .destructive(!read)
            .open_world(true),
    );
    tool
}
pub fn tools() -> Vec<Tool> {
    vec![
        tool::<Empty>(
            "get_status",
            "Get TinyShell MCP status and limits once when needed; avoid repeated status checks during a task.",
            true,
        ),
        tool::<Connections>(
            "list_connections",
            "List only authorized saved SSH connections. Reuse the selected connection ID instead of listing again for each operation.",
            true,
        ),
        tool::<OpenSession>(
            "open_session",
            "Open a dedicated AI SSH session; returns a task ticket. Await get_task with wait_ms=10000, then reuse result.session_id for subsequent operations on the same connection.",
            false,
        ),
        tool::<CloseSession>(
            "close_session",
            "Close your AI session when finished using the connection; never closes a human terminal. Returns a task ticket; await get_task with wait_ms=10000.",
            false,
        ),
        tool::<RunCommand>(
            "run_command",
            "Run a non-interactive remote command; returns a task ticket. Await get_task with wait_ms=10000 for completion. Controlled mode requires TinyShell approval. Never automatically repeat an unknown outcome.",
            false,
        ),
        tool::<GetTask>(
            "get_task",
            "Read your task state and bounded output. Set wait_ms up to 10000 to wait for a state change instead of polling frequently. Never automatically retry an unknown remote outcome.",
            true,
        ),
        tool::<CancelTask>(
            "cancel_task",
            "Request cancellation. Remote processes may continue.",
            false,
        ),
        tool::<Directory>(
            "list_directory",
            "List remote SFTP directory entries without running shell commands.",
            true,
        ),
        tool::<ReadFile>(
            "read_file",
            "Read complete remote text up to 256 KiB and its version. Preserve the version for an update; avoid rereading unchanged content unnecessarily.",
            true,
        ),
        tool::<WriteFile>(
            "write_file",
            "Create or update remote text; updates require expected_version and approval in controlled mode. Returns a task ticket; await get_task with wait_ms=10000. Never automatically repeat an unknown outcome.",
            false,
        ),
    ]
}
