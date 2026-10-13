//! Optional, bounded followups never own the chat runner or its tool capability.
use super::*;
use serde_json::{Value, json};
use std::time::Duration;
use tiny_shell_mcp::audit::now_ms;

pub(super) fn parse_suggestions(text: &str) -> Result<Vec<String>> {
    ensure!(text.len() <= 4096, "AI_BAD_RESPONSE");
    let items: Vec<String> = serde_json::from_str(text).map_err(|_| anyhow!("AI_BAD_RESPONSE"))?;
    ensure!(items.len() <= 3, "AI_BAD_RESPONSE");
    let mut result = Vec::new();
    for item in items {
        let item = item.trim();
        ensure!(
            !item.is_empty()
                && item.chars().count() <= 120
                && !item.chars().any(|c| c.is_control() || matches!(c,
                    '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')),
            "AI_BAD_RESPONSE"
        );
        if !result
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(item))
        {
            result.push(item.to_owned());
        }
    }
    Ok(result)
}

pub(super) fn selected(c: &Conversation, turn: &str, index: usize) -> Result<String> {
    c.visible_followups()
        .filter(|suggestions| suggestions.turn_id == turn)
        .and_then(|suggestions| suggestions.items.get(index))
        .cloned()
        .ok_or_else(|| anyhow!("AI_FOLLOWUP_EXPIRED"))
}

fn context(c: &Conversation) -> Option<Value> {
    let turn = c.turns.last()?;
    let user = c
        .messages
        .get(turn.user_message)
        .filter(|m| m.role == "user")?;
    let assistant = c
        .messages
        .get(turn.user_message + 1..)?
        .iter()
        .rev()
        .find(|m| {
            m.role == "assistant" && m.tool_calls.is_empty() && !m.content.trim().is_empty()
        })?;
    // The final reply summarizes the outcome. Avoid resending raw files, tool output,
    // hidden SSH metadata, or unrelated turns just to produce three short suggestions.
    Some(json!({
        "user": user.content.chars().take(4000).collect::<String>(),
        "assistant": assistant.content.chars().take(6000).collect::<String>()
    }))
}

impl Service {
    pub fn followup_text(&self, id: &str, turn: &str, index: usize) -> Result<String> {
        let state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        ensure!(
            state.snapshot.settings.execution.followup_suggestions,
            "AI_FOLLOWUP_EXPIRED"
        );
        let c = state
            .snapshot
            .conversations
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow!("AI_CONVERSATION_MISSING"))?;
        selected(c, turn, index)
    }

    pub(super) fn cancel_followups(&self, id: &str) {
        if let Ok(state) = self.state.lock()
            && let Some((_, cancel)) = state.followup_cancel.get(id)
        {
            cancel.send_replace(true);
        }
    }

    pub(super) fn cancel_disabled_followups(&self) {
        if let Ok(state) = self.state.lock()
            && !state.snapshot.settings.execution.followup_suggestions
        {
            for (_, cancel) in state.followup_cancel.values() {
                cancel.send_replace(true);
            }
        }
    }

    pub(super) fn schedule_followups(
        self: &Arc<Self>,
        completed: &Conversation,
        provider: &Provider,
    ) {
        let (conversation, context, record, cancel) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if !state.snapshot.settings.execution.followup_suggestions
                || state.cancel.get(&completed.id).is_some_and(|s| *s.borrow())
            {
                return;
            }
            let Some(c) = state
                .snapshot
                .conversations
                .iter_mut()
                .find(|c| c.id == completed.id)
            else {
                return;
            };
            let turn_id = c.turn_id();
            if c.state != TurnState::Idle
                || turn_id != completed.turn_id()
                || c.turns.last().is_none_or(|turn| turn.state != "completed")
                || c.followups.as_ref().is_some_and(|s| s.turn_id == turn_id)
            {
                return;
            }
            let Some(context) = context(c) else { return };
            let record = ModelRequest {
                id: uuid::Uuid::new_v4().to_string(),
                conversation_id: Some(c.id.clone()),
                turn_id: turn_id.clone(),
                purpose: RequestPurpose::Followup,
                provider_id: provider.id.clone(),
                provider_name: provider.name.clone(),
                model: completed.model.clone(),
                conversation_title: c.title.clone(),
                response_model: None,
                provider_request_id: None,
                http_status: None,
                started_ms: now_ms(),
                first_byte_ms: None,
                finished_ms: None,
                state: "pending".into(),
                usage: None,
                error: None,
            };
            c.followups = Some(Followups {
                turn_id,
                request_id: record.id.clone(),
                state: FollowupState::Generating,
                items: Vec::new(),
            });
            c.requests.push(record.clone());
            let mut conversation = c.clone();
            conversation.target = None;
            conversation.model = completed.model.clone();
            let (sender, cancel) = watch::channel(false);
            state
                .followup_cancel
                .insert(conversation.id.clone(), (record.id.clone(), sender));
            (conversation, context, record, cancel)
        };
        self.request_event(&conversation, &record, "model_started");
        self.changed();
        let service = self.clone();
        let provider = provider.clone();
        self.runtime.spawn(async move {
            service
                .generate_followups(conversation, provider, context, record, cancel)
                .await;
        });
    }

    async fn generate_followups(
        self: &Arc<Self>,
        c: Conversation,
        provider: Provider,
        context: Value,
        record: ModelRequest,
        mut cancel: watch::Receiver<bool>,
    ) {
        let request_id = record.id.clone();
        let holder = Arc::new(Mutex::new(record));
        let already_cancelled = *cancel.borrow();
        let request = async {
            ensure!(!already_cancelled, "AI_CANCELLED");
            let key = self.request_key(&provider, None).await?;
            let message =
                crate::ai::provider::followups(&provider, &c.model, &key, &context, |metadata| {
                    self.observe_usage(&c, &holder, metadata);
                })
                .await?;
            ensure!(message.tool_calls.is_empty(), "AI_INVALID_TOOL");
            parse_suggestions(&message.content)?;
            Ok(message)
        };
        let result = tokio::select! {
            biased;
            _ = cancel.changed() => Err(anyhow!("AI_CANCELLED")),
            result = tokio::time::timeout(Duration::from_secs(20), request) => {
                result.unwrap_or_else(|_| Err(anyhow!("AI_TIMEOUT")))
            }
        };
        self.finish_request(&c, &holder, &result, "");
        if let Ok(mut state) = self.state.lock() {
            let enabled = state.snapshot.settings.execution.followup_suggestions;
            if let Some(live) = state
                .snapshot
                .conversations
                .iter_mut()
                .find(|live| live.id == c.id)
                && live.state == TurnState::Idle
                && live.turn_id() == c.turn_id()
                && let Some(suggestions) = &mut live.followups
                && suggestions.request_id == request_id
            {
                let items = result
                    .as_ref()
                    .ok()
                    .and_then(|m| parse_suggestions(&m.content).ok());
                if let Some(items) = items.filter(|_| enabled && !*cancel.borrow()) {
                    suggestions.state = FollowupState::Ready;
                    suggestions.items = items;
                } else {
                    suggestions.state = FollowupState::Failed;
                    suggestions.items.clear();
                }
            }
            // A cancelled older request must not remove the next turn's cancellation handle.
            if state
                .followup_cancel
                .get(&c.id)
                .is_some_and(|(id, _)| id == &request_id)
            {
                state.followup_cancel.remove(&c.id);
            }
        }
        let service = self.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Err(error) = service.checkpoint(&c.id)
                && error.to_string() != "AI_CONVERSATION_MISSING"
            {
                service.storage_failure(error);
            }
        })
        .await;
        self.changed();
    }
}
