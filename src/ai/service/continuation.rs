//! A budget choice wakes the existing runner; it never resubmits a user message or a tool.
use super::*;
use tiny_shell_mcp::engine::InternalClient;

impl Service {
    pub fn attention(&self) -> RunAttention {
        self.state
            .lock()
            .map(|state| RunAttention {
                background_notifications: state
                    .snapshot
                    .settings
                    .execution
                    .background_notifications,
                active: state.cancel.keys().cloned().collect(),
                pending: state
                    .snapshot
                    .conversations
                    .iter()
                    .filter_map(|c| {
                        c.continuation.as_ref().map(|choice| PendingContinuation {
                            conversation: c.id.clone(),
                            title: c.title.clone(),
                            updated: c.updated,
                            choice: choice.clone(),
                        })
                    })
                    .collect(),
            })
            .unwrap_or_default()
    }

    pub fn continuation_live(&self, id: &str, token: &str) -> bool {
        self.state.lock().is_ok_and(|state| {
            state.cancel.get(id).is_some_and(|cancel| !*cancel.borrow())
                && state.snapshot.conversations.iter().any(|c| {
                    c.id == id
                        && c.continuation
                            .as_ref()
                            .is_some_and(|pause| pause.token == token)
                })
        })
    }
    pub fn resume(&self, id: &str, token: &str) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        ensure!(
            state.cancel.get(id).is_some_and(|cancel| !*cancel.borrow()),
            "AI_CONTINUATION_EXPIRED"
        );
        let c = state
            .snapshot
            .conversations
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow!("AI_CONTINUATION_EXPIRED"))?;
        ensure!(
            c.state == TurnState::AwaitingContinuation
                && c.continuation
                    .as_ref()
                    .is_some_and(|pause| pause.token == token),
            "AI_CONTINUATION_EXPIRED"
        );
        let sender = state
            .continuations
            .remove(id)
            .ok_or_else(|| anyhow!("AI_CONTINUATION_EXPIRED"))?;
        sender
            .send(())
            .map_err(|_| anyhow!("AI_CONTINUATION_EXPIRED"))?;
        if let Some(c) = state.snapshot.conversations.iter_mut().find(|c| c.id == id) {
            c.continuation = None;
            c.state = TurnState::Generating;
        }
        self.changed();
        Ok(())
    }

    pub fn terminate_continuation(&self, id: &str, token: &str) -> Result<()> {
        let state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        let c = state
            .snapshot
            .conversations
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow!("AI_CONTINUATION_EXPIRED"))?;
        ensure!(
            c.continuation
                .as_ref()
                .is_some_and(|pause| pause.token == token),
            "AI_CONTINUATION_EXPIRED"
        );
        let cancel = state
            .cancel
            .get(id)
            .ok_or_else(|| anyhow!("AI_CONTINUATION_EXPIRED"))?;
        ensure!(!*cancel.borrow(), "AI_CONTINUATION_EXPIRED");
        cancel.send_replace(true);
        self.changed();
        Ok(())
    }

    pub(super) async fn wait_for_continuation(
        self: &Arc<Self>,
        id: &str,
        kind: LimitKind,
        used: usize,
        allowance: usize,
        capability: Option<&InternalClient>,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<()> {
        ensure!(!*cancel.borrow(), "AI_CANCELLED");
        ensure!(
            capability.is_none_or(|client| client.is_valid()),
            "AI_ACCESS_CHANGED"
        );
        let (sender, receiver) = tokio::sync::oneshot::channel();
        {
            let mut state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
            let c = state
                .snapshot
                .conversations
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| anyhow!("AI_CANCELLED"))?;
            c.state = TurnState::AwaitingContinuation;
            c.updated = chrono::Utc::now().timestamp();
            c.continuation = Some(Continuation {
                token: uuid::Uuid::new_v4().to_string(),
                kind,
                used,
                allowance,
            });
            state.continuations.insert(id.to_owned(), sender);
        }
        self.changed();
        self.persist_async(id).await;
        let revoked = async {
            if let Some(client) = capability {
                client.revoked().await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        let result = tokio::select! {
            biased;
            _ = cancel.changed() => Err(anyhow!("AI_CANCELLED")),
            _ = revoked => Err(anyhow!("AI_ACCESS_CHANGED")),
            choice = receiver => choice.map_err(|_| anyhow!("AI_CANCELLED")),
        };
        {
            let mut state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
            state.continuations.remove(id);
            if let Some(c) = state.snapshot.conversations.iter_mut().find(|c| c.id == id) {
                c.continuation = None;
            }
        }
        self.changed();
        result?;
        ensure!(!*cancel.borrow(), "AI_CANCELLED");
        ensure!(
            capability.is_none_or(|client| client.is_valid()),
            "AI_ACCESS_CHANGED"
        );
        Ok(())
    }

    pub fn save_execution(self: &Arc<Self>, execution: ExecutionSettings) -> Result<()> {
        execution.validate()?;
        self.begin_operation()?;
        let service = self.clone();
        self.runtime.spawn_blocking(move || {
            let result = (|| {
                let _serial = service
                    .persistence
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
                let mut settings = service
                    .state
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
                    .snapshot
                    .settings
                    .clone();
                settings.execution = execution.clone();
                service.storage()?.save_settings(&settings)?;
                service
                    .state
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
                    .snapshot
                    .settings
                    .execution = execution;
                Ok(())
            })();
            service.notice(result);
        });
        Ok(())
    }
}
