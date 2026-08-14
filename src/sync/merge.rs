use std::collections::{HashMap, HashSet};

use sha2::Digest as _;

use crate::{
    session::config::{
        DeletedConnectionGroup, DeletedSession, ManagedKey, QuickCommand, QuickCommandCategory,
        Session,
    },
    sync::{
        model::{SyncDeletedConnectionGroup, SyncDeletedSession, SyncManagedKey, SyncSession},
        protocol::{SyncEntity, SyncTombstone, V3SyncPayload},
        secrets::{SecretResolutionStats, resolve_secret},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MergePreference {
    Local,
    Remote,
}

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
    pub base_payload: Option<V3SyncPayload>,
}

pub struct MergeLocal<'a> {
    pub sessions: &'a [Session],
    pub deleted_sessions: &'a [DeletedSession],
    pub connection_groups: &'a [String],
    pub deleted_connection_groups: &'a [DeletedConnectionGroup],
    pub keys: &'a [ManagedKey],
    pub commands: &'a [QuickCommandCategory],
    pub base_payload: Option<&'a V3SyncPayload>,
}

#[cfg(test)]
pub fn merge_payload(
    local_sessions: &[Session],
    local_connection_groups: &[String],
    local_keys: &[ManagedKey],
    local_commands: &[QuickCommandCategory],
    remote: crate::sync::model::SyncPayload,
    privacy_password: &str,
) -> MergedConfig {
    let remote = V3SyncPayload::from_legacy_for_tests(remote);
    merge_payload_with_secret_access(
        MergeLocal {
            sessions: local_sessions,
            deleted_sessions: &[],
            connection_groups: local_connection_groups,
            deleted_connection_groups: &[],
            keys: local_keys,
            commands: local_commands,
            base_payload: None,
        },
        remote,
        Some(privacy_password),
        MergePreference::Remote,
    )
}

pub fn merge_payload_with_deleted(
    local: MergeLocal<'_>,
    remote: V3SyncPayload,
    privacy_password: &str,
) -> MergedConfig {
    merge_payload_with_secret_access(
        local,
        remote,
        Some(privacy_password),
        MergePreference::Remote,
    )
}

pub fn merge_payload_for_upload_with_deleted(
    local: MergeLocal<'_>,
    remote: V3SyncPayload,
    privacy_password: &str,
) -> MergedConfig {
    merge_payload_with_secret_access(
        local,
        remote,
        Some(privacy_password),
        MergePreference::Local,
    )
}

pub fn merge_public_payload_with_deleted(
    local: MergeLocal<'_>,
    remote: V3SyncPayload,
) -> MergedConfig {
    merge_payload_with_secret_access(local, remote, None, MergePreference::Remote)
}

pub(crate) fn decode_payload(remote: V3SyncPayload, privacy_password: &str) -> MergedConfig {
    merge_payload_with_secret_access(
        MergeLocal {
            sessions: &[],
            deleted_sessions: &[],
            connection_groups: &[],
            deleted_connection_groups: &[],
            keys: &[],
            commands: &[],
            base_payload: None,
        },
        remote,
        Some(privacy_password),
        MergePreference::Remote,
    )
}

fn merge_payload_with_secret_access(
    local: MergeLocal<'_>,
    remote: V3SyncPayload,
    privacy_password: Option<&str>,
    preference: MergePreference,
) -> MergedConfig {
    let base_payload = remote.clone();
    let projected = project_v3_payload(remote);
    let sessions = filter_local_sessions(local.sessions, local.base_payload, &base_payload);
    let keys = filter_local_keys(local.keys, local.base_payload, &base_payload);
    let groups = filter_local_groups(local.connection_groups, local.base_payload, &base_payload);
    let commands = filter_local_commands(local.commands, local.base_payload, &base_payload);
    let local = MergeLocal {
        sessions: &sessions,
        deleted_sessions: local.deleted_sessions,
        connection_groups: &groups,
        deleted_connection_groups: local.deleted_connection_groups,
        keys: &keys,
        commands: &commands,
        base_payload: local.base_payload,
    };
    let mut merged = merge_projected_payload(local, projected, privacy_password, preference);
    merged.base_payload = Some(base_payload);
    merged
}

fn filter_local_sessions(
    local: &[Session],
    local_base: Option<&V3SyncPayload>,
    remote: &V3SyncPayload,
) -> Vec<Session> {
    local
        .iter()
        .filter(|session| !remote_deleted_since_base("session", &session.id, local_base, remote))
        .cloned()
        .collect()
}

