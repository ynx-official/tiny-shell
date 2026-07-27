use crate::{
    session::config::{
        DeletedConnectionGroup, DeletedSession, ManagedKey, QuickCommand, QuickCommandCategory,
        Session,
    },
    sync::{
        model::{
            SyncDeletedConnectionGroup, SyncDeletedSession, SyncManagedKey, SyncPayload,
            SyncSession,
        },
        secrets::{SecretResolutionStats, resolve_secret},
    },
};

#[derive(Clone)]
pub struct MergedConfig {
    pub sessions: Vec<Session>,
    pub deleted_sessions: Vec<DeletedSession>,
    pub deleted_connection_groups: Vec<DeletedConnectionGroup>,
    pub connection_groups: Vec<String>,
    pub managed_keys: Vec<ManagedKey>,
    pub quick_command_categories: Vec<QuickCommandCategory>,
    pub decrypted_count: u32,
    pub unavailable_secret_count: u32,
    pub unavailable_session_secret_count: u32,
    pub unavailable_managed_key_secret_count: u32,
}

pub struct MergeLocal<'a> {
    pub sessions: &'a [Session],
    pub deleted_sessions: &'a [DeletedSession],
    pub connection_groups: &'a [String],
    pub deleted_connection_groups: &'a [DeletedConnectionGroup],
    pub keys: &'a [ManagedKey],
    pub commands: &'a [QuickCommandCategory],
}

pub fn merge_payload(
    local_sessions: &[Session],
    local_connection_groups: &[String],
    local_keys: &[ManagedKey],
    local_commands: &[QuickCommandCategory],
    remote: SyncPayload,
    privacy_password: &str,
) -> MergedConfig {
    merge_payload_with_secret_access(
        MergeLocal {
            sessions: local_sessions,
            deleted_sessions: &[],
            connection_groups: local_connection_groups,
            deleted_connection_groups: &[],
            keys: local_keys,
            commands: local_commands,
        },
        remote,
        Some(privacy_password),
    )
}

pub fn merge_public_payload(
    local_sessions: &[Session],
    local_connection_groups: &[String],
    local_keys: &[ManagedKey],
    local_commands: &[QuickCommandCategory],
    remote: SyncPayload,
) -> MergedConfig {
    merge_payload_with_secret_access(
        MergeLocal {
            sessions: local_sessions,
            deleted_sessions: &[],
            connection_groups: local_connection_groups,
            deleted_connection_groups: &[],
            keys: local_keys,
            commands: local_commands,
        },
        remote,
        None,
    )
}

pub fn merge_payload_with_deleted(
    local: MergeLocal<'_>,
    remote: SyncPayload,
    privacy_password: &str,
) -> MergedConfig {
    merge_payload_with_secret_access(local, remote, Some(privacy_password))
}

fn merge_payload_with_secret_access(
    local: MergeLocal<'_>,
    remote: SyncPayload,
    privacy_password: Option<&str>,
) -> MergedConfig {
    merge_payload_with_secret_access_and_deleted(local, remote, privacy_password)
}

