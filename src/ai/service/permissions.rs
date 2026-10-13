//! Conversation preferences are persisted with encrypted history, never global settings.
use super::*;
use tiny_shell_mcp::{
    audit::{Context, Event, Source},
    policy::Mode,
};

impl Service {
    pub fn set_conversation_mode(self: &Arc<Self>, id: &str, mode: Option<Mode>) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        // The runner can be finishing after the visible state becomes idle.
        ensure!(!state.cancel.contains_key(id), "AI_BUSY");
        let c = state
            .snapshot
            .conversations
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow!("AI_CONVERSATION_MISSING"))?;
        ensure!(!c.state.active(), "AI_BUSY");
        if c.mode_override == mode {
            return Ok(());
        }
        let before = c.mode_override;
        c.mode_override = mode;
        c.updated = chrono::Utc::now().timestamp();
        drop(state);
        let mut context = Context::new(Source::Local);
        context.conversation_id = Some(id.to_owned());
        self.audit.record(Event::new(
            context,
            "policy_changed",
            "permissions",
            "completed",
            serde_json::json!({"scope":"builtin_ai_conversation", "before":before, "after":mode,
                "actor":"local_user", "channel":"conversation_composer"}),
        ));
        self.changed();
        let service = self.clone();
        let id = id.to_owned();
        self.runtime.spawn_blocking(move || {
            if let Err(error) = service.checkpoint(&id)
                && error.to_string() != "AI_CONVERSATION_MISSING"
            {
                service.storage_failure(error);
            }
        });
        Ok(())
    }
}
