use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Ai,
    External,
    Local,
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Context {
    pub operation_id: String,
    pub source: Source,
    pub client_id: Option<String>,
    pub client_name: Option<String>,
    pub target_name: Option<String>,
    pub purpose: Option<String>,
    pub purpose_source: Option<String>,
    pub conversation_id: Option<String>,
    pub turn_id: Option<String>,
    pub model_request_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub task_id: Option<String>,
    pub parent_id: Option<String>,
    pub target_id: Option<String>,
    pub instance_id: Option<String>,
}
impl Context {
    pub fn new(source: Source) -> Self {
        Self {
            operation_id: Uuid::new_v4().to_string(),
            source,
            client_id: None,
            client_name: None,
            target_name: None,
            purpose: None,
            purpose_source: None,
            conversation_id: None,
            turn_id: None,
            model_request_id: None,
            tool_call_id: None,
            task_id: None,
            parent_id: None,
            target_id: None,
            instance_id: None,
        }
    }
    pub fn client(client: &str) -> Self {
        let mut result = Self::new(if client.starts_with("ai:") {
            Source::Ai
        } else {
            Source::External
        });
        result.client_id = Some(client.into());
        result.conversation_id = client.strip_prefix("ai:").map(str::to_owned);
        result
    }
    pub fn child(&self) -> Self {
        let mut result = self.clone();
        result.parent_id = Some(self.operation_id.clone());
        result.operation_id = Uuid::new_v4().to_string();
        result.task_id = None;
        result
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Create,
    Delete,
    Update,
    Read,
    Control,
    Command,
    Model,
    Unknown,
}
pub fn category(tool: &str, args: &Value) -> Category {
    match tool {
        "write_file" if args["mode"] == "create" => Category::Create,
        "write_file" => Category::Update,
        "read_file" | "list_directory" | "list_connections" | "get_status" | "get_task" => {
            Category::Read
        }
        "open_session" | "close_session" | "cancel_task" | "budget" => Category::Control,
        "run_command" => Category::Command,
        "model" => Category::Model,
        _ => Category::Unknown,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub sequence: i64,
    pub timestamp_ms: i64,
    pub context: Context,
    pub kind: String,
    pub tool: String,
    pub category: Category,
    pub state: String,
    pub details: Value,
    #[serde(default)]
    pub temporary: bool,
}
impl Event {
    pub fn new(context: Context, kind: &str, tool: &str, state: &str, details: Value) -> Self {
        let args = details
            .get("arguments")
            .or_else(|| details.get("call").and_then(|c| c.get("arguments")))
            .unwrap_or(&details);
        Self {
            schema_version: 1,
            id: Uuid::new_v4().to_string(),
            sequence: 0,
            timestamp_ms: now_ms(),
            context,
            kind: kind.into(),
            tool: tool.into(),
            category: category(tool, args),
            state: state.into(),
            details,
            temporary: false,
        }
    }
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub total: Option<u64>,
    pub cached_input: Option<u64>,
    pub cache_write: Option<u64>,
    pub reasoning_output: Option<u64>,
    #[serde(default)]
    pub total_derived: bool,
}
impl Usage {
    /// Providers may send usage incrementally. Missing fields never erase an observed value.
    pub fn merge(&mut self, next: Self) {
        if next.input.is_some() {
            self.input = next.input;
        }
        if next.output.is_some() {
            self.output = next.output;
        }
        if next.total.is_some() {
            self.total = next.total;
            self.total_derived = next.total_derived;
        } else if self.total.is_none() || self.total_derived {
            self.total = self
                .input
                .zip(self.output)
                .and_then(|(a, b)| a.checked_add(b))
                .filter(|n| *n <= i64::MAX as u64);
            self.total_derived = self.total.is_some();
        }
        if next.cached_input.is_some() {
            self.cached_input = next.cached_input;
        }
        if next.cache_write.is_some() {
            self.cache_write = next.cache_write;
        }
        if next.reasoning_output.is_some() {
            self.reasoning_output = next.reasoning_output;
        }
    }
    pub fn parse(value: &Value) -> Option<Self> {
        let number = |v: &Value| v.as_u64().filter(|n| *n <= i64::MAX as u64);
        let input = number(&value["prompt_tokens"]).or_else(|| number(&value["input_tokens"]));
        let output =
            number(&value["completion_tokens"]).or_else(|| number(&value["output_tokens"]));
        let reported = number(&value["total_tokens"]);
        let total = reported.or_else(|| {
            input
                .zip(output)
                .and_then(|(a, b)| a.checked_add(b))
                .filter(|n| *n <= i64::MAX as u64)
        });
        if input.is_none() && output.is_none() && total.is_none() {
            return None;
        }
        Some(Self {
            input,
            output,
            total,
            total_derived: reported.is_none() && total.is_some(),
            cached_input: number(&value["prompt_tokens_details"]["cached_tokens"])
                .or_else(|| number(&value["input_tokens_details"]["cached_tokens"])),
            cache_write: number(&value["prompt_tokens_details"]["cache_write_tokens"]),
            reasoning_output: number(&value["completion_tokens_details"]["reasoning_tokens"])
                .or_else(|| number(&value["output_tokens_details"]["reasoning_tokens"])),
        })
    }
    pub fn complete(&self) -> bool {
        self.input.is_some() && self.output.is_some() && self.total.is_some()
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Summary {
    pub input: u64,
    pub output: u64,
    pub total: u64,
    pub cached_input: u64,
    pub reasoning_output: u64,
    pub requests: usize,
    pub confirmed: usize,
    pub unknown: usize,
    #[serde(default)]
    pub pending: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestPurpose {
    #[default]
    Chat,
    Title,
    Followup,
    ProviderTest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelRequest {
    pub id: String,
    pub conversation_id: Option<String>,
    pub turn_id: String,
    #[serde(default)]
    pub purpose: RequestPurpose,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub conversation_title: String,
    pub response_model: Option<String>,
    pub provider_request_id: Option<String>,
    #[serde(default)]
    pub http_status: Option<u16>,
    pub started_ms: i64,
    pub first_byte_ms: Option<i64>,
    pub finished_ms: Option<i64>,
    pub state: String,
    pub usage: Option<Usage>,
    pub error: Option<String>,
}

impl Summary {
    /// An unfinished request may still receive its usage tail. Only settled requests with
    /// missing fields are unknown; received numbers remain visible in either state.
    pub fn add(&mut self, usage: Option<&Usage>, awaiting_usage: bool) {
        self.requests += 1;
        if let Some(u) = usage {
            self.input = self.input.saturating_add(u.input.unwrap_or_default());
            self.output = self.output.saturating_add(u.output.unwrap_or_default());
            self.total = self.total.saturating_add(u.total.unwrap_or_default());
            self.cached_input = self
                .cached_input
                .saturating_add(u.cached_input.unwrap_or_default());
            self.reasoning_output = self
                .reasoning_output
                .saturating_add(u.reasoning_output.unwrap_or_default());
        }
        if usage.is_some_and(Usage::complete) {
            self.confirmed += 1;
        } else if awaiting_usage {
            self.pending += 1;
        } else {
            self.unknown += 1;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub retention_days: u32,
    pub capacity_mb: u32,
    pub redact_patterns: Vec<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            retention_days: 90,
            capacity_mb: 512,
            redact_patterns: Vec::new(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=3650).contains(&self.retention_days) && (16..=32768).contains(&self.capacity_mb),
            "AUDIT_INVALID_SETTINGS"
        );
        anyhow::ensure!(self.redact_patterns.len() <= 32, "AUDIT_INVALID_SETTINGS");
        for pattern in &self.redact_patterns {
            anyhow::ensure!(pattern.len() <= 512, "AUDIT_INVALID_SETTINGS");
            regex::Regex::new(pattern).map_err(|_| anyhow::anyhow!("AUDIT_INVALID_SETTINGS"))?;
        }
        Ok(())
    }
}
#[derive(Clone, Default, Debug)]
pub struct Filter {
    pub source: Option<Source>,
    pub conversation_id: Option<String>,
    pub operation_id: Option<String>,
    pub target_id: Option<String>,
    pub client_id: Option<String>,
    pub category: Option<Category>,
    pub state: Option<String>,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub search: String,
    pub before_sequence: Option<i64>,
    pub approvals_only: bool,
    pub model_requests_only: bool,
    pub limit: usize,
}
#[derive(Clone, Default)]
pub struct Page {
    pub events: Vec<Event>,
    pub next: Option<i64>,
    pub scanned: usize,
}
#[derive(Clone, Default)]
pub struct Status {
    pub ready: bool,
    pub durable: bool,
    pub error: Option<String>,
    pub bytes: u64,
    pub settings: Settings,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn incomplete_usage_waits_until_settled_and_observed_numbers_remain_visible() {
        let partial = Usage::parse(&json!({"input_tokens":8})).unwrap();
        let mut pending = Summary::default();
        pending.add(Some(&partial), true);
        assert_eq!((pending.input, pending.pending, pending.unknown), (8, 1, 0));
        let mut settled = Summary::default();
        settled.add(Some(&partial), false);
        assert_eq!((settled.input, settled.pending, settled.unknown), (8, 0, 1));
        let zero = Usage::parse(&json!({"prompt_tokens":0,"completion_tokens":0})).unwrap();
        settled.add(Some(&zero), false);
        assert_eq!(
            (
                settled.requests,
                settled.confirmed,
                settled.unknown,
                settled.total
            ),
            (2, 1, 1, 0)
        );
        let mut reported_before_completion = Summary::default();
        reported_before_completion.add(Some(&zero), true);
        assert_eq!(
            (
                reported_before_completion.confirmed,
                reported_before_completion.pending,
                reported_before_completion.unknown
            ),
            (1, 0, 0)
        );
    }
    #[test]
    fn partial_updates_keep_observed_fields_and_recompute_derived_total() {
        let mut usage = Usage::parse(&json!({"input_tokens":8,"output_tokens":1})).unwrap();
        usage.merge(Usage::parse(&json!({"output_tokens":4})).unwrap());
        assert_eq!(usage.input, Some(8));
        assert_eq!(usage.output, Some(4));
        assert_eq!(usage.total, Some(12));
        assert!(usage.total_derived);
    }
    #[test]
    fn usage_keeps_missing_distinct_from_zero_and_subtotals_are_not_double_counted() {
        assert!(Usage::parse(&json!({})).is_none());
        let u = Usage::parse(&json!({"prompt_tokens":10,"completion_tokens":3,"total_tokens":13,"prompt_tokens_details":{"cached_tokens":8},"completion_tokens_details":{"reasoning_tokens":2}})).unwrap();
        let mut sum = Summary::default();
        sum.add(Some(&u), false);
        sum.add(None, false);
        assert_eq!(sum.total, 13);
        assert_eq!(sum.unknown, 1);
        assert_eq!(sum.cached_input, 8);
        assert!(
            Usage::parse(&json!({"prompt_tokens":0,"completion_tokens":0}))
                .unwrap()
                .complete()
        );
    }
    #[test]
    fn arbitrary_shell_remains_a_command_and_cannot_change_permission_classification() {
        assert_eq!(
            category("run_command", &json!({"command":"echo read; rm /a"})),
            Category::Command
        );
        assert_eq!(
            category("write_file", &json!({"mode":"create"})),
            Category::Create
        );
    }
}
