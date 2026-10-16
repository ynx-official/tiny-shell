use super::*;

impl Service {
    /// The open chat needs one transcript, not copies of every saved conversation.
    pub fn panel_snapshot(&self, id: Option<&str>) -> Snapshot {
        self.state
            .lock()
            .map(|state| state.snapshot.for_conversation(id))
            .unwrap_or_default()
    }
}

impl Snapshot {
    fn for_conversation(&self, id: Option<&str>) -> Self {
        Self {
            settings: self.settings.clone(),
            conversations: self
                .conversations
                .iter()
                .find(|conversation| Some(conversation.id.as_str()) == id)
                .cloned()
                .into_iter()
                .collect(),
            ready: self.ready,
            temporary: self.temporary,
            storage_error: self.storage_error.clone(),
            keys_saved: self.keys_saved.clone(),
            temporary_keys: self.temporary_keys.clone(),
            busy: self.busy,
            notice: self.notice.clone(),
            history_notice: self.history_notice.clone(),
            discovered_models: self.discovered_models.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_snapshot_only_contains_selected_history_and_keeps_service_status() {
        let mut snapshot = Snapshot {
            ready: true,
            temporary: true,
            storage_error: Some("fixture warning".into()),
            ..Default::default()
        };
        for index in 0..100 {
            let mut conversation = Conversation::new(None, &snapshot.settings);
            conversation.id = index.to_string();
            conversation.messages.push(Message::text("user", "fixture"));
            snapshot.conversations.push(conversation);
        }
        let selected = snapshot.for_conversation(Some("42"));
        assert_eq!(selected.conversations.len(), 1);
        assert_eq!(selected.conversations[0].id, "42");
        assert_eq!(selected.conversations[0].messages[0].content, "fixture");
        assert!(selected.ready && selected.temporary);
        assert_eq!(selected.storage_error, snapshot.storage_error);
        assert!(
            snapshot
                .for_conversation(Some("deleted"))
                .conversations
                .is_empty()
        );
        assert!(snapshot.for_conversation(None).conversations.is_empty());
        assert_eq!(snapshot.conversations.len(), 100);
    }
}