fn filter_local_keys(
    local: &[ManagedKey],
    local_base: Option<&V3SyncPayload>,
    remote: &V3SyncPayload,
) -> Vec<ManagedKey> {
    local
        .iter()
        .filter(|key| !remote_deleted_since_base("managed-key", &key.id, local_base, remote))
        .cloned()
        .collect()
}

fn filter_local_groups(
    local: &[String],
    local_base: Option<&V3SyncPayload>,
    remote: &V3SyncPayload,
) -> Vec<String> {
    local
        .iter()
        .filter(|group| {
            let id = stable_group_id(group);
            !remote_deleted_since_base("connection-group", &id, local_base, remote)
        })
        .cloned()
        .collect()
}

fn filter_local_commands(
    local: &[QuickCommandCategory],
    local_base: Option<&V3SyncPayload>,
    remote: &V3SyncPayload,
) -> Vec<QuickCommandCategory> {
    local
        .iter()
        .filter_map(|category| {
            if remote_deleted_since_base("quick-command-category", &category.id, local_base, remote)
            {
                return None;
            }
            let commands = category
                .commands
                .iter()
                .filter(|command| {
                    !remote_deleted_since_base("quick-command", &command.id, local_base, remote)
                })
                .cloned()
                .collect();
            Some(QuickCommandCategory {
                id: category.id.clone(),
                name: category.name.clone(),
                commands,
            })
        })
        .collect()
}

fn remote_deleted_since_base(
    entity_type: &str,
    entity_id: &str,
    local_base: Option<&V3SyncPayload>,
    remote: &V3SyncPayload,
) -> bool {
    let Some(remote_tombstone) = latest_matching_tombstone(remote, entity_type, entity_id) else {
        return false;
    };
    let remote_active = active_version(remote, entity_type, entity_id);
    if remote_active.is_some_and(|version| version.compare(&remote_tombstone.version).is_gt()) {
        return false;
    }
    let base_version =
        local_base.and_then(|base| latest_entity_version(base, entity_type, entity_id));
    base_version.is_some_and(|version| remote_tombstone.version.compare(version).is_gt())
}

fn latest_matching_tombstone<'a>(
    payload: &'a V3SyncPayload,
    entity_type: &str,
    entity_id: &str,
) -> Option<&'a SyncTombstone> {
    payload
        .deleted_sessions
        .iter()
        .chain(payload.deleted_managed_keys.iter())
        .chain(payload.deleted_connection_groups.iter())
        .chain(payload.deleted_quick_command_categories.iter())
        .chain(payload.deleted_quick_commands.iter())
        .filter(|item| item.entity_type == entity_type && item.entity_id == entity_id)
        .max_by(|left, right| left.version.compare(&right.version))
}

fn active_version<'a>(
    payload: &'a V3SyncPayload,
    entity_type: &str,
    entity_id: &str,
) -> Option<&'a crate::sync::protocol::EntityVersion> {
    match entity_type {
        "session" => payload
            .sessions
            .iter()
            .find(|item| item.id == entity_id)
            .map(|item| &item.version),
        "managed-key" => payload
            .managed_keys
            .iter()
            .find(|item| item.id == entity_id)
            .map(|item| &item.version),
        "connection-group" => payload
            .connection_groups
            .iter()
            .find(|item| item.id == entity_id)
            .map(|item| &item.version),
        "quick-command-category" => payload
            .quick_command_categories
            .iter()
            .find(|item| item.id == entity_id)
            .map(|item| &item.version),
        "quick-command" => payload
            .quick_commands
            .iter()
            .find(|item| item.id == entity_id)
            .map(|item| &item.version),
        _ => None,
    }
}

fn latest_entity_version<'a>(
    payload: &'a V3SyncPayload,
    entity_type: &str,
    entity_id: &str,
) -> Option<&'a crate::sync::protocol::EntityVersion> {
    let active = active_version(payload, entity_type, entity_id);
    let tombstone =
        latest_matching_tombstone(payload, entity_type, entity_id).map(|item| &item.version);
    match (active, tombstone) {
        (Some(active), Some(tombstone)) if tombstone.compare(active).is_gt() => Some(tombstone),
        (Some(active), _) => Some(active),
        (None, tombstone) => tombstone,
    }
}

fn stable_group_id(group: &str) -> String {
    let mut digest = sha2::Sha256::new();
    digest.update(b"connection-group");
    digest.update([0]);
    digest.update(group.as_bytes());
    format!("connection-group:{}", hex::encode(digest.finalize()))
}

