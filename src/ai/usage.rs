use super::Conversation;
use serde::{Deserialize, Serialize};
pub(crate) use tiny_shell_mcp::audit::{ModelRequest, Summary, Usage};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct TurnRecord {
    pub id: String,
    pub user_message: usize,
    pub started_ms: i64,
    pub finished_ms: Option<i64>,
    pub state: String,
}
#[derive(Clone, Default, Debug)]
pub(super) struct Metadata {
    pub usage: Option<Usage>,
    pub provider_request_id: Option<String>,
    pub response_model: Option<String>,
    pub http_status: Option<u16>,
}
impl Metadata {
    pub fn merge(&mut self, next: Self) {
        if let Some(usage) = next.usage {
            self.usage.get_or_insert_with(Usage::default).merge(usage);
        }
        if next.provider_request_id.is_some() {
            self.provider_request_id = next.provider_request_id;
        }
        if next.response_model.is_some() {
            self.response_model = next.response_model;
        }
        if next.http_status.is_some() {
            self.http_status = next.http_status;
        }
    }
    pub fn parse(value: &serde_json::Value) -> Self {
        Self {
            usage: Usage::parse(&value["usage"]),
            provider_request_id: value["id"]
                .as_str()
                .filter(|s| s.len() <= 256)
                .map(str::to_owned),
            response_model: value["model"]
                .as_str()
                .filter(|s| s.len() <= 256)
                .map(str::to_owned),
            http_status: None,
        }
    }
    pub fn present(&self) -> bool {
        self.usage.is_some()
            || self.provider_request_id.is_some()
            || self.response_model.is_some()
            || self.http_status.is_some()
    }
}
impl Conversation {
    pub fn usage_summary(&self) -> Summary {
        let mut summary = Summary::default();
        for request in &self.requests {
            summary.add(
                request.usage.as_ref(),
                request.finished_ms.is_none()
                    && matches!(request.state.as_str(), "pending" | "running"),
            );
        }
        summary
    }
    pub fn turn_id(&self) -> String {
        self.turns.last().map(|t| t.id.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::Settings;
    use serde_json::json;
    #[test]
    fn usage_from_an_empty_choices_tail_is_distinct_from_a_cancelled_unknown_request() {
        let metadata = Metadata::parse(
            &json!({"choices":[],"usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6}}),
        );
        assert_eq!(metadata.usage.unwrap().total, Some(6));
        let mut c = Conversation::new(None, &Settings::default());
        let request = ModelRequest {
            id: "one".into(),
            conversation_id: Some(c.id.clone()),
            turn_id: "turn".into(),
            purpose: tiny_shell_mcp::audit::RequestPurpose::Chat,
            provider_id: "provider".into(),
            provider_name: "Provider".into(),
            model: "model".into(),
            conversation_title: "title".into(),
            response_model: None,
            provider_request_id: None,
            http_status: None,
            started_ms: 0,
            first_byte_ms: None,
            finished_ms: Some(1),
            state: "completed".into(),
            usage: Usage::parse(&json!({"prompt_tokens":4,"completion_tokens":2})),
            error: None,
        };
        c.requests.push(request.clone());
        let mut cancelled = request;
        cancelled.id = "two".into();
        cancelled.usage = None;
        cancelled.state = "cancelled".into();
        c.requests.push(cancelled);
        let sum = c.usage_summary();
        assert_eq!(sum.total, 6);
        assert_eq!(sum.requests, 2);
        assert_eq!(sum.unknown, 1);
    }
}
