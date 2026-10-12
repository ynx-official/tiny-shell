use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Model {
    pub id: String,
    pub supports_tools: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<Model>,
    pub default_model: String,
    pub enabled: bool,
    pub allow_http: bool,
    pub allow_no_key: bool,
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
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Settings {
    pub providers: Vec<Provider>,
    pub default_provider: Option<String>,
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
    RunningTool,
    Failed,
    Interrupted,
}
impl TurnState {
    pub fn active(self) -> bool {
        matches!(
            self,
            Self::Generating | Self::AwaitingApproval | Self::RunningTool
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
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Conversation {
    pub id: String,
    pub title: String,
    pub target: Option<BoundTarget>,
    pub provider_id: Option<String>,
    pub model: String,
    pub messages: Vec<Message>,
    pub activities: Vec<Activity>,
    pub state: TurnState,
    pub error: Option<String>,
    pub updated: i64,
}
impl Conversation {
    pub fn new(target: Option<BoundTarget>, settings: &Settings) -> Self {
        let provider = settings
            .providers
            .iter()
            .find(|p| Some(&p.id) == settings.default_provider.as_ref() && p.enabled);
        Self {
            id: Uuid::new_v4().to_string(),
            title: String::new(),
            target,
            provider_id: provider.map(|p| p.id.clone()),
            model: provider
                .map(|p| p.default_model.clone())
                .unwrap_or_default(),
            messages: Vec::new(),
            activities: Vec::new(),
            state: TurnState::Idle,
            error: None,
            updated: chrono::Utc::now().timestamp(),
        }
    }
}