#[derive(Default)]
struct ProjectedPayload {
    sessions: Vec<SyncSession>,
    managed_keys: Vec<SyncManagedKey>,
    connection_groups: Vec<String>,
    commands: Vec<QuickCommandCategory>,
    deleted_sessions: Vec<SyncDeletedSession>,
    deleted_groups: Vec<SyncDeletedConnectionGroup>,
}

pub(crate) fn normalize_merged_config(config: &MergedConfig) -> MergedConfig {
    let mut normalized = merge_projected_payload(
        MergeLocal {
            sessions: &config.sessions,
            deleted_sessions: &config.deleted_sessions,
            connection_groups: &config.connection_groups,
            deleted_connection_groups: &config.deleted_connection_groups,
            keys: &config.managed_keys,
            commands: &config.quick_command_categories,
            base_payload: config.base_payload.as_ref(),
        },
        ProjectedPayload::default(),
        None,
        MergePreference::Local,
    );
    normalized.decrypted_count = config.decrypted_count;
    normalized.unavailable_secret_count = config.unavailable_secret_count;
    normalized.unavailable_session_secret_count = config.unavailable_session_secret_count;
    normalized.unavailable_managed_key_secret_count = config.unavailable_managed_key_secret_count;
    normalized.base_payload.clone_from(&config.base_payload);
    normalized
}

fn project_v3_payload(payload: V3SyncPayload) -> ProjectedPayload {
    let session_tombstones = latest_tombstones(payload.deleted_sessions, "session");
    let key_tombstones = latest_tombstones(payload.deleted_managed_keys, "managed-key");
    let group_tombstones = latest_tombstones(payload.deleted_connection_groups, "connection-group");
    let category_tombstones = latest_tombstones(
        payload.deleted_quick_command_categories,
        "quick-command-category",
    );
    let command_tombstones = latest_tombstones(payload.deleted_quick_commands, "quick-command");

    let sessions = payload
        .sessions
        .into_iter()
        .filter(|entity| is_newer_than_tombstone(entity, session_tombstones.get(&entity.id)))
        .map(|entity| entity.value)
        .collect();
    let managed_keys = payload
        .managed_keys
        .into_iter()
        .filter(|entity| is_newer_than_tombstone(entity, key_tombstones.get(&entity.id)))
        .map(|entity| entity.value)
        .collect();
    let connection_groups = payload
        .connection_groups
        .into_iter()
        .filter(|entity| is_newer_than_tombstone(entity, group_tombstones.get(&entity.id)))
        .map(|entity| entity.value)
        .collect();

    let mut categories = payload
        .quick_command_categories
        .into_iter()
        .filter(|entity| is_newer_than_tombstone(entity, category_tombstones.get(&entity.id)))
        .map(|entity| QuickCommandCategory {
            id: entity.id,
            name: entity.value.name,
            commands: Vec::new(),
        })
        .collect::<Vec<_>>();
    let category_index: HashMap<String, usize> = categories
        .iter()
        .enumerate()
        .map(|(index, category)| (category.id.clone(), index))
        .collect();
    for command in payload
        .quick_commands
        .into_iter()
        .filter(|entity| is_newer_than_tombstone(entity, command_tombstones.get(&entity.id)))
    {
        if let Some(category) = category_index
            .get(command.value.category_id.as_str())
            .and_then(|index| categories.get_mut(*index))
        {
            category.commands.push(QuickCommand {
                id: command.id,
                name: command.value.name,
                remark: command.value.remark,
                command: command.value.command,
            });
        }
    }

    let deleted_sessions = project_deleted_snapshots(
        payload.deleted_session_snapshots,
        session_tombstones,
        "session",
    );
    let deleted_groups =
        project_deleted_group_snapshots(payload.deleted_group_snapshots, group_tombstones);
    ProjectedPayload {
        sessions,
        managed_keys,
        connection_groups,
        commands: categories,
        deleted_sessions,
        deleted_groups,
    }
}

fn latest_tombstones(
    tombstones: Vec<SyncTombstone>,
    entity_type: &str,
) -> HashMap<String, SyncTombstone> {
    tombstones
        .into_iter()
        .filter(|tombstone| tombstone.entity_type == entity_type)
        .fold(HashMap::new(), |mut latest, tombstone| {
            let replace = latest
                .get(&tombstone.entity_id)
                .is_none_or(|current| tombstone.version.compare(&current.version).is_gt());
            if replace {
                latest.insert(tombstone.entity_id.clone(), tombstone);
            }
            latest
        })
}

pub(crate) fn is_newer_than_tombstone<T>(
    entity: &SyncEntity<T>,
    tombstone: Option<&SyncTombstone>,
) -> bool {
    tombstone.is_none_or(|tombstone| entity.version.compare(&tombstone.version).is_gt())
}