fn merge_payload_with_secret_access_and_deleted(
    local: MergeLocal<'_>,
    remote: SyncPayload,
    privacy_password: Option<&str>,
) -> MergedConfig {
    let mut session_stats = SecretResolutionStats::default();
    let mut managed_key_stats = SecretResolutionStats::default();
    let remote_deleted_sessions = remote.deleted_sessions;
    let remote_deleted_groups = remote.deleted_connection_groups;
    let sessions = merge_sessions(
        local.sessions,
        remote.sessions,
        privacy_password,
        &mut session_stats,
    );
    let managed_keys = merge_keys(
        local.keys,
        remote.managed_keys,
        privacy_password,
        &mut managed_key_stats,
    );
    let deleted_sessions = merge_deleted_sessions(
        local.deleted_sessions,
        remote_deleted_sessions,
        local.sessions,
        privacy_password,
        &mut session_stats,
    );
    let deleted_connection_groups = merge_deleted_groups(
        local.deleted_connection_groups,
        remote_deleted_groups,
        local.deleted_sessions,
        privacy_password,
        &mut session_stats,
    );
    let mut sessions = sessions;
    let mut connection_groups =
        merge_unique_strings(local.connection_groups, remote.connection_groups);
    apply_deleted_projection(
        &mut sessions,
        &mut connection_groups,
        &deleted_sessions,
        &deleted_connection_groups,
    );

    MergedConfig {
        sessions,
        deleted_sessions,
        deleted_connection_groups,
        connection_groups,
        managed_keys,
        quick_command_categories: merge_command_categories(
            local.commands,
            remote.quick_command_categories,
        ),
        decrypted_count: session_stats.decrypted_count + managed_key_stats.decrypted_count,
        unavailable_secret_count: session_stats.unavailable_count
            + managed_key_stats.unavailable_count,
        unavailable_session_secret_count: session_stats.unavailable_count,
        unavailable_managed_key_secret_count: managed_key_stats.unavailable_count,
    }
}

fn merge_deleted_sessions(
    local_deleted: &[DeletedSession],
    remote_deleted: Vec<SyncDeletedSession>,
    local_sessions: &[Session],
    password: Option<&str>,
    stats: &mut SecretResolutionStats,
) -> Vec<DeletedSession> {
    let mut merged = local_deleted.to_vec();
    for remote in remote_deleted {
        let local = merged
            .iter()
            .find(|item| item.session.id == remote.session.id)
            .map(|item| &item.session);
        let local_active = local_sessions
            .iter()
            .find(|item| item.id == remote.session.id)
            .or(local);
        let session = session_from_remote(remote.session, local_active, password, stats);
        if let Some(existing) = merged.iter_mut().find(|item| item.session.id == session.id) {
            if remote.deleted_at >= existing.deleted_at {
                *existing = DeletedSession {
                    session,
                    deleted_at: remote.deleted_at,
                };
            }
        } else {
            merged.push(DeletedSession {
                session,
                deleted_at: remote.deleted_at,
            });
        }
    }
    merged
}

fn merge_deleted_groups(
    local_deleted: &[DeletedConnectionGroup],
    remote_deleted: Vec<SyncDeletedConnectionGroup>,
    local_deleted_sessions: &[DeletedSession],
    password: Option<&str>,
    stats: &mut SecretResolutionStats,
) -> Vec<DeletedConnectionGroup> {
    let mut merged = local_deleted.to_vec();
    for remote in remote_deleted {
        let sessions = remote
            .sessions
            .into_iter()
            .map(|session| {
                let local = local_deleted_sessions
                    .iter()
                    .find(|item| item.session.id == session.id)
                    .map(|item| &item.session);
                session_from_remote(session, local, password, stats)
            })
            .collect();
        let group = DeletedConnectionGroup {
            name: remote.name,
            groups: remote.groups,
            sessions,
            deleted_at: remote.deleted_at,
        };
        if let Some(existing) = merged.iter_mut().find(|item| item.name == group.name) {
            if group.deleted_at >= existing.deleted_at {
                *existing = group;
            }
        } else {
            merged.push(group);
        }
    }
    merged
}

