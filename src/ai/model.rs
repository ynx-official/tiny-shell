use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Model {
    pub id: String,
    pub supports_tools: bool,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<Model>,
    pub default_model: String,
    pub enabled: bool,
    pub allow_http: bool,
    pub allow_no_key: bool,
    #[serde(default = "usage_enabled")]
    pub stream_usage: bool,
}
fn usage_enabled() -> bool {
    true
}
impl Default for Provider {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: String::new(),
            base_url: String::new(),
            models: Vec::new(),
            default_model: String::new(),
            enabled: true,
            allow_http: false,
            allow_no_key: false,
            stream_usage: true,
        }
    }
}
impl Provider {
    pub fn validate(&self) -> anyhow::Result<()> {
        let url =
            reqwest::Url::parse(self.base_url.trim()).map_err(|_| anyhow::anyhow!("AI_BAD_URL"))?;
        let local = matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
        );
        anyhow::ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "AI_BAD_URL"
        );
        anyhow::ensure!(
            url.scheme() == "https" || (url.scheme() == "http" && local && self.allow_http),
            "AI_HTTPS_REQUIRED"
        );
        anyhow::ensure!(!self.allow_no_key || local, "AI_LOCAL_ONLY");
        anyhow::ensure!(
            !url.path().ends_with("/chat/completions"),
            "AI_BASE_URL_REQUIRED"
        );
        Ok(())
    }
    pub fn endpoint(&self, resource: &str) -> anyhow::Result<reqwest::Url> {
        self.validate()?;
        reqwest::Url::parse(&format!(
            "{}/{}",
            self.base_url.trim().trim_end_matches('/'),
            resource
        ))
        .map_err(|_| anyhow::anyhow!("AI_BAD_URL"))
    }
}
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Settings {
    pub providers: Vec<Provider>,
    pub default_provider: Option<String>,
    #[serde(default)]
    pub execution: ExecutionSettings,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ExecutionSettings {
    pub tool_calls: usize,
    pub model_requests: usize,
    pub background_notifications: bool,
    pub followup_suggestions: bool,
}
impl Default for ExecutionSettings {
    fn default() -> Self {
        Self {
            tool_calls: 16,
            model_requests: 8,
            background_notifications: false,
            followup_suggestions: true,
        }
    }
}
impl ExecutionSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=128).contains(&self.tool_calls) && (1..=64).contains(&self.model_requests),
            "AI_INVALID_BUDGET"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum LimitKind {
    ToolCalls,
    ModelRequests,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Continuation {
    pub token: String,
    pub kind: LimitKind,
    pub used: usize,
    pub allowance: usize,
    #[serde(default)]
    pub audit_id: Option<String>,
}

#[derive(Clone)]
pub(crate) struct PendingContinuation {
    pub conversation: String,
    pub title: String,
    pub updated: i64,
    pub choice: Continuation,
}
#[derive(Default)]
pub(crate) struct RunAttention {
    pub background_notifications: bool,
    pub active: std::collections::HashSet<String>,
    pub pending: Vec<PendingContinuation>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct BoundTarget {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub digest: String,
}
impl BoundTarget {
    pub fn remote(&self) -> tiny_shell_mcp::engine::Target {
        tiny_shell_mcp::engine::Target {
            id: self.id.clone(),
            name: self.name.clone(),
            group: None,
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            digest: self.digest.clone(),
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Message {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<Value>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}
impl Message {
    pub fn text(role: &str, text: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: text.into(),
            ..Self::default()
        }
    }
    pub fn wire(&self) -> Value {
        let mut value = serde_json::json!({"role":self.role,"content":self.content});
        if !self.tool_calls.is_empty() {
            value["tool_calls"] = self.tool_calls.clone().into();
        }
        if let Some(id) = &self.tool_call_id {
            value["tool_call_id"] = id.clone().into();
        }
        value
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TurnState {
    #[default]
    Idle,
    Generating,
    AwaitingApproval,
    AwaitingContinuation,
    RunningTool,
    Failed,
    Interrupted,
}
impl TurnState {
    pub fn active(self) -> bool {
        matches!(
            self,
            Self::Generating
                | Self::AwaitingApproval
                | Self::AwaitingContinuation
                | Self::RunningTool
        )
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Activity {
    pub name: String,
    pub task_id: Option<String>,
    pub state: String,
    // The transcript anchor keeps repeated calls distinct across tool rounds and turns.
    #[serde(default)]
    pub message_index: Option<usize>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TitleState {
    /// Old histories and user-chosen titles must never be renamed in the background.
    #[default]
    Preserved,
    AwaitingFirstTurn,
    Generating,
    Generated,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FollowupState {
    Generating,
    Ready,
    Failed,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Followups {
    pub turn_id: String,
    pub request_id: String,
    pub state: FollowupState,
    pub items: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Conversation {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub title_state: TitleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followups: Option<Followups>,
    pub target: Option<BoundTarget>,
    pub provider_id: Option<String>,
    pub model: String,
    pub messages: Vec<Message>,
    /// None follows the global AI mode; the SSH enable switch remains global.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode_override: Option<tiny_shell_mcp::policy::Mode>,
    pub activities: Vec<Activity>,
    pub state: TurnState,
    pub error: Option<String>,
    #[serde(default)]
    pub continuation: Option<Continuation>,
    pub updated: i64,
    #[serde(default)]
    pub requests: Vec<super::ModelRequest>,
    #[serde(default)]
    pub turns: Vec<super::TurnRecord>,
}
impl Conversation {
    pub fn visible_followups(&self) -> Option<&Followups> {
        self.followups.as_ref().filter(|suggestions| {
            self.state == TurnState::Idle
                && self
                    .turns
                    .last()
                    .is_some_and(|turn| turn.id == suggestions.turn_id && turn.state == "completed")
                && suggestions.state == FollowupState::Ready
                && !suggestions.items.is_empty()
        })
    }

    pub fn new(target: Option<BoundTarget>, settings: &Settings) -> Self {
        let provider = settings
            .providers
            .iter()
            .find(|p| Some(&p.id) == settings.default_provider.as_ref() && p.enabled);
        Self {
            id: Uuid::new_v4().to_string(),
            title: String::new(),
            title_state: TitleState::AwaitingFirstTurn,
            followups: None,
            target,
            provider_id: provider.map(|p| p.id.clone()),
            model: provider
                .map(|p| p.default_model.clone())
                .unwrap_or_default(),
            messages: Vec::new(),
            mode_override: None,
            activities: Vec::new(),
            state: TurnState::Idle,
            error: None,
            continuation: None,
            updated: chrono::Utc::now().timestamp(),
            requests: Vec::new(),
            turns: Vec::new(),
        }
    }
}