fn project_deleted_snapshots(
    snapshots: Vec<SyncEntity<SyncDeletedSession>>,
    tombstones: HashMap<String, SyncTombstone>,
    entity_type: &str,
) -> Vec<SyncDeletedSession> {
    snapshots
        .into_iter()
        .filter_map(|snapshot| {
            tombstones
                .get(&snapshot.id)
                .filter(|tombstone| tombstone.entity_type == entity_type)
                .map(|tombstone| SyncDeletedSession {
                    session: snapshot.value.session,
                    deleted_at: tombstone.deleted_at,
                })
        })
        .collect()
}

fn project_deleted_group_snapshots(
    snapshots: Vec<SyncEntity<SyncDeletedConnectionGroup>>,
    tombstones: HashMap<String, SyncTombstone>,
) -> Vec<SyncDeletedConnectionGroup> {
    snapshots
        .into_iter()
        .filter_map(|snapshot| {
            tombstones
                .get(&snapshot.id)
                .map(|tombstone| SyncDeletedConnectionGroup {
                    name: snapshot.value.name,
                    groups: snapshot.value.groups,
                    sessions: snapshot.value.sessions,
                    deleted_at: tombstone.deleted_at,
                })
        })
        .collect()
}

fn merge_projected_payload(
    local: MergeLocal<'_>,
    remote: ProjectedPayload,
    privacy_password: Option<&str>,
    preference: MergePreference,
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
    let remote_deleted_groups = remote.deleted_groups;
    let merged_keys = merge_keys(
        local.keys,
        remote.managed_keys,
        privacy_password,
        preference,
        &mut managed_key_stats,
    );
    let valid_key_ids: HashSet<&str> = merged_keys.keys.iter().map(|key| key.id.as_str()).collect();
    let sessions = merge_sessions(
        local.sessions,
        remote.sessions,
        privacy_password,
        preference,
        &merged_keys.aliases,
        &valid_key_ids,
        &mut session_stats,
    );
    let mut deleted_sessions = merge_deleted_sessions(
        local.deleted_sessions,
        remote_deleted_sessions,
        local.sessions,
        privacy_password,
        &merged_keys.aliases,
        &valid_key_ids,
        &mut session_stats,
    );
    let mut deleted_connection_groups = merge_deleted_groups(
        local.deleted_connection_groups,
        remote_deleted_groups,
        local.deleted_sessions,
        privacy_password,
        &merged_keys.aliases,
        &valid_key_ids,
        &mut session_stats,
    );
    normalize_deleted_key_references(
        &mut deleted_sessions,
        &mut deleted_connection_groups,
        &merged_keys.aliases,
        &valid_key_ids,
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
        managed_keys: merged_keys.keys,
        quick_command_categories: merge_command_categories(local.commands, remote.commands),
        decrypted_count: session_stats.decrypted_count + managed_key_stats.decrypted_count,
        unavailable_secret_count: session_stats.unavailable_count
            + managed_key_stats.unavailable_count,
        unavailable_session_secret_count: session_stats.unavailable_count,
        unavailable_managed_key_secret_count: managed_key_stats.unavailable_count,
        base_payload: None,
    }
}

fn merge_deleted_sessions(
    local_deleted: &[DeletedSession],
    remote_deleted: Vec<SyncDeletedSession>,
    local_sessions: &[Session],
    password: Option<&str>,
    key_aliases: &HashMap<String, String>,
    valid_key_ids: &HashSet<&str>,
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
        let session = session_from_remote(
            remote.session,
            local,
            password,
            MergePreference::Remote,
            key_aliases,
            valid_key_ids,
            stats,
        );
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
    key_aliases: &HashMap<String, String>,
    valid_key_ids: &HashSet<&str>,
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
                session_from_remote(
                    session,
                    local,
                    password,
                    MergePreference::Remote,
                    key_aliases,
                    valid_key_ids,
                    stats,
                )
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

fn normalize_deleted_key_references(
    deleted_sessions: &mut [DeletedSession],
    deleted_groups: &mut [DeletedConnectionGroup],
    aliases: &HashMap<String, String>,
    valid_key_ids: &HashSet<&str>,
) {
    for deleted in deleted_sessions {
        deleted.session.managed_key_id = select_key_reference(
            deleted.session.managed_key_id.as_ref(),
            None,
            aliases,
            valid_key_ids,
        );
    }
    for group in deleted_groups {
        for session in &mut group.sessions {
            session.managed_key_id = select_key_reference(
                session.managed_key_id.as_ref(),
                None,
                aliases,
                valid_key_ids,
            );
        }
    }
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
    preference: MergePreference,
    key_aliases: &HashMap<String, String>,
    valid_key_ids: &HashSet<&str>,
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
        sessions.push(session_from_remote(
            remote,
            local,
            password,
            preference,
            key_aliases,
            valid_key_ids,
            stats,
        ));
    }

    sessions.extend(
        local_sessions
            .iter()
            .enumerate()
            .filter(|(position, _)| !consumed_local.contains(position))
            .map(|(_, local)| {
                let mut session = local.clone();
                session.managed_key_id = select_key_reference(
                    local.managed_key_id.as_ref(),
                    None,
                    key_aliases,
                    valid_key_ids,
                );
                session
            }),
    );
    sessions
}

