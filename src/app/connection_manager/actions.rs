#![allow(dead_code)]

use anyhow::Result;

use crate::session::{
    config::ConfigStore,
    connection_catalog::{self, ConnectionSortKey},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionManagerAction {
    CopySession {
        id: String,
    },
    CutSession {
        id: String,
    },
    CopyAddress {
        id: String,
    },
    PasteAddress {
        address: String,
    },
    Paste {
        group: Option<String>,
    },
    MoveSession {
        id: String,
        group: Option<String>,
    },
    CopyGroup {
        name: String,
    },
    DeleteSession {
        id: String,
    },
    DeleteGroup {
        name: String,
    },
    RestoreSession {
        id: String,
    },
    RestoreGroup {
        name: String,
    },
    PurgeSession {
        id: String,
    },
    SetSort {
        key: ConnectionSortKey,
        descending: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardPayload {
    Session { id: String, cut: bool },
    Group { name: String, cut: bool },
}

#[derive(Debug, Clone, Default)]
pub struct ConnectionManagerActions {
    pub clipboard: Option<ClipboardPayload>,
}

impl ConnectionManagerActions {
    pub fn execute(
        &mut self,
        config: &mut ConfigStore,
        action: ConnectionManagerAction,
    ) -> Result<Option<String>> {
        match action {
            ConnectionManagerAction::CopySession { id } => {
                self.clipboard = Some(ClipboardPayload::Session { id, cut: false });
                Ok(None)
            }
            ConnectionManagerAction::CutSession { id } => {
                self.clipboard = Some(ClipboardPayload::Session { id, cut: true });
                Ok(None)
            }
            ConnectionManagerAction::CopyAddress { id } => {
                let session = config
                    .get(&id)
                    .ok_or_else(|| anyhow::anyhow!("connection not found"))?;
                Ok(Some(connection_catalog::session_address(session)))
            }
            ConnectionManagerAction::PasteAddress { address } => {
                let session = connection_catalog::parse_session_address(&address)?;
                Ok(Some(connection_catalog::session_address(&session)))
            }
            ConnectionManagerAction::Paste { group } => self.paste(config, group.as_deref()),
            ConnectionManagerAction::MoveSession { id, group } => {
                connection_catalog::move_session(config, &id, group.as_deref())?;
                Ok(None)
            }
            ConnectionManagerAction::CopyGroup { name } => {
                self.clipboard = Some(ClipboardPayload::Group { name, cut: false });
                Ok(None)
            }
            ConnectionManagerAction::DeleteSession { id } => {
                config.soft_delete_session(&id);
                Ok(None)
            }
            ConnectionManagerAction::DeleteGroup { name } => {
                config.soft_delete_connection_group(&name);
                Ok(None)
            }
            ConnectionManagerAction::RestoreSession { id } => {
                config.restore_deleted_session(&id);
                Ok(None)
            }
            ConnectionManagerAction::RestoreGroup { name } => {
                config.restore_deleted_connection_group(&name);
                Ok(None)
            }
            ConnectionManagerAction::PurgeSession { id } => {
                config.purge_deleted_session(&id);
                Ok(None)
            }
            ConnectionManagerAction::SetSort { .. } => Ok(None),
        }
    }

    pub fn cut_session(&mut self, id: String) {
        self.clipboard = Some(ClipboardPayload::Session { id, cut: true });
    }

    pub fn cut_group(&mut self, name: String) {
        self.clipboard = Some(ClipboardPayload::Group { name, cut: true });
    }

    pub fn paste(
        &mut self,
        config: &mut ConfigStore,
        group: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(payload) = self.clipboard.clone() else {
            return Ok(None);
        };
        match payload {
            ClipboardPayload::Session { id, cut } if cut => {
                connection_catalog::move_session(config, &id, group)?;
                self.clipboard = None;
                Ok(Some(id))
            }
            ClipboardPayload::Session { id, .. } => {
                Ok(Some(connection_catalog::copy_session(config, &id, group)?))
            }
            ClipboardPayload::Group { name, cut } if cut => {
                let moved = connection_catalog::move_connection_group(config, &name, group)?;
                self.clipboard = None;
                Ok(Some(moved))
            }
            ClipboardPayload::Group { name, .. } => Ok(Some(
                connection_catalog::copy_connection_group(config, &name, group)?,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::config::Session;

    #[test]
    fn structured_clipboard_distinguishes_copy_and_cut() {
        let mut actions = ConnectionManagerActions::default();
        let mut config = ConfigStore::in_memory();
        let mut session =
            Session::password("host".to_string(), 22, "user".to_string(), String::new());
        session.id = "session-1".to_string();
        config.upsert(session);

        actions
            .execute(
                &mut config,
                ConnectionManagerAction::CopySession {
                    id: "session-1".to_string(),
                },
            )
            .unwrap();
        let copied = actions.paste(&mut config, None).unwrap().unwrap();
        assert_ne!(copied, "session-1");
        actions.cut_session("session-1".to_string());
        actions.paste(&mut config, Some("prod")).unwrap();
        assert_eq!(
            config
                .get("session-1")
                .and_then(|item| item.group.as_deref()),
            Some("prod")
        );
    }

    #[test]
    fn address_actions_validate_and_return_ssh_addresses_without_saving_partial_sessions() {
        let mut actions = ConnectionManagerActions::default();
        let mut config = ConfigStore::in_memory();
        let mut session =
            Session::password("host".to_string(), 22, "user".to_string(), String::new());
        session.id = "session-1".to_string();
        config.upsert(session);

        let address = actions
            .execute(
                &mut config,
                ConnectionManagerAction::CopyAddress {
                    id: "session-1".to_string(),
                },
            )
            .unwrap();
        assert_eq!(address.as_deref(), Some("ssh://user@host:22"));
        assert_eq!(
            actions
                .execute(
                    &mut config,
                    ConnectionManagerAction::PasteAddress {
                        address: "ssh://alice@example.test:2200".to_string(),
                    },
                )
                .unwrap()
                .as_deref(),
            Some("ssh://alice@example.test:2200")
        );
        assert!(
            actions
                .execute(
                    &mut config,
                    ConnectionManagerAction::PasteAddress {
                        address: "ssh://example.test:22".to_string(),
                    },
                )
                .is_err()
        );
        assert_eq!(config.sessions().len(), 1);
    }
}