fn apply_deleted_projection(
    sessions: &mut Vec<Session>,
    groups: &mut Vec<String>,
    deleted_sessions: &[DeletedSession],
    deleted_groups: &[DeletedConnectionGroup],
) {
    let deleted_ids: std::collections::HashSet<&str> = deleted_sessions
        .iter()
        .map(|item| item.session.id.as_str())
        .collect();
    let deleted_group_names: Vec<&str> = deleted_groups
        .iter()
        .flat_map(|item| item.groups.iter().map(String::as_str))
        .collect();
    sessions.retain(|session| {
        !deleted_ids.contains(session.id.as_str())
            && !session.group.as_deref().is_some_and(|group| {
                deleted_group_names
                    .iter()
                    .any(|deleted| group == *deleted || group.starts_with(&format!("{deleted}/")))
            })
    });
    groups.retain(|group| {
        !deleted_group_names
            .iter()
            .any(|deleted| group == *deleted || group.starts_with(&format!("{deleted}/")))
    });
}
fn merge_sessions(
    local_sessions: &[Session],
    remote_sessions: Vec<SyncSession>,
    password: Option<&str>,
    stats: &mut SecretResolutionStats,
) -> Vec<Session> {
    let mut sessions: Vec<_> = remote_sessions
        .into_iter()
        .map(|remote| {
            let local = local_sessions
                .iter()
                .find(|local| session_matches(local, &remote));
            session_from_remote(remote, local, password, stats)
        })
        .collect();
    let local_only: Vec<_> = local_sessions
        .iter()
        .filter(|local| !sessions.iter().any(|remote| sessions_match(remote, local)))
        .cloned()
        .collect();
    sessions.extend(local_only);
    sessions
}

fn session_matches(local: &Session, remote: &SyncSession) -> bool {
    identifiers_match(&local.id, &remote.id)
        || (identifier_missing(&local.id, &remote.id)
            && local.host == remote.host
            && local.port == remote.port
            && local.user == remote.user)
}

fn sessions_match(a: &Session, b: &Session) -> bool {
    identifiers_match(&a.id, &b.id)
        || (identifier_missing(&a.id, &b.id)
            && a.host == b.host
            && a.port == b.port
            && a.user == b.user)
}

fn session_from_remote(
    remote: SyncSession,
    local: Option<&Session>,
    password: Option<&str>,
    stats: &mut SecretResolutionStats,
) -> Session {
    let id = if remote.id.is_empty() {
        local.map_or_else(String::new, |value| value.id.clone())
    } else {
        remote.id.clone()
    };
    Session {
        id,
        name: remote.name,
        host: remote.host,
        port: remote.port,
        user: remote.user,
        auth: remote.auth,
        password: resolve_secret(
            remote.password,
            local.map_or("", |value| value.password.as_str()),
            password,
            stats,
        ),
        private_key_path: remote.private_key_path,
        private_key_inline: resolve_secret(
            remote.private_key_inline,
            local.map_or("", |value| value.private_key_inline.as_str()),
            password,
            stats,
        ),
        passphrase: resolve_secret(
            remote.passphrase,
            local.map_or("", |value| value.passphrase.as_str()),
            password,
            stats,
        ),
        managed_key_id: remote.managed_key_id,
        last_used: remote.last_used,
        group: remote.group,
        proxy_type: remote.proxy_type,
        proxy_host: remote.proxy_host,
        proxy_port: remote.proxy_port,
        proxy_user: remote.proxy_user,
        proxy_password: resolve_secret(
            remote.proxy_password,
            local.map_or("", |value| value.proxy_password.as_str()),
            password,
            stats,
        ),
    }
}

fn merge_keys(
    local_keys: &[ManagedKey],
    remote_keys: Vec<SyncManagedKey>,
    password: Option<&str>,
    stats: &mut SecretResolutionStats,
) -> Vec<ManagedKey> {
    let mut keys: Vec<_> = remote_keys
        .into_iter()
        .map(|remote| {
            let local = local_keys.iter().find(|local| key_matches(local, &remote));
            key_from_remote(remote, local, password, stats)
        })
        .collect();
    let local_only: Vec<_> = local_keys
        .iter()
        .filter(|local| !keys.iter().any(|remote| managed_keys_match(remote, local)))
        .cloned()
        .collect();
    keys.extend(local_only);
    keys
}

