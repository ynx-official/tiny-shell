use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use sha2::{Digest, Sha256};

use super::super::model::{
    SyncDeletedConnectionGroup, SyncDeletedSession, SyncManagedKey, SyncSecret, SyncSession,
};

pub const V3_FORMAT_VERSION: u32 = 3;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityVersion {
    pub generation: u64,
    pub device_id: String,
    pub updated_at: i64,
}

impl EntityVersion {
    pub fn initial(device_id: impl Into<String>, updated_at: i64) -> Self {
        Self {
            generation: 1,
            device_id: device_id.into(),
            updated_at,
        }
    }

    pub fn compare(&self, other: &Self) -> Ordering {
        self.generation
            .cmp(&other.generation)
            .then_with(|| self.updated_at.cmp(&other.updated_at))
            .then_with(|| self.device_id.cmp(&other.device_id))
    }

    pub fn next(&self, device_id: impl Into<String>, updated_at: i64) -> Self {
        Self {
            generation: self.generation.saturating_add(1),
            device_id: device_id.into(),
            updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncEntity<T> {
    pub id: String,
    pub version: EntityVersion,
    pub value: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncTombstone {
    pub entity_type: String,
    pub entity_id: String,
    pub version: EntityVersion,
    pub deleted_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct V3QuickCommandCategory {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct V3QuickCommand {
    pub category_id: String,
    pub name: String,
    pub remark: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3SyncPayload {
    pub schema_version: u32,
    pub revision: String,
    pub updated_at: String,
    pub device_id: String,
    #[serde(default)]
    pub sessions: Vec<SyncEntity<SyncSession>>,
    #[serde(default)]
    pub managed_keys: Vec<SyncEntity<SyncManagedKey>>,
    #[serde(default)]
    pub connection_groups: Vec<SyncEntity<String>>,
    #[serde(default)]
    pub quick_command_categories: Vec<SyncEntity<V3QuickCommandCategory>>,
    #[serde(default)]
    pub quick_commands: Vec<SyncEntity<V3QuickCommand>>,
    #[serde(default)]
    pub deleted_sessions: Vec<SyncTombstone>,
    #[serde(default)]
    pub deleted_managed_keys: Vec<SyncTombstone>,
    #[serde(default)]
    pub deleted_connection_groups: Vec<SyncTombstone>,
    #[serde(default)]
    pub deleted_quick_command_categories: Vec<SyncTombstone>,
    #[serde(default)]
    pub deleted_quick_commands: Vec<SyncTombstone>,
    #[serde(default)]
    pub deleted_session_snapshots: Vec<SyncEntity<SyncDeletedSession>>,
    #[serde(default)]
    pub deleted_group_snapshots: Vec<SyncEntity<SyncDeletedConnectionGroup>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy_password_verifier: Option<String>,
    #[serde(default)]
    pub secrets: Vec<SyncSecret>,
}

impl V3SyncPayload {
    pub fn from_config(
        device_id: String,
        config: &crate::sync::merge::MergedConfig,
        include_secrets: bool,
        password: &str,
        base_payload: Option<&Self>,
    ) -> anyhow::Result<Self> {
        let updated_at = chrono::Utc::now().timestamp();
        let version = |entity_type: &str, id: &str| {
            base_payload
                .and_then(|payload| find_entity_version(payload, entity_type, id))
                .map_or_else(
                    || EntityVersion::initial(device_id.clone(), updated_at),
                    |current| current.next(device_id.clone(), updated_at),
                )
        };
        let sessions = config
            .sessions
            .clone()
            .into_iter()
            .map(|session| {
                let id = session.id.clone();
                Ok(SyncEntity {
                    id: id.clone(),
                    version: version("session", &id),
                    value: SyncSession::export(session, include_secrets, password)?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let managed_keys = config
            .managed_keys
            .clone()
            .into_iter()
            .map(|key| {
                let id = key.id.clone();
                Ok(SyncEntity {
                    id: id.clone(),
                    version: version("managed-key", &id),
                    value: SyncManagedKey::export(key, include_secrets, password)?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let connection_groups = config
            .connection_groups
            .clone()
            .into_iter()
            .map(|value| {
                let id = stable_entity_id("connection-group", &value);
                SyncEntity {
                    id: id.clone(),
                    version: version("connection-group", &id),
                    value,
                }
            })
            .collect();
        let mut quick_command_categories = Vec::new();
        let mut quick_commands = Vec::new();
        for category in &config.quick_command_categories {
            let category_id = category.id.clone();
            quick_command_categories.push(SyncEntity {
                id: category_id.clone(),
                version: version("quick-command-category", &category_id),
                value: V3QuickCommandCategory {
                    name: category.name.clone(),
                },
            });
            quick_commands.extend(category.commands.iter().map(|command| SyncEntity {
                id: command.id.clone(),
                version: version("quick-command", &command.id),
                value: V3QuickCommand {
                    category_id: category_id.clone(),
                    name: command.name.clone(),
                    remark: command.remark.clone(),
                    command: command.command.clone(),
                },
            }));
        }
        let deleted_sessions = config
            .deleted_sessions
            .iter()
            .map(|deleted| SyncTombstone {
                entity_type: "session".to_string(),
                entity_id: deleted.session.id.clone(),
                version: version("session", &deleted.session.id),
                deleted_at: deleted.deleted_at,
            })
            .collect();
        let deleted_session_snapshots = config
            .deleted_sessions
            .clone()
            .into_iter()
            .map(|deleted| {
                let id = deleted.session.id.clone();
                Ok(SyncEntity {
                    id: id.clone(),
                    version: version("session", &id),
                    value: SyncDeletedSession::export(deleted, include_secrets, password)?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let deleted_connection_groups = config
            .deleted_connection_groups
            .iter()
            .map(|deleted| {
                let id = stable_entity_id("connection-group", &deleted.name);
                SyncTombstone {
                    entity_type: "connection-group".to_string(),
                    entity_id: id.clone(),
                    version: version("connection-group", &id),
                    deleted_at: deleted.deleted_at,
                }
            })
            .collect();
        let deleted_group_snapshots = config
            .deleted_connection_groups
            .clone()
            .into_iter()
            .map(|deleted| {
                let id = stable_entity_id("connection-group", &deleted.name);
                let snapshot =
                    SyncDeletedConnectionGroup::export(deleted, include_secrets, password)?;
                Ok(SyncEntity {
                    id: id.clone(),
                    version: version("connection-group", &id),
                    value: snapshot,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            schema_version: V3_FORMAT_VERSION,
            revision: uuid::Uuid::new_v4().to_string(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            device_id,
            sessions,
            managed_keys,
            connection_groups,
            quick_command_categories,
            quick_commands,
            deleted_sessions,
            deleted_managed_keys: Vec::new(),
            deleted_connection_groups,
            deleted_quick_command_categories: Vec::new(),
            deleted_quick_commands: Vec::new(),
            deleted_session_snapshots,
            deleted_group_snapshots,
            privacy_password_verifier: include_secrets
                .then(|| crate::crypto::hash_privacy_password(password))
                .transpose()?,
            secrets: Vec::new(),
        })
    }
    pub fn privacy_password_status(
        &self,
        password: &str,
    ) -> anyhow::Result<crate::sync::PrivacyPasswordStatus> {
        let Some(verifier) = self.privacy_password_verifier.as_deref() else {
            return Ok(crate::sync::PrivacyPasswordStatus::NotConfigured);
        };
        if password.is_empty() {
            return Ok(crate::sync::PrivacyPasswordStatus::Missing);
        }
        if crate::crypto::verify_privacy_password(password, verifier)? {
            Ok(crate::sync::PrivacyPasswordStatus::Verified)
        } else {
            Ok(crate::sync::PrivacyPasswordStatus::Mismatch)
        }
    }

    #[cfg(test)]
    pub fn from_legacy_for_tests(payload: super::super::model::SyncPayload) -> Self {
        let updated_at = chrono::DateTime::parse_from_rfc3339(&payload.updated_at)
            .map_or_else(|_| 0, |value| value.timestamp());
        let device_id = payload.device_id.clone();
        let version = || EntityVersion::initial(device_id.clone(), updated_at);
        let sessions = payload
            .sessions
            .into_iter()
            .map(|value| SyncEntity {
                id: value.id.clone(),
                version: version(),
                value,
            })
            .collect();
        let managed_keys = payload
            .managed_keys
            .into_iter()
            .map(|value| SyncEntity {
                id: value.id.clone(),
                version: version(),
                value,
            })
            .collect();
        let mut quick_command_categories = Vec::new();
        let mut quick_commands = Vec::new();
        for category in payload.quick_command_categories {
            let category_id = category.id.clone();
            quick_command_categories.push(SyncEntity {
                id: category_id.clone(),
                version: version(),
                value: V3QuickCommandCategory {
                    name: category.name,
                },
            });
            quick_commands.extend(category.commands.into_iter().map(|command| SyncEntity {
                id: command.id.clone(),
                version: version(),
                value: V3QuickCommand {
                    category_id: category_id.clone(),
                    name: command.name,
                    remark: command.remark,
                    command: command.command,
                },
            }));
        }
        let mut deleted_sessions = Vec::new();
        let mut deleted_session_snapshots = Vec::new();
        for deleted in payload.deleted_sessions {
            let id = deleted.session.id.clone();
            let entity_version = EntityVersion::initial(device_id.clone(), deleted.deleted_at);
            deleted_sessions.push(SyncTombstone {
                entity_type: "session".into(),
                entity_id: id.clone(),
                version: entity_version.clone(),
                deleted_at: deleted.deleted_at,
            });
            deleted_session_snapshots.push(SyncEntity {
                id,
                version: entity_version,
                value: deleted,
            });
        }
        let mut deleted_connection_groups = Vec::new();
        let mut deleted_group_snapshots = Vec::new();
        for deleted in payload.deleted_connection_groups {
            let id = stable_entity_id("connection-group", &deleted.name);
            let entity_version = EntityVersion::initial(device_id.clone(), deleted.deleted_at);
            deleted_connection_groups.push(SyncTombstone {
                entity_type: "connection-group".into(),
                entity_id: id.clone(),
                version: entity_version.clone(),
                deleted_at: deleted.deleted_at,
            });
            deleted_group_snapshots.push(SyncEntity {
                id,
                version: entity_version,
                value: deleted,
            });
        }
        let privacy_password_verifier = payload.privacy_password_verifier;
        Self {
            schema_version: V3_FORMAT_VERSION,
            revision: payload.revision,
            updated_at: payload.updated_at,
            device_id: payload.device_id,
            sessions,
            managed_keys,
            connection_groups: payload
                .connection_groups
                .into_iter()
                .map(|value| SyncEntity {
                    id: stable_entity_id("connection-group", &value),
                    version: version(),
                    value,
                })
                .collect(),
            quick_command_categories,
            quick_commands,
            deleted_sessions,
            deleted_managed_keys: Vec::new(),
            deleted_connection_groups,
            deleted_quick_command_categories: Vec::new(),
            deleted_quick_commands: Vec::new(),
            deleted_session_snapshots,
            deleted_group_snapshots,
            privacy_password_verifier,
            secrets: Vec::new(),
        }
    }
}

#[cfg(test)]
impl From<super::super::model::SyncPayload> for V3SyncPayload {
    fn from(payload: super::super::model::SyncPayload) -> Self {
        Self::from_legacy_for_tests(payload)
    }
}

fn find_entity_version(
    payload: &V3SyncPayload,
    entity_type: &str,
    entity_id: &str,
) -> Option<EntityVersion> {
    fn active<T>(entities: &[SyncEntity<T>], id: &str) -> Option<EntityVersion> {
        entities
            .iter()
            .find(|entity| entity.id == id)
            .map(|entity| entity.version.clone())
    }
    fn tombstone(tombstones: &[SyncTombstone], id: &str) -> Option<EntityVersion> {
        tombstones
            .iter()
            .find(|item| item.entity_id == id)
            .map(|item| item.version.clone())
    }

    match entity_type {
        "session" => active(&payload.sessions, entity_id)
            .or_else(|| tombstone(&payload.deleted_sessions, entity_id)),
        "managed-key" => active(&payload.managed_keys, entity_id)
            .or_else(|| tombstone(&payload.deleted_managed_keys, entity_id)),
        "connection-group" => active(&payload.connection_groups, entity_id)
            .or_else(|| tombstone(&payload.deleted_connection_groups, entity_id)),
        "quick-command-category" => active(&payload.quick_command_categories, entity_id)
            .or_else(|| tombstone(&payload.deleted_quick_command_categories, entity_id)),
        "quick-command" => active(&payload.quick_commands, entity_id)
            .or_else(|| tombstone(&payload.deleted_quick_commands, entity_id)),
        _ => None,
    }
}
fn stable_entity_id(kind: &str, value: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(kind.as_bytes());
    digest.update([0]);
    digest.update(value.as_bytes());
    format!("{kind}:{}", hex::encode(digest.finalize()))
}
pub fn serialize_payload(payload: &V3SyncPayload) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec_pretty(payload)
        .map_err(|error| anyhow::anyhow!("serialize v3 sync payload: {error}"))
}

pub fn parse_payload(raw: &[u8]) -> anyhow::Result<V3SyncPayload> {
    let payload: V3SyncPayload = serde_json::from_slice(raw)
        .map_err(|error| anyhow::anyhow!("parse v3 sync payload: {error}"))?;
    if payload.schema_version != V3_FORMAT_VERSION {
        return Err(anyhow::anyhow!(
            "unsupported synchronized configuration version {}",
            payload.schema_version
        ));
    }
    Ok(payload)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_versions_compare_generation_then_time_then_device() {
        let base = EntityVersion::initial("device-a", 10);
        assert!(base.compare(&base.next("device-a", 1)).is_lt());
        assert!(
            base.compare(&EntityVersion {
                generation: 1,
                device_id: "device-a".into(),
                updated_at: 11,
            })
            .is_lt()
        );
        assert!(
            EntityVersion::initial("device-a", 10)
                .compare(&EntityVersion::initial("device-b", 10))
                .is_lt()
        );
    }

    #[test]
    fn payload_builder_advances_existing_entity_and_tombstone_versions() {
        let mut base = V3SyncPayload::from_legacy_for_tests(
            crate::sync::model::SyncPayload::new(
                "device-a".into(),
                vec![],
                vec![],
                vec![],
                vec![],
                false,
                "",
            )
            .unwrap(),
        );
        let version = EntityVersion {
            generation: 7,
            device_id: "device-a".into(),
            updated_at: 10,
        };
        base.deleted_sessions.push(SyncTombstone {
            entity_type: "session".into(),
            entity_id: "session-1".into(),
            version,
            deleted_at: 10,
        });
        let config = crate::sync::merge::MergedConfig {
            sessions: vec![crate::session::config::Session {
                id: "session-1".into(),
                name: "restored".into(),
                host: "example.test".into(),
                port: 22,
                user: "alice".into(),
                auth: crate::session::config::AuthMethod::Password,
                password: String::new(),
                private_key_path: String::new(),
                private_key_inline: String::new(),
                passphrase: String::new(),
                managed_key_id: None,
                last_used: None,
                group: None,
                proxy_type: String::new(),
                proxy_host: String::new(),
                proxy_port: None,
                proxy_user: String::new(),
                proxy_password: String::new(),
            }],
            deleted_sessions: vec![],
            deleted_connection_groups: vec![],
            connection_groups: vec![],
            managed_keys: vec![],
            quick_command_categories: vec![],
            decrypted_count: 0,
            unavailable_secret_count: 0,
            unavailable_session_secret_count: 0,
            unavailable_managed_key_secret_count: 0,
            base_payload: None,
        };

        let payload =
            V3SyncPayload::from_config("device-b".into(), &config, false, "", Some(&base)).unwrap();

        assert_eq!(payload.sessions[0].version.generation, 8);
        assert_eq!(payload.sessions[0].version.device_id, "device-b");
    }
    #[test]
    fn equal_active_and_tombstone_versions_are_not_active() {
        let version = EntityVersion::initial("device-a", 10);
        let entity = SyncEntity {
            id: "session-1".into(),
            version: version.clone(),
            value: SyncSession {
                id: "session-1".into(),
                name: "test".into(),
                host: "example.test".into(),
                port: 22,
                user: "alice".into(),
                auth: crate::session::config::AuthMethod::Password,
                password: SyncSecret::Omitted,
                private_key_path: String::new(),
                private_key_inline: SyncSecret::Omitted,
                passphrase: SyncSecret::Omitted,
                managed_key_id: None,
                last_used: None,
                group: None,
                proxy_type: String::new(),
                proxy_host: String::new(),
                proxy_port: None,
                proxy_user: String::new(),
                proxy_password: SyncSecret::Omitted,
            },
        };
        let tombstone = SyncTombstone {
            entity_type: "session".into(),
            entity_id: entity.id.clone(),
            version,
            deleted_at: 10,
        };
        assert!(!crate::sync::merge::is_newer_than_tombstone(
            &entity,
            Some(&tombstone)
        ));
    }
}