fn session_from_remote(
    remote: SyncSession,
    local: Option<&Session>,
    password: Option<&str>,
    preference: MergePreference,
    key_aliases: &HashMap<String, String>,
    valid_key_ids: &HashSet<&str>,
    stats: &mut SecretResolutionStats,
) -> Session {
    let id = if remote.id.is_empty() {
        local.map_or_else(String::new, |value| value.id.clone())
    } else {
        remote.id.clone()
    };
    if preference == MergePreference::Local
        && let Some(local) = local
    {
        let mut session = local.clone();
        session.id = id;
        session.password =
            resolve_local_preferred_secret(remote.password, &local.password, password, stats);
        session.private_key_inline = resolve_local_preferred_secret(
            remote.private_key_inline,
            &local.private_key_inline,
            password,
            stats,
        );
        session.passphrase =
            resolve_local_preferred_secret(remote.passphrase, &local.passphrase, password, stats);
        session.proxy_password = resolve_local_preferred_secret(
            remote.proxy_password,
            &local.proxy_password,
            password,
            stats,
        );
        session.managed_key_id = select_key_reference(
            local.managed_key_id.as_ref(),
            remote.managed_key_id.as_ref(),
            key_aliases,
            valid_key_ids,
        );
        return session;
    }

    let managed_key_id = select_key_reference(
        remote.managed_key_id.as_ref(),
        local.and_then(|value| value.managed_key_id.as_ref()),
        key_aliases,
        valid_key_ids,
    );
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
        managed_key_id,
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

fn resolve_local_preferred_secret(
    remote: crate::sync::model::SyncSecret,
    local: &str,
    password: Option<&str>,
    stats: &mut SecretResolutionStats,
) -> String {
    if local.is_empty() {
        resolve_secret(remote, local, password, stats)
    } else {
        local.to_string()
    }
}

fn select_key_reference(
    preferred: Option<&String>,
    fallback: Option<&String>,
    aliases: &HashMap<String, String>,
    valid_key_ids: &HashSet<&str>,
) -> Option<String> {
    preferred
        .and_then(|id| canonical_key_id(id, aliases, valid_key_ids))
        .or_else(|| fallback.and_then(|id| canonical_key_id(id, aliases, valid_key_ids)))
}

fn canonical_key_id(
    id: &str,
    aliases: &HashMap<String, String>,
    valid_key_ids: &HashSet<&str>,
) -> Option<String> {
    let canonical = aliases.get(id).map_or(id, String::as_str);
    valid_key_ids
        .contains(canonical)
        .then(|| canonical.to_string())
}

struct ManagedKeyIndex<'a> {
    by_id: HashMap<&'a str, usize>,
    by_fingerprint: HashMap<&'a str, usize>,
}

impl<'a> ManagedKeyIndex<'a> {
    fn new(keys: &'a [ManagedKey]) -> Self {
        let mut index = Self {
            by_id: HashMap::with_capacity(keys.len()),
            by_fingerprint: HashMap::with_capacity(keys.len()),
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
            }
        }
        index
    }

    fn find(&self, remote: &SyncManagedKey) -> Option<usize> {
        if !remote.id.is_empty()
            && let Some(position) = self.by_id.get(remote.id.as_str())
        {
            return Some(*position);
        }
        (!remote.fingerprint.is_empty())
            .then(|| {
                self.by_fingerprint
                    .get(remote.fingerprint.as_str())
                    .copied()
            })
            .flatten()
    }
}

struct MergedKeys {
    keys: Vec<ManagedKey>,
    aliases: HashMap<String, String>,
}

