use std::{cmp::Ordering, collections::HashSet};

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
            .collect::<Vec<_>>();
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
        let deleted_sessions = complete_tombstones(
            "session",
            sessions.iter().map(|entity| entity.id.clone()).collect(),
            deleted_sessions,
            base_payload,
            &device_id,
            updated_at,
        );
        let deleted_managed_keys = complete_tombstones(
            "managed-key",
            managed_keys
                .iter()
                .map(|entity| entity.id.clone())
                .collect(),
            Vec::new(),
            base_payload,
            &device_id,
            updated_at,
        );
        let deleted_connection_groups = complete_tombstones(
            "connection-group",
            connection_groups
                .iter()
                .map(|entity| entity.id.clone())
                .collect(),
            deleted_connection_groups,
            base_payload,
            &device_id,
            updated_at,
        );
        let deleted_quick_command_categories = complete_tombstones(
            "quick-command-category",
            quick_command_categories
                .iter()
                .map(|entity| entity.id.clone())
                .collect(),
            Vec::new(),
            base_payload,
            &device_id,
            updated_at,
        );
        let deleted_quick_commands = complete_tombstones(
            "quick-command",
            quick_commands
                .iter()
                .map(|entity| entity.id.clone())
                .collect(),
            Vec::new(),
            base_payload,
            &device_id,
            updated_at,
        );
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
            deleted_managed_keys,
            deleted_connection_groups,
            deleted_quick_command_categories,
            deleted_quick_commands,
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

    /// Compare synchronized content without treating transport metadata as a
    /// local configuration change. Revisions and entity versions are generated
    /// during serialization, so comparing the raw payload would upload on every
    /// reconciliation cycle.
    pub fn is_content_equivalent(&self, other: &Self) -> bool {
        fn normalize(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(object) => {
                    object.remove("revision");
                    object.remove("updated_at");
                    object.remove("device_id");
                    object.remove("version");
                    for value in object.values_mut() {
                        normalize(value);
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        normalize(value);
                    }
                }
                _ => {}
            }
        }

        let Ok(mut left) = serde_json::to_value(self) else {
            return false;
        };
        let Ok(mut right) = serde_json::to_value(other) else {
            return false;
        };
        normalize(&mut left);
        normalize(&mut right);
        left == right
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

