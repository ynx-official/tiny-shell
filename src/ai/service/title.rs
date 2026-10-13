//! One bounded background request names a new conversation without owning its chat runner.
use super::*;
use std::time::Duration;
use tiny_shell_mcp::audit::now_ms;

pub(super) fn fallback_title(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(20)
        .collect()
}

pub(super) fn normalize_title(text: &str) -> Option<String> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let line = lines.next()?;
    if lines.next().is_some() {
        return None;
    }
    let line = line.trim_start_matches(['#', '*', '`', ' ']);
    let line = ["标题：", "标题:", "Title:", "title:"]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))
        .unwrap_or(line)
        .trim();
    let line = line.trim_matches(['"', '\'', '“', '”', '「', '」', '`', '*', ' ']);
    let cleaned = line.chars().filter(|c| !c.is_control()).collect::<String>();
    let title = cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(20)
        .collect::<String>();
    let title = title.trim_end_matches([' ', '。', '.']);
    (!title.is_empty()).then(|| title.to_owned())
}

impl Service {
    pub(super) fn cancel_title(&self, id: &str) {
        if let Ok(state) = self.state.lock()
            && let Some(cancel) = state.title_cancel.get(id)
        {
            cancel.send_replace(true);
        }
    }

    pub(super) fn schedule_title(self: &Arc<Self>, c: &Conversation, provider: &Provider) {
        if c.turns.len() != 1 || c.turns[0].user_message != 0 {
            return;
        }
        let Some(first) = c.messages.first().filter(|m| m.role == "user") else {
            return;
        };
        let task = first.content.chars().take(4000).collect::<String>();
        let record = ModelRequest {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: Some(c.id.clone()),
            turn_id: c.turn_id(),
            purpose: RequestPurpose::Title,
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model: c.model.clone(),
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
        let (sender, cancel) = watch::channel(false);
        {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if state.cancel.get(&c.id).is_some_and(|s| *s.borrow()) {
                return;
            }
            let Some(live) = state
                .snapshot
                .conversations
                .iter_mut()
                .find(|v| v.id == c.id)
            else {
                return;
            };
            if live.title_state != TitleState::AwaitingFirstTurn {
                return;
            }
            live.title_state = TitleState::Generating;
            live.requests.push(record.clone());
            state.title_cancel.insert(c.id.clone(), sender);
        }
        // A title never includes the bound SSH target or tool output, including in its audit context.
        let mut conversation = c.clone();
        conversation.target = None;
        self.request_event(&conversation, &record, "model_started");
        self.changed();
        let service = self.clone();
        let provider = provider.clone();
        self.runtime.spawn(async move {
            service
                .generate_title(conversation, provider, task, record, cancel)
                .await;
        });
    }

    async fn generate_title(
        self: &Arc<Self>,
        c: Conversation,
        provider: Provider,
        task: String,
        record: ModelRequest,
        mut cancel: watch::Receiver<bool>,
    ) {
        let holder = Arc::new(Mutex::new(record));
        let already_cancelled = *cancel.borrow();
        let request = async {
            ensure!(!already_cancelled, "AI_CANCELLED");
            let key = self.request_key(&provider, None).await?;
            let message =
                crate::ai::provider::title(&provider, &c.model, &key, &task, |metadata| {
                    self.observe_usage(&c, &holder, metadata);
                })
                .await?;
            ensure!(message.tool_calls.is_empty(), "AI_INVALID_TOOL");
            ensure!(
                normalize_title(&message.content).is_some(),
                "AI_EMPTY_RESPONSE"
            );
            Ok(message)
        };
        let result = tokio::select! {
            biased;
            _ = cancel.changed() => Err(anyhow!("AI_CANCELLED")),
            result = tokio::time::timeout(Duration::from_secs(30), request) => {
                result.unwrap_or_else(|_| Err(anyhow!("AI_TIMEOUT")))
            }
        };
        self.finish_request(&c, &holder, &result, "");
        let _ = self.mutate(&c.id, |live| {
            if live.title_state == TitleState::Generating {
                if let Some(title) = result
                    .as_ref()
                    .ok()
                    .and_then(|m| normalize_title(&m.content))
                    && !*cancel.borrow()
                {
                    live.title = title;
                    live.title_state = TitleState::Generated;
                } else {
                    live.title_state = TitleState::Failed;
                }
            }
            Ok(())
        });
        if let Ok(mut state) = self.state.lock() {
            state.title_cancel.remove(&c.id);
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