fn merge_keys(
    local_keys: &[ManagedKey],
    remote_keys: Vec<SyncManagedKey>,
    password: Option<&str>,
    preference: MergePreference,
    stats: &mut SecretResolutionStats,
) -> MergedKeys {
    let index = ManagedKeyIndex::new(local_keys);
    let mut consumed_local = HashSet::with_capacity(local_keys.len());
    let mut keys: Vec<ManagedKey> = Vec::with_capacity(local_keys.len() + remote_keys.len());
    let mut aliases = HashMap::with_capacity(local_keys.len() + remote_keys.len());
    let mut merged_by_id: HashMap<String, usize> = HashMap::new();
    let mut merged_by_fingerprint: HashMap<String, usize> = HashMap::new();

    for remote in remote_keys {
        let local_index = index.find(&remote);
        let local = local_index.map(|position| &local_keys[position]);
        if let Some(position) = local_index {
            consumed_local.insert(position);
        }

        let existing = (!remote.id.is_empty())
            .then(|| merged_by_id.get(&remote.id).copied())
            .flatten()
            .or_else(|| {
                (!remote.fingerprint.is_empty())
                    .then(|| merged_by_fingerprint.get(&remote.fingerprint).copied())
                    .flatten()
            });
        if let Some(existing) = existing {
            let canonical_id = keys[existing].id.clone();
            register_key_alias(&mut aliases, &remote.id, &canonical_id);
            if let Some(local) = local {
                register_key_alias(&mut aliases, &local.id, &canonical_id);
            }
            continue;
        }

        let canonical_id = if remote.id.is_empty() {
            local.map_or_else(String::new, |key| key.id.clone())
        } else {
            remote.id.clone()
        };
        register_key_alias(&mut aliases, &remote.id, &canonical_id);
        if let Some(local) = local {
            register_key_alias(&mut aliases, &local.id, &canonical_id);
        }
        let key = key_from_remote(remote, local, password, preference, canonical_id, stats);
        let position = keys.len();
        if !key.id.is_empty() {
            merged_by_id.insert(key.id.clone(), position);
        }
        if !key.fingerprint.is_empty() {
            merged_by_fingerprint.insert(key.fingerprint.clone(), position);
        }
        keys.push(key);
    }

    for (position, local) in local_keys.iter().enumerate() {
        if consumed_local.contains(&position) {
            continue;
        }
        let existing = (!local.id.is_empty())
            .then(|| merged_by_id.get(&local.id).copied())
            .flatten()
            .or_else(|| {
                (!local.fingerprint.is_empty())
                    .then(|| merged_by_fingerprint.get(&local.fingerprint).copied())
                    .flatten()
            });
        if let Some(existing) = existing {
            register_key_alias(&mut aliases, &local.id, &keys[existing].id);
            continue;
        }

        register_key_alias(&mut aliases, &local.id, &local.id);
        let merged_position = keys.len();
        if !local.id.is_empty() {
            merged_by_id.insert(local.id.clone(), merged_position);
        }
        if !local.fingerprint.is_empty() {
            merged_by_fingerprint.insert(local.fingerprint.clone(), merged_position);
        }
        keys.push(local.clone());
    }
    MergedKeys { keys, aliases }
}

fn register_key_alias(aliases: &mut HashMap<String, String>, id: &str, canonical_id: &str) {
    if !id.is_empty() && !canonical_id.is_empty() {
        aliases.insert(id.to_string(), canonical_id.to_string());
    }
}

