use sha2::{Digest, Sha256};

use super::{
    v2::SyncPayload,
    v3::{
        EntityVersion, SyncEntity, SyncTombstone, V3_FORMAT_VERSION, V3QuickCommand,
        V3QuickCommandCategory, V3SyncPayload,
    },
};

/// 将当前 v1/v2 兼容模型转换为稳定的 v3 内部快照。
///
/// 迁移不创建新的实体 UUID。没有实体级版本的旧记录统一使用来源包的
/// `device_id` 和 `updated_at` 生成初始版本，因此重复迁移结果保持一致。
#[allow(dead_code)]
pub fn migrate_to_v3(payload: SyncPayload) -> V3SyncPayload {
    let updated_at = parse_timestamp(&payload.updated_at);
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
    let connection_groups = payload
        .connection_groups
        .into_iter()
        .map(|value| SyncEntity {
            id: legacy_id("connection-group", &value),
            version: version(),
            value,
        })
        .collect();
    let mut quick_command_categories = Vec::new();
    let mut quick_commands = Vec::new();
    for category in payload.quick_command_categories {
        let category_id = category.id;
        quick_commands.extend(category.commands.into_iter().map(|command| SyncEntity {
            id: command.id,
            version: version(),
            value: V3QuickCommand {
                category_id: category_id.clone(),
                name: command.name,
                remark: command.remark,
                command: command.command,
            },
        }));
        quick_command_categories.push(SyncEntity {
            id: category_id,
            version: version(),
            value: V3QuickCommandCategory {
                name: category.name,
            },
        });
    }

    let mut deleted_sessions = Vec::new();
    let mut deleted_session_snapshots = Vec::new();
    for deleted in payload.deleted_sessions {
        let id = deleted.session.id.clone();
        let entity_version = EntityVersion::initial(device_id.clone(), deleted.deleted_at);
        deleted_sessions.push(SyncTombstone {
            entity_type: "session".to_string(),
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
        let id = legacy_id("connection-group", &deleted.name);
        let entity_version = EntityVersion::initial(device_id.clone(), deleted.deleted_at);
        deleted_connection_groups.push(SyncTombstone {
            entity_type: "connection-group".to_string(),
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

    V3SyncPayload {
        schema_version: V3_FORMAT_VERSION,
        revision: payload.revision,
        updated_at: payload.updated_at,
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
        privacy_password_verifier: payload.privacy_password_verifier,
        secrets: Vec::new(),
    }
}

fn parse_timestamp(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.timestamp())
        .unwrap_or_default()
}

fn legacy_id(kind: &str, value: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(kind.as_bytes());
    digest.update([0]);
    digest.update(value.as_bytes());
    let digest = digest.finalize();
    let encoded = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("legacy:{kind}:{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::config::{
        AuthMethod, ManagedKey, QuickCommand, QuickCommandCategory, Session,
    };
    use crate::sync::model::{
        SyncDeletedConnectionGroup, SyncDeletedSession, SyncPayload, SyncSecret,
    };

    fn session() -> Session {
        Session {
            id: "session-1".into(),
            name: "Production".into(),
            host: "example.test".into(),
            port: 22,
            user: "alice".into(),
            auth: AuthMethod::Key,
            password: String::new(),
            private_key_path: String::new(),
            private_key_inline: String::new(),
            passphrase: String::new(),
            managed_key_id: Some("key-1".into()),
            last_used: None,
            group: Some("prod".into()),
            proxy_type: "none".into(),
            proxy_host: String::new(),
            proxy_port: None,
            proxy_user: String::new(),
            proxy_password: String::new(),
        }
    }

    fn managed_key() -> ManagedKey {
        ManagedKey {
            id: "key-1".into(),
            name: "Production key".into(),
            key_type: "ed25519".into(),
            fingerprint: "SHA256:test".into(),
            inline_content: String::new(),
            passphrase: String::new(),
            created_at: 1,
        }
    }

    fn payload() -> SyncPayload {
        let mut sync_session =
            crate::sync::model::SyncSession::export(session(), false, "").unwrap();
        sync_session.password = SyncSecret::Omitted;
        sync_session.private_key_inline = SyncSecret::Empty;
        sync_session.passphrase = SyncSecret::Encrypted("sealed-passphrase".into());
        sync_session.proxy_password = SyncSecret::LegacyPlaintext("legacy-proxy".into());
        let mut sync_key =
            crate::sync::model::SyncManagedKey::export(managed_key(), false, "").unwrap();
        sync_key.inline_content = SyncSecret::Encrypted("sealed-key".into());
        sync_key.passphrase = SyncSecret::Empty;

        SyncPayload {
            schema_version: 2,
            revision: "revision-1".into(),
            updated_at: "2026-01-02T03:04:05Z".into(),
            device_id: "device-1".into(),
            privacy_password_verifier: None,
            connection_groups: vec!["prod".into()],
            sessions: vec![sync_session.clone()],
            deleted_sessions: vec![SyncDeletedSession {
                session: sync_session.clone(),
                deleted_at: 123,
            }],
            deleted_connection_groups: vec![SyncDeletedConnectionGroup {
                name: "prod".into(),
                groups: vec!["prod".into()],
                sessions: vec![sync_session],
                deleted_at: 124,
            }],
            managed_keys: vec![sync_key],
            quick_command_categories: vec![QuickCommandCategory {
                id: "category-1".into(),
                name: "Operations".into(),
                commands: vec![QuickCommand {
                    id: "command-1".into(),
                    name: "Status".into(),
                    remark: "Read only".into(),
                    command: "systemctl status app".into(),
                }],
            }],
        }
    }

    #[test]
    fn migration_preserves_entities_secrets_tombstones_and_is_deterministic() {
        let first = migrate_to_v3(payload());
        let second = migrate_to_v3(payload());
        let first_json = serde_json::to_vec(&first).unwrap();
        let second_json = serde_json::to_vec(&second).unwrap();

        assert_eq!(first_json, second_json);
        assert_eq!(first.sessions[0].id, "session-1");
        assert_eq!(first.managed_keys[0].id, "key-1");
        assert_eq!(first.sessions[0].version.updated_at, 1_767_323_045);
        assert_eq!(
            first.connection_groups[0].id,
            legacy_id("connection-group", "prod")
        );
        assert!(matches!(
            first.sessions[0].value.password,
            SyncSecret::Omitted
        ));
        assert!(matches!(
            first.sessions[0].value.private_key_inline,
            SyncSecret::Empty
        ));
        assert!(matches!(
            first.sessions[0].value.passphrase,
            SyncSecret::Encrypted(ref value) if value == "sealed-passphrase"
        ));
        assert!(matches!(
            first.sessions[0].value.proxy_password,
            SyncSecret::LegacyPlaintext(ref value) if value == "legacy-proxy"
        ));
        assert_eq!(first.deleted_sessions[0].entity_id, "session-1");
        assert_eq!(first.deleted_sessions[0].deleted_at, 123);
        assert_eq!(first.deleted_connection_groups[0].deleted_at, 124);
        assert_eq!(first.quick_command_categories[0].id, "category-1");
        assert_eq!(first.quick_commands[0].id, "command-1");
        assert_eq!(first.quick_commands[0].value.category_id, "category-1");
    }

    #[test]
    fn parsed_v1_payload_migrates_with_stable_ids_and_legacy_secret_states() {
        let raw = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "revision": "legacy-revision",
            "updated_at": "2026-01-02T03:04:05Z",
            "device_id": "legacy-device",
            "sessions": [session()],
            "managed_keys": [managed_key()]
        }))
        .unwrap();
        let parsed = crate::sync::model::parse_payload(&raw, "").unwrap();

        let migrated = migrate_to_v3(parsed);

        assert_eq!(migrated.sessions[0].id, "session-1");
        assert_eq!(migrated.managed_keys[0].id, "key-1");
        assert!(matches!(
            migrated.sessions[0].value.password,
            SyncSecret::Omitted
        ));
        assert!(matches!(
            migrated.sessions[0].value.managed_key_id.as_deref(),
            Some("key-1")
        ));
    }
}