fn key_matches(local: &ManagedKey, remote: &SyncManagedKey) -> bool {
    identifiers_match(&local.id, &remote.id)
        || (identifier_missing(&local.id, &remote.id)
            && !local.fingerprint.is_empty()
            && local.fingerprint == remote.fingerprint)
}

fn managed_keys_match(a: &ManagedKey, b: &ManagedKey) -> bool {
    identifiers_match(&a.id, &b.id)
        || (identifier_missing(&a.id, &b.id)
            && !a.fingerprint.is_empty()
            && a.fingerprint == b.fingerprint)
}

fn identifiers_match(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && a == b
}

fn identifier_missing(a: &str, b: &str) -> bool {
    a.is_empty() || b.is_empty()
}

fn key_from_remote(
    remote: SyncManagedKey,
    local: Option<&ManagedKey>,
    password: Option<&str>,
    stats: &mut SecretResolutionStats,
) -> ManagedKey {
    let id = if remote.id.is_empty() {
        local.map_or_else(String::new, |value| value.id.clone())
    } else {
        remote.id.clone()
    };
    ManagedKey {
        id,
        name: remote.name,
        key_type: remote.key_type,
        fingerprint: remote.fingerprint,
        inline_content: resolve_secret(
            remote.inline_content,
            local.map_or("", |value| value.inline_content.as_str()),
            password,
            stats,
        ),
        passphrase: resolve_secret(
            remote.passphrase,
            local.map_or("", |value| value.passphrase.as_str()),
            password,
            stats,
        ),
        created_at: remote.created_at,
    }
}

fn merge_unique_strings(local: &[String], remote: Vec<String>) -> Vec<String> {
    let mut merged = remote;
    let local_only: Vec<_> = local
        .iter()
        .filter(|value| !merged.contains(value))
        .cloned()
        .collect();
    merged.extend(local_only);
    merged
}

fn merge_command_categories(
    local: &[QuickCommandCategory],
    remote: Vec<QuickCommandCategory>,
) -> Vec<QuickCommandCategory> {
    let mut categories: Vec<_> = remote
        .into_iter()
        .map(|mut remote_category| {
            if let Some(local_category) = local
                .iter()
                .find(|local_category| local_category.id == remote_category.id)
            {
                remote_category.commands =
                    merge_commands(&local_category.commands, remote_category.commands);
            }
            remote_category
        })
        .collect();
    let local_only: Vec<_> = local
        .iter()
        .filter(|local_category| {
            !categories
                .iter()
                .any(|remote_category| remote_category.id == local_category.id)
        })
        .cloned()
        .collect();
    categories.extend(local_only);
    categories
}