fn complete_tombstones(
    entity_type: &str,
    active_ids: HashSet<String>,
    mut tombstones: Vec<SyncTombstone>,
    base_payload: Option<&V3SyncPayload>,
    device_id: &str,
    updated_at: i64,
) -> Vec<SyncTombstone> {
    let Some(base) = base_payload else {
        return tombstones;
    };
    let mut deleted_ids: HashSet<String> = tombstones
        .iter()
        .map(|item| item.entity_id.clone())
        .collect();
    let base_tombstones: &[SyncTombstone] = match entity_type {
        "session" => &base.deleted_sessions,
        "managed-key" => &base.deleted_managed_keys,
        "connection-group" => &base.deleted_connection_groups,
        "quick-command-category" => &base.deleted_quick_command_categories,
        "quick-command" => &base.deleted_quick_commands,
        _ => &[],
    };
    for tombstone in base_tombstones {
        if !active_ids.contains(&tombstone.entity_id)
            && deleted_ids.insert(tombstone.entity_id.clone())
        {
            tombstones.push(tombstone.clone());
        }
    }
    let base_active_ids: Vec<&str> = match entity_type {
        "session" => base
            .sessions
            .iter()
            .map(|entity| entity.id.as_str())
            .collect(),
        "managed-key" => base
            .managed_keys
            .iter()
            .map(|entity| entity.id.as_str())
            .collect(),
        "connection-group" => base
            .connection_groups
            .iter()
            .map(|entity| entity.id.as_str())
            .collect(),
        "quick-command-category" => base
            .quick_command_categories
            .iter()
            .map(|entity| entity.id.as_str())
            .collect(),
        "quick-command" => base
            .quick_commands
            .iter()
            .map(|entity| entity.id.as_str())
            .collect(),
        _ => Vec::new(),
    };
    for id in base_active_ids {
        if active_ids.contains(id) || !deleted_ids.insert(id.to_string()) {
            continue;
        }
        let version = find_entity_version(base, entity_type, id).map_or_else(
            || EntityVersion::initial(device_id.to_string(), updated_at),
            |version| version.next(device_id.to_string(), updated_at),
        );
        tombstones.push(SyncTombstone {
            entity_type: entity_type.to_string(),
            entity_id: id.to_string(),
            version,
            deleted_at: updated_at,
        });
    }
    tombstones
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
pub(crate) fn stable_entity_id(kind: &str, value: &str) -> String {
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
                connection_type: crate::session::config::ConnectionType::Ssh,
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
    fn payload_builder_creates_and_preserves_tombstones_for_removed_entities() {
        let mut base_config = crate::sync::merge::MergedConfig {
            sessions: vec![],
            deleted_sessions: vec![],
            deleted_connection_groups: vec![],
            connection_groups: vec![],
            managed_keys: vec![crate::session::config::ManagedKey {
                id: "key-1".into(),
                name: "Primary".into(),
                key_type: "ed25519".into(),
                fingerprint: "SHA256:test".into(),
                inline_content: String::new(),
                passphrase: String::new(),
                created_at: 1,
            }],
            quick_command_categories: vec![crate::session::config::QuickCommandCategory {
                id: "category-1".into(),
                name: "Ops".into(),
                commands: vec![crate::session::config::QuickCommand {
                    id: "command-1".into(),
                    name: "Status".into(),
                    remark: String::new(),
                    command: "uptime".into(),
                }],
            }],
            decrypted_count: 0,
            unavailable_secret_count: 0,
            unavailable_session_secret_count: 0,
            unavailable_managed_key_secret_count: 0,
            base_payload: None,
        };
        let base =
            V3SyncPayload::from_config("device-a".into(), &base_config, false, "", None).unwrap();
        base_config.managed_keys.clear();
        base_config.quick_command_categories.clear();

        let removed =
            V3SyncPayload::from_config("device-b".into(), &base_config, false, "", Some(&base))
                .unwrap();
        assert_eq!(removed.deleted_managed_keys[0].entity_id, "key-1");
        assert_eq!(
            removed.deleted_quick_command_categories[0].entity_id,
            "category-1"
        );
        assert_eq!(removed.deleted_quick_commands[0].entity_id, "command-1");

        let repeated =
            V3SyncPayload::from_config("device-b".into(), &base_config, false, "", Some(&removed))
                .unwrap();
        assert_eq!(repeated.deleted_managed_keys, removed.deleted_managed_keys);
        assert_eq!(
            repeated.deleted_quick_command_categories,
            removed.deleted_quick_command_categories
        );
        assert_eq!(
            repeated.deleted_quick_commands,
            removed.deleted_quick_commands
        );
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
                connection_type: crate::session::config::ConnectionType::Ssh,
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

    #[test]
    fn content_equivalence_ignores_transport_and_entity_metadata() {
        let mut left = V3SyncPayload::from_legacy_for_tests(
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
        left.connection_groups.push(SyncEntity {
            id: "group-1".into(),
            version: EntityVersion::initial("device-a", 10),
            value: "Production".into(),
        });
        let mut right = left.clone();
        right.revision = "different-revision".into();
        right.updated_at = "2026-08-14T00:00:00Z".into();
        right.device_id = "device-b".into();
        right.connection_groups[0].version = EntityVersion::initial("device-b", 99);

        assert!(left.is_content_equivalent(&right));
    }

    #[test]
    fn content_equivalence_detects_synchronized_value_changes() {
        let left = V3SyncPayload::from_legacy_for_tests(
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
        let mut right = left.clone();
        right.privacy_password_verifier = Some("changed".into());

        assert!(!left.is_content_equivalent(&right));
    }
}