fn key_from_remote(
    remote: SyncManagedKey,
    local: Option<&ManagedKey>,
    password: Option<&str>,
    preference: MergePreference,
    canonical_id: String,
    stats: &mut SecretResolutionStats,
) -> ManagedKey {
    if preference == MergePreference::Local
        && let Some(local) = local
    {
        let mut key = local.clone();
        key.id = canonical_id;
        key.inline_content = resolve_local_preferred_secret(
            remote.inline_content,
            &local.inline_content,
            password,
            stats,
        );
        key.passphrase =
            resolve_local_preferred_secret(remote.passphrase, &local.passphrase, password, stats);
        return key;
    }

    ManagedKey {
        id: canonical_id,
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
    use crate::{
        session::config::AuthMethod,
        sync::model::{SyncPayload, SyncPayloadInput},
    };

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

        let merged = merge_public_payload_with_deleted(
            MergeLocal {
                sessions: &[local_session],
                deleted_sessions: &[],
                connection_groups: &[],
                deleted_connection_groups: &[],
                keys: &[],
                commands: &[],
                base_payload: None,
            },
            payload.into(),
        );
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
    fn upload_merge_keeps_local_session_and_remaps_key_reference() {
        let mut local_session = session("session-1", "Local session", "local-password");
        local_session.auth = AuthMethod::Key;
        local_session.managed_key_id = Some("local-key".into());
        let local_key = key("local-key", "Local key", "local-key-content");

        let mut remote_session = session("session-1", "Remote session", "remote-password");
        remote_session.auth = AuthMethod::Key;
        remote_session.managed_key_id = Some("remote-key".into());
        let payload = SyncPayload::new(
            "device-2".into(),
            vec![remote_session],
            Vec::new(),
            vec![key("remote-key", "Remote key", "remote-key-content")],
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload_for_upload_with_deleted(
            MergeLocal {
                sessions: &[local_session],
                deleted_sessions: &[],
                connection_groups: &[],
                deleted_connection_groups: &[],
                keys: &[local_key],
                commands: &[],
                base_payload: None,
            },
            payload.into(),
            "",
        );

        assert_eq!(merged.managed_keys.len(), 1);
        assert_eq!(merged.managed_keys[0].id, "remote-key");
        assert_eq!(merged.managed_keys[0].name, "Local key");
        assert_eq!(merged.managed_keys[0].inline_content, "local-key-content");
        assert_eq!(merged.sessions.len(), 1);
        assert_eq!(merged.sessions[0].name, "Local session");
        assert_eq!(
            merged.sessions[0].managed_key_id.as_deref(),
            Some("remote-key")
        );
    }

    #[test]
    fn download_merge_prefers_remote_session_and_key_fields() {
        let mut local_session = session("session-1", "Local session", "local-password");
        local_session.auth = AuthMethod::Key;
        local_session.managed_key_id = Some("local-key".into());
        let local_key = key("local-key", "Local key", "local-key-content");

        let mut remote_session = session("session-1", "Remote session", "remote-password");
        remote_session.auth = AuthMethod::Key;
        remote_session.managed_key_id = Some("remote-key".into());
        let payload = SyncPayload::new(
            "device-2".into(),
            vec![remote_session],
            Vec::new(),
            vec![key("remote-key", "Remote key", "remote-key-content")],
            Vec::new(),
            true,
            "privacy-password",
        )
        .unwrap();

        let merged = merge_payload(
            &[local_session],
            &[],
            &[local_key],
            &[],
            payload,
            "privacy-password",
        );

        assert_eq!(merged.managed_keys.len(), 1);
        assert_eq!(merged.managed_keys[0].id, "remote-key");
        assert_eq!(merged.managed_keys[0].name, "Remote key");
        assert_eq!(merged.managed_keys[0].inline_content, "remote-key-content");
        assert_eq!(merged.sessions[0].name, "Remote session");
        assert_eq!(merged.sessions[0].password, "remote-password");
        assert_eq!(
            merged.sessions[0].managed_key_id.as_deref(),
            Some("remote-key")
        );
    }

    #[test]
    fn empty_fingerprints_do_not_collapse_distinct_keys() {
        let mut local_key = key("local-key", "Local key", "local-key-content");
        local_key.fingerprint.clear();
        let mut remote_key = key("remote-key", "Remote key", "remote-key-content");
        remote_key.fingerprint.clear();
        let payload = SyncPayload::new(
            "device-2".into(),
            Vec::new(),
            Vec::new(),
            vec![remote_key],
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload(&[], &[], &[local_key], &[], payload, "");

        assert_eq!(merged.managed_keys.len(), 2);
        assert_eq!(merged.managed_keys[0].id, "remote-key");
        assert_eq!(merged.managed_keys[1].id, "local-key");
    }

    #[test]
    fn duplicate_remote_fingerprints_collapse_and_rewrite_all_references() {
        let mut first_session = session("session-1", "First", "");
        first_session.auth = AuthMethod::Key;
        first_session.managed_key_id = Some("remote-key-1".into());
        let mut second_session = session("session-2", "Second", "");
        second_session.auth = AuthMethod::Key;
        second_session.managed_key_id = Some("remote-key-2".into());
        let payload = SyncPayload::new(
            "device-2".into(),
            vec![first_session, second_session],
            Vec::new(),
            vec![
                key("remote-key-1", "First key", "first-content"),
                key("remote-key-2", "Duplicate key", "duplicate-content"),
            ],
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload(&[], &[], &[], &[], payload, "");

        assert_eq!(merged.managed_keys.len(), 1);
        assert_eq!(merged.managed_keys[0].id, "remote-key-1");
        assert!(
            merged
                .sessions
                .iter()
                .all(|session| session.managed_key_id.as_deref() == Some("remote-key-1"))
        );
    }

    #[test]
    fn duplicate_local_fingerprints_collapse_and_rewrite_all_references() {
        let mut first_session = session("session-1", "First", "");
        first_session.auth = AuthMethod::Key;
        first_session.managed_key_id = Some("local-key-1".into());
        let mut second_session = session("session-2", "Second", "");
        second_session.auth = AuthMethod::Key;
        second_session.managed_key_id = Some("local-key-2".into());
        let local_keys = [
            key("local-key-1", "First key", "first-content"),
            key("local-key-2", "Duplicate key", "duplicate-content"),
        ];
        let payload = SyncPayload::new(
            "device-2".into(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload(
            &[first_session, second_session],
            &[],
            &local_keys,
            &[],
            payload,
            "",
        );

        assert_eq!(merged.managed_keys.len(), 1);
        assert_eq!(merged.managed_keys[0].id, "local-key-1");
        assert!(
            merged
                .sessions
                .iter()
                .all(|session| session.managed_key_id.as_deref() == Some("local-key-1"))
        );
    }

    #[test]
    fn deleted_session_key_references_are_remapped_to_canonical_id() {
        let mut deleted_session = session("session-1", "Deleted", "local-password");
        deleted_session.auth = AuthMethod::Key;
        deleted_session.managed_key_id = Some("local-key".into());
        let deleted = DeletedSession {
            session: deleted_session,
            deleted_at: 10,
        };
        let payload = SyncPayload::new(
            "device-2".into(),
            Vec::new(),
            Vec::new(),
            vec![key("remote-key", "Remote key", "remote-key-content")],
            Vec::new(),
            false,
            "",
        )
        .unwrap();

        let merged = merge_payload_for_upload_with_deleted(
            MergeLocal {
                sessions: &[],
                deleted_sessions: &[deleted],
                connection_groups: &[],
                deleted_connection_groups: &[],
                keys: &[key("local-key", "Local key", "local-key-content")],
                commands: &[],
                base_payload: None,
            },
            payload.into(),
            "",
        );

        assert_eq!(merged.deleted_sessions.len(), 1);
        assert_eq!(
            merged.deleted_sessions[0].session.managed_key_id.as_deref(),
            Some("remote-key")
        );
    }

    #[test]
    fn newer_remote_tombstones_remove_active_objects_and_survive_merge() {
        let local_session = session("session-1", "Local", "local-password");
        let mut deleted_group_session = session("session-2", "Grouped", "group-password");
        deleted_group_session.group = Some("prod/eu".into());
        let payload = SyncPayload::new_with_deleted(SyncPayloadInput {
            device_id: "device-1".into(),
            sessions: vec![],
            deleted_sessions: vec![DeletedSession {
                session: local_session.clone(),
                deleted_at: 20,
            }],
            connection_groups: vec!["prod".into(), "prod/eu".into()],
            deleted_connection_groups: vec![DeletedConnectionGroup {
                name: "prod".into(),
                groups: vec!["prod".into(), "prod/eu".into()],
                sessions: vec![deleted_group_session],
                deleted_at: 30,
            }],
            managed_keys: vec![],
            quick_command_categories: vec![],
            include_secrets: false,
            privacy_password: "".into(),
        })
        .unwrap();

        let merged = merge_payload_with_deleted(
            MergeLocal {
                sessions: &[local_session],
                deleted_sessions: &[],
                connection_groups: &["prod".into(), "prod/eu".into()],
                deleted_connection_groups: &[],
                keys: &[],
                commands: &[],
                base_payload: None,
            },
            payload.into(),
            "",
        );
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

        let _merged = merge_payload_with_deleted(
            MergeLocal {
                sessions: &[],
                deleted_sessions: &[deleted],
                connection_groups: &["prod".into()],
                deleted_connection_groups: &[],
                keys: &[],
                commands: &[],
                base_payload: None,
            },
            payload.into(),
            "",
        );
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
                base_payload: None,
            },
            payload.into(),
        );
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
                base_payload: None,
            },
            payload.into(),
        );
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
                base_payload: first.base_payload.as_ref(),
            },
            payload.into(),
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
        let payload = SyncPayload::new_with_deleted(SyncPayloadInput {
            device_id: "device-1".into(),
            sessions: Vec::new(),
            deleted_sessions: vec![
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
            connection_groups: Vec::new(),
            deleted_connection_groups: Vec::new(),
            managed_keys: Vec::new(),
            quick_command_categories: Vec::new(),
            include_secrets: false,
            privacy_password: "".into(),
        })
        .unwrap();

        let merged = merge_payload_with_deleted(
            MergeLocal {
                sessions: &[],
                deleted_sessions: &[],
                connection_groups: &[],
                deleted_connection_groups: &[],
                keys: &[],
                commands: &[],
                base_payload: None,
            },
            payload.into(),
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
