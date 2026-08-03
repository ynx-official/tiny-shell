use std::collections::{HashMap, HashSet};

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
    merge_payload_with_deleted(
        MergeLocal {
            sessions: local_sessions,
            deleted_sessions: &[],
            connection_groups: local_connection_groups,
            deleted_connection_groups: &[],
            keys: local_keys,
            commands: local_commands,
        },
        remote,
        privacy_password,
    )
}

#[allow(dead_code)]
pub fn merge_public_payload(
    local_sessions: &[Session],
    local_connection_groups: &[String],
    local_keys: &[ManagedKey],
    local_commands: &[QuickCommandCategory],
    remote: SyncPayload,
) -> MergedConfig {
    merge_public_payload_with_deleted(
        MergeLocal {
            sessions: local_sessions,
            deleted_sessions: &[],
            connection_groups: local_connection_groups,
            deleted_connection_groups: &[],
            keys: local_keys,
            commands: local_commands,
        },
        remote,
    )
}

pub fn merge_payload_with_deleted(
    local: MergeLocal<'_>,
    remote: SyncPayload,
    privacy_password: &str,
) -> MergedConfig {
    merge_payload_with_secret_access(local, remote, Some(privacy_password))
}

pub fn merge_public_payload_with_deleted(
    local: MergeLocal<'_>,
    remote: SyncPayload,
) -> MergedConfig {
    merge_payload_with_secret_access(local, remote, None)
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
    tracing::debug!(
        local_sessions = local.sessions.len(),
        remote_sessions = remote.sessions.len(),
        local_managed_keys = local.keys.len(),
        remote_managed_keys = remote.managed_keys.len(),
        local_deleted_sessions = local.deleted_sessions.len(),
        remote_deleted_sessions = remote.deleted_sessions.len(),
        "starting sync payload merge"
    );
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
    let mut deleted_index: HashMap<String, usize> = merged
        .iter()
        .enumerate()
        .map(|(index, item)| (item.session.id.clone(), index))
        .collect();
    let active_index: HashMap<&str, &Session> = local_sessions
        .iter()
        .map(|session| (session.id.as_str(), session))
        .collect();

    for remote in remote_deleted {
        let id = remote.session.id.clone();
        let local = active_index.get(id.as_str()).copied().or_else(|| {
            deleted_index
                .get(id.as_str())
                .and_then(|index| merged.get(*index))
                .map(|item| &item.session)
        });
        let deleted_at = remote.deleted_at;
        let session = session_from_remote(remote.session, local, password, stats);
        if let Some(index) = deleted_index.get(&id).copied() {
            if deleted_at >= merged[index].deleted_at {
                merged[index] = DeletedSession {
                    session,
                    deleted_at,
                };
            }
        } else {
            deleted_index.insert(id, merged.len());
            merged.push(DeletedSession {
                session,
                deleted_at,
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
    let mut group_index: HashMap<String, usize> = merged
        .iter()
        .enumerate()
        .map(|(index, item)| (item.name.clone(), index))
        .collect();
    let session_index: HashMap<&str, &Session> = local_deleted_sessions
        .iter()
        .map(|item| (item.session.id.as_str(), &item.session))
        .collect();

    for remote in remote_deleted {
        let sessions = remote
            .sessions
            .into_iter()
            .map(|session| {
                let local = session_index.get(session.id.as_str()).copied();
                session_from_remote(session, local, password, stats)
            })
            .collect();
        let group = DeletedConnectionGroup {
            name: remote.name,
            groups: remote.groups,
            sessions,
            deleted_at: remote.deleted_at,
        };
        if let Some(index) = group_index.get(group.name.as_str()).copied() {
            if group.deleted_at >= merged[index].deleted_at {
                merged[index] = group;
            }
        } else {
            group_index.insert(group.name.clone(), merged.len());
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
    let deleted_ids: HashSet<&str> = deleted_sessions
        .iter()
        .map(|item| item.session.id.as_str())
        .collect();
    let deleted_group_names: HashSet<&str> = deleted_groups
        .iter()
        .flat_map(|item| item.groups.iter().map(String::as_str))
        .collect();

    sessions.retain(|session| {
        !deleted_ids.contains(session.id.as_str())
            && !session
                .group
                .as_deref()
                .is_some_and(|group| group_is_deleted(group, &deleted_group_names))
    });
    groups.retain(|group| !group_is_deleted(group, &deleted_group_names));
}

fn group_is_deleted(group: &str, deleted_groups: &HashSet<&str>) -> bool {
    if deleted_groups.contains(group) {
        return true;
    }

    let mut boundary = 0;
    while let Some(relative) = group[boundary..].find('/') {
        boundary += relative;
        if deleted_groups.contains(&group[..boundary]) {
            return true;
        }
        boundary += 1;
    }
    false
}

struct SessionIndex<'a> {
    by_id: HashMap<&'a str, usize>,
    by_endpoint: HashMap<(&'a str, u16, &'a str), usize>,
    by_endpoint_without_id: HashMap<(&'a str, u16, &'a str), usize>,
}

impl<'a> SessionIndex<'a> {
    fn new(sessions: &'a [Session]) -> Self {
        let mut index = Self {
            by_id: HashMap::with_capacity(sessions.len()),
            by_endpoint: HashMap::with_capacity(sessions.len()),
            by_endpoint_without_id: HashMap::new(),
        };
        for (position, session) in sessions.iter().enumerate() {
            if !session.id.is_empty() {
                index.by_id.entry(session.id.as_str()).or_insert(position);
            } else {
                index
                    .by_endpoint_without_id
                    .entry((session.host.as_str(), session.port, session.user.as_str()))
                    .or_insert(position);
            }
            index
                .by_endpoint
                .entry((session.host.as_str(), session.port, session.user.as_str()))
                .or_insert(position);
        }
        index
    }

    fn find(&self, remote: &SyncSession) -> Option<usize> {
        if !remote.id.is_empty() {
            self.by_id.get(remote.id.as_str()).copied().or_else(|| {
                self.by_endpoint_without_id
                    .get(&(remote.host.as_str(), remote.port, remote.user.as_str()))
                    .copied()
            })
        } else {
            self.by_endpoint
                .get(&(remote.host.as_str(), remote.port, remote.user.as_str()))
                .copied()
        }
    }
}

fn merge_sessions(
    local_sessions: &[Session],
    remote_sessions: Vec<SyncSession>,
    password: Option<&str>,
    stats: &mut SecretResolutionStats,
) -> Vec<Session> {
    let index = SessionIndex::new(local_sessions);
    let mut consumed_local = HashSet::with_capacity(local_sessions.len());
    let mut sessions = Vec::with_capacity(local_sessions.len() + remote_sessions.len());

    for remote in remote_sessions {
        let local_index = index.find(&remote);
        let local = local_index.map(|position| &local_sessions[position]);
        if let Some(position) = local_index {
            consumed_local.insert(position);
        }
        sessions.push(session_from_remote(remote, local, password, stats));
    }

    sessions.extend(
        local_sessions
            .iter()
            .enumerate()
            .filter(|(position, _)| !consumed_local.contains(position))
            .map(|(_, local)| local.clone()),
    );
    sessions
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

struct ManagedKeyIndex<'a> {
    by_id: HashMap<&'a str, usize>,
    by_fingerprint: HashMap<&'a str, usize>,
    by_fingerprint_without_id: HashMap<&'a str, usize>,
}

impl<'a> ManagedKeyIndex<'a> {
    fn new(keys: &'a [ManagedKey]) -> Self {
        let mut index = Self {
            by_id: HashMap::with_capacity(keys.len()),
            by_fingerprint: HashMap::new(),
            by_fingerprint_without_id: HashMap::new(),
        };
        for (position, key) in keys.iter().enumerate() {
            if !key.id.is_empty() {
                index.by_id.entry(key.id.as_str()).or_insert(position);
            }
            if !key.fingerprint.is_empty() {
                index
                    .by_fingerprint
                    .entry(key.fingerprint.as_str())
                    .or_insert(position);
                if key.id.is_empty() {
                    index
                        .by_fingerprint_without_id
                        .entry(key.fingerprint.as_str())
                        .or_insert(position);
                }
            }
        }
        index
    }

    fn find(&self, remote: &SyncManagedKey) -> Option<usize> {
        if !remote.id.is_empty() {
            self.by_id.get(remote.id.as_str()).copied().or_else(|| {
                self.by_fingerprint_without_id
                    .get(remote.fingerprint.as_str())
                    .copied()
            })
        } else {
            self.by_fingerprint
                .get(remote.fingerprint.as_str())
                .copied()
        }
    }
}

fn merge_keys(
    local_keys: &[ManagedKey],
    remote_keys: Vec<SyncManagedKey>,
    password: Option<&str>,
    stats: &mut SecretResolutionStats,
) -> Vec<ManagedKey> {
    let index = ManagedKeyIndex::new(local_keys);
    let mut consumed_local = HashSet::with_capacity(local_keys.len());
    let mut keys = Vec::with_capacity(local_keys.len() + remote_keys.len());

    for remote in remote_keys {
        let local_index = index.find(&remote);
        let local = local_index.map(|position| &local_keys[position]);
        if let Some(position) = local_index {
            consumed_local.insert(position);
        }
        keys.push(key_from_remote(remote, local, password, stats));
    }

    keys.extend(
        local_keys
            .iter()
            .enumerate()
            .filter(|(position, _)| !consumed_local.contains(position))
            .map(|(_, local)| local.clone()),
    );
    keys
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
    let mut seen: HashSet<String> = remote.iter().cloned().collect();
    let mut merged = remote;
    merged.extend(
        local
            .iter()
            .filter(|value| seen.insert((*value).clone()))
            .cloned(),
    );
    merged
}

fn merge_command_categories(
    local: &[QuickCommandCategory],
    remote: Vec<QuickCommandCategory>,
) -> Vec<QuickCommandCategory> {
    let local_by_id: HashMap<&str, &QuickCommandCategory> = local
        .iter()
        .map(|category| (category.id.as_str(), category))
        .collect();
    let remote_ids: HashSet<String> = remote.iter().map(|category| category.id.clone()).collect();

    let mut categories: Vec<_> = remote
        .into_iter()
        .map(|mut remote_category| {
            if let Some(local_category) = local_by_id.get(remote_category.id.as_str()) {
                remote_category.commands =
                    merge_commands(&local_category.commands, remote_category.commands);
            }
            remote_category
        })
        .collect();
    categories.extend(
        local
            .iter()
            .filter(|category| !remote_ids.contains(category.id.as_str()))
            .cloned(),
    );
    categories
}

fn merge_commands(local: &[QuickCommand], remote: Vec<QuickCommand>) -> Vec<QuickCommand> {
    let remote_ids: HashSet<String> = remote.iter().map(|command| command.id.clone()).collect();
    let mut commands = remote;
    commands.extend(
        local
            .iter()
            .filter(|command| !remote_ids.contains(command.id.as_str()))
            .cloned(),
    );
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
    fn stable_remote_session_id_replaces_matching_legacy_local_session() {
        let local = session("", "Local", "local-password");
        let mut remote = session("remote-id", "Remote", "remote-password");
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
        assert_eq!(merged.sessions[0].id, "remote-id");
        assert_eq!(merged.sessions[0].name, "Remote");
        assert_eq!(merged.sessions[0].password, "local-password");
    }

    #[test]
    fn stable_remote_key_id_replaces_matching_legacy_local_key() {
        let local = key("", "Local key", "local-key-content");
        let remote = key("remote-key", "Remote key", "remote-key-content");
        let payload = SyncPayload::new(
            "device-1".into(),
            Vec::new(),
            Vec::new(),
            vec![remote],
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload(&[], &[], &[local], &[], payload, "");

        assert_eq!(merged.managed_keys.len(), 1);
        assert_eq!(merged.managed_keys[0].id, "remote-key");
        assert_eq!(merged.managed_keys[0].name, "Remote key");
        assert_eq!(merged.managed_keys[0].inline_content, "local-key-content");
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

    #[test]
    fn public_merge_preserves_local_tombstones_without_secret_access() {
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

        let merged = merge_public_payload_with_deleted(
            MergeLocal {
                sessions: &[],
                deleted_sessions: &[deleted],
                connection_groups: &[],
                deleted_connection_groups: &[],
                keys: &[],
                commands: &[],
            },
            payload,
        );

        assert!(merged.sessions.is_empty());
        assert_eq!(merged.connection_groups, vec!["prod"]);
        assert_eq!(merged.deleted_sessions.len(), 1);
    }

    #[test]
    fn local_group_tombstones_prevent_remote_group_contents_from_returning() {
        let mut remote = session("session-1", "Remote", "");
        remote.group = Some("prod/eu".into());
        let payload = SyncPayload::new(
            "device-1".into(),
            vec![remote],
            vec!["prod".into(), "prod/eu".into()],
            Vec::new(),
            Vec::new(),
            false,
            "",
        )
        .unwrap();
        let deleted_group = DeletedConnectionGroup {
            name: "prod".into(),
            groups: vec!["prod".into(), "prod/eu".into()],
            sessions: Vec::new(),
            deleted_at: 100,
        };

        let merged = merge_public_payload_with_deleted(
            MergeLocal {
                sessions: &[],
                deleted_sessions: &[],
                connection_groups: &[],
                deleted_connection_groups: &[deleted_group],
                keys: &[],
                commands: &[],
            },
            payload,
        );

        assert!(merged.sessions.is_empty());
        assert!(merged.connection_groups.is_empty());
        assert_eq!(merged.deleted_connection_groups.len(), 1);
    }

    #[test]
    fn repeated_merge_is_idempotent() {
        let local = session("local", "Local", "password");
        let remote = session("remote", "Remote", "password");
        let payload = SyncPayload::new(
            "device-1".into(),
            vec![remote],
            vec!["prod".into()],
            Vec::new(),
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let first = merge_payload(&[local], &["local".into()], &[], &[], payload.clone(), "");
        let second = merge_payload_with_deleted(
            MergeLocal {
                sessions: &first.sessions,
                deleted_sessions: &first.deleted_sessions,
                connection_groups: &first.connection_groups,
                deleted_connection_groups: &first.deleted_connection_groups,
                keys: &first.managed_keys,
                commands: &first.quick_command_categories,
            },
            payload,
            "",
        );

        assert_eq!(
            serde_json::to_string(&first.sessions).unwrap(),
            serde_json::to_string(&second.sessions).unwrap()
        );
        assert_eq!(first.connection_groups, second.connection_groups);
        assert_eq!(first.deleted_sessions.len(), second.deleted_sessions.len());
        assert_eq!(first.managed_keys.len(), second.managed_keys.len());
        assert_eq!(
            first.quick_command_categories.len(),
            second.quick_command_categories.len()
        );
    }

    #[test]
    fn duplicate_remote_tombstones_collapse_to_one_latest_entry() {
        let target = session("session-1", "Target", "password");
        let payload = SyncPayload::new_with_deleted(
            "device-1".into(),
            Vec::new(),
            vec![
                DeletedSession {
                    session: target.clone(),
                    deleted_at: 10,
                },
                DeletedSession {
                    session: Session {
                        name: "Latest".into(),
                        ..target
                    },
                    deleted_at: 20,
                },
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload_with_deleted(
            MergeLocal {
                sessions: &[],
                deleted_sessions: &[],
                connection_groups: &[],
                deleted_connection_groups: &[],
                keys: &[],
                commands: &[],
            },
            payload,
            "",
        );

        assert_eq!(merged.deleted_sessions.len(), 1);
        assert_eq!(merged.deleted_sessions[0].deleted_at, 20);
        assert_eq!(merged.deleted_sessions[0].session.name, "Latest");
    }

    #[test]
    fn large_session_catalog_merges_overlapping_ids_once() {
        let local: Vec<_> = (0..10_000)
            .map(|index| session(&format!("session-{index}"), "Local", ""))
            .collect();
        let remote: Vec<_> = (5_000..15_000)
            .map(|index| session(&format!("session-{index}"), "Remote", ""))
            .collect();
        let payload = SyncPayload::new(
            "device-1".into(),
            remote,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload(&local, &[], &[], &[], payload, "");
        let ids: HashSet<_> = merged.sessions.iter().map(|item| &item.id).collect();

        assert_eq!(merged.sessions.len(), 15_000);
        assert_eq!(ids.len(), merged.sessions.len());
        assert_eq!(
            merged
                .sessions
                .iter()
                .find(|item| item.id == "session-5000")
                .map(|item| item.name.as_str()),
            Some("Remote")
        );
    }
}