fn merge_commands(local: &[QuickCommand], remote: Vec<QuickCommand>) -> Vec<QuickCommand> {
    let mut commands = remote;
    let local_only: Vec<_> = local
        .iter()
        .filter(|local_command| {
            !commands
                .iter()
                .any(|remote_command| remote_command.id == local_command.id)
        })
        .cloned()
        .collect();
    commands.extend(local_only);
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{session::config::AuthMethod, sync::model::SyncPayload};

    fn session(id: &str, name: &str, password: &str) -> Session {
        Session {
            id: id.into(),
            name: name.into(),
            host: "example.test".into(),
            port: 22,
            user: "alice".into(),
            auth: AuthMethod::Password,
            password: password.into(),
            private_key_path: String::new(),
            private_key_inline: "private-key".into(),
            passphrase: "passphrase".into(),
            managed_key_id: None,
            last_used: None,
            group: Some("Remote".into()),
            proxy_type: "none".into(),
            proxy_host: String::new(),
            proxy_port: None,
            proxy_user: String::new(),
            proxy_password: "proxy-password".into(),
        }
    }

    fn key(id: &str, name: &str, content: &str) -> ManagedKey {
        ManagedKey {
            id: id.into(),
            name: name.into(),
            key_type: "ed25519".into(),
            fingerprint: "SHA256:same".into(),
            inline_content: content.into(),
            passphrase: "key-passphrase".into(),
            created_at: 1,
        }
    }

    #[test]
    fn wrong_password_recovers_public_domains_and_keeps_matching_local_secrets() {
        let local_session = session("session-1", "Local name", "local-password");
        let local_key = key("local-key", "Local key", "local-key-content");
        let commands = vec![QuickCommandCategory {
            id: "category-1".into(),
            name: "Operations".into(),
            commands: vec![QuickCommand {
                id: "command-1".into(),
                name: "Status".into(),
                remark: String::new(),
                command: "systemctl status app".into(),
            }],
        }];
        let payload = SyncPayload::new(
            "device-1".into(),
            vec![session("session-1", "Remote name", "remote-password")],
            vec!["Remote".into()],
            vec![key("local-key", "Remote key", "remote-key-content")],
            commands.clone(),
            true,
            "correct-password",
        )
        .unwrap();

        let merged = merge_payload(
            &[local_session],
            &["Local".into()],
            &[local_key],
            &[],
            payload,
            "wrong-password",
        );

        assert_eq!(merged.sessions[0].name, "Remote name");
        assert_eq!(merged.sessions[0].password, "local-password");
        assert_eq!(merged.managed_keys[0].name, "Remote key");
        assert_eq!(merged.managed_keys[0].inline_content, "local-key-content");
        assert_eq!(merged.connection_groups, vec!["Remote", "Local"]);
        assert_eq!(
            merged.quick_command_categories[0].commands[0].command,
            commands[0].commands[0].command
        );
        assert!(merged.unavailable_secret_count > 0);
        assert!(merged.unavailable_session_secret_count > 0);
        assert!(merged.unavailable_managed_key_secret_count > 0);
    }

    #[test]
    fn public_merge_never_attempts_to_decrypt_remote_secrets() {
        let local_session = session("session-1", "Local name", "local-password");
        let payload = SyncPayload::new(
            "device-1".into(),
            vec![
                session("session-1", "Remote name", "remote-password"),
                session("remote-only", "Remote only", "remote-only-password"),
            ],
            vec!["Remote".into()],
            Vec::new(),
            Vec::new(),
            true,
            "privacy-password",
        )
        .unwrap();

        let merged = merge_public_payload(&[local_session], &[], &[], &[], payload);

        assert_eq!(merged.sessions[0].name, "Remote name");
        assert_eq!(merged.sessions[0].password, "local-password");
        assert_eq!(merged.sessions[1].name, "Remote only");
        assert!(merged.sessions[1].password.is_empty());
        assert_eq!(merged.decrypted_count, 0);
        assert_eq!(merged.unavailable_secret_count, 0);
    }

    #[test]
    fn wrong_password_leaves_new_object_secrets_empty() {
        let payload = SyncPayload::new(
            "device-1".into(),
            vec![session("remote-only", "Remote only", "remote-password")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            true,
            "correct-password",
        )
        .unwrap();

        let merged = merge_payload(&[], &[], &[], &[], payload, "wrong-password");

        assert_eq!(merged.sessions[0].host, "example.test");
        assert!(merged.sessions[0].password.is_empty());
        assert!(merged.sessions[0].private_key_inline.is_empty());
        assert!(merged.unavailable_secret_count > 0);
    }

    #[test]
    fn matching_remote_commands_override_and_local_only_commands_are_preserved() {
        let local = vec![QuickCommandCategory {
            id: "category-1".into(),
            name: "Local category".into(),
            commands: vec![
                QuickCommand {
                    id: "shared".into(),
                    name: "Local shared".into(),
                    remark: String::new(),
                    command: "local-shared".into(),
                },
                QuickCommand {
                    id: "local-only".into(),
                    name: "Local only".into(),
                    remark: String::new(),
                    command: "local-only".into(),
                },
            ],
        }];
        let remote = vec![QuickCommandCategory {
            id: "category-1".into(),
            name: "Remote category".into(),
            commands: vec![QuickCommand {
                id: "shared".into(),
                name: "Remote shared".into(),
                remark: String::new(),
                command: "remote-shared".into(),
            }],
        }];

        let merged = merge_command_categories(&local, remote);

        assert_eq!(merged[0].name, "Remote category");
        assert_eq!(merged[0].commands.len(), 2);
        assert_eq!(merged[0].commands[0].command, "remote-shared");
        assert_eq!(merged[0].commands[1].command, "local-only");
    }

    #[test]
    fn stable_ids_prevent_same_endpoint_sessions_from_collapsing() {
        let local = session("local-id", "Local", "local-password");
        let payload = SyncPayload::new(
            "device-1".into(),
            vec![session("remote-id", "Remote", "remote-password")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload(&[local], &[], &[], &[], payload, "");

        assert_eq!(merged.sessions.len(), 2);
        assert_eq!(merged.sessions[0].id, "remote-id");
        assert_eq!(merged.sessions[1].id, "local-id");
    }

    #[test]
    fn missing_legacy_session_id_falls_back_to_endpoint_identity() {
        let local = session("local-id", "Local", "local-password");
        let mut remote = session("", "Remote", "remote-password");
        remote.private_key_inline.clear();
        remote.passphrase.clear();
        remote.proxy_password.clear();
        let payload = SyncPayload::new(
            "device-1".into(),
            vec![remote],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload(&[local], &[], &[], &[], payload, "");

        assert_eq!(merged.sessions.len(), 1);
        assert_eq!(merged.sessions[0].name, "Remote");
        assert_eq!(merged.sessions[0].password, "local-password");
    }

    #[test]
    fn newer_remote_tombstones_remove_active_objects_and_survive_merge() {
        let local_session = session("session-1", "Local", "local-password");
        let mut deleted_group_session = session("session-2", "Grouped", "group-password");
        deleted_group_session.group = Some("prod/eu".into());
        let payload = SyncPayload::new_with_deleted(
            "device-1".into(),
            vec![],
            vec![DeletedSession {
                session: local_session.clone(),
                deleted_at: 20,
            }],
            vec!["prod".into(), "prod/eu".into()],
            vec![DeletedConnectionGroup {
                name: "prod".into(),
                groups: vec!["prod".into(), "prod/eu".into()],
                sessions: vec![deleted_group_session],
                deleted_at: 30,
            }],
            vec![],
            vec![],
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload_with_deleted(
            MergeLocal {
                sessions: &[local_session],
                deleted_sessions: &[],
                connection_groups: &["prod".into(), "prod/eu".into()],
                deleted_connection_groups: &[],
                keys: &[],
                commands: &[],
            },
            payload,
            "",
        );

        assert!(merged.sessions.is_empty());
        assert!(merged.connection_groups.is_empty());
        assert_eq!(merged.deleted_sessions.len(), 1);
        assert_eq!(merged.deleted_connection_groups.len(), 1);
        assert_eq!(merged.deleted_connection_groups[0].deleted_at, 30);
    }

    #[test]
    fn local_tombstones_prevent_remote_active_objects_from_returning() {
        let local = session("session-1", "Local", "local-password");
        let payload = SyncPayload::new(
            "device-1".into(),
            vec![local.clone()],
            vec!["prod".into()],
            Vec::new(),
            Vec::new(),
            false,
            "",
        )
        .unwrap();
        let deleted = DeletedSession {
            session: local,
            deleted_at: 100,
        };

        let merged = merge_payload_with_deleted(
            MergeLocal {
                sessions: &[],
                deleted_sessions: &[deleted],
                connection_groups: &["prod".into()],
                deleted_connection_groups: &[],
                keys: &[],
                commands: &[],
            },
            payload,
            "",
        );

        assert!(merged.sessions.is_empty());
        assert_eq!(merged.deleted_sessions.len(), 1);
    }
}
