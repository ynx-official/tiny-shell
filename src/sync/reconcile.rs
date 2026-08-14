use std::collections::{HashMap, HashSet};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session::config::{QuickCommand, QuickCommandCategory, Session};

use super::{MergeLocal, MergedConfig, protocol::V3SyncPayload};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyncEntityKind {
    Session,
    ManagedKey,
    ConnectionGroup,
    QuickCommandCategory,
    QuickCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictResolution {
    Local,
    Remote,
}

#[derive(Clone, PartialEq)]
enum EntitySnapshot {
    Active(Value),
    Deleted(Value),
    Missing,
}

impl EntitySnapshot {
    fn same_content(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Active(left), Self::Active(right)) => left == right,
            (Self::Deleted(_), Self::Deleted(_))
            | (Self::Deleted(_), Self::Missing)
            | (Self::Missing, Self::Deleted(_))
            | (Self::Missing, Self::Missing) => true,
            _ => false,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct EntityKey {
    kind: SyncEntityKind,
    id: String,
}

#[derive(Clone)]
pub struct SyncConflict {
    key: EntityKey,
    label: String,
    local: EntitySnapshot,
    remote: EntitySnapshot,
}

impl std::fmt::Debug for SyncConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyncConflict")
            .field("kind", &self.key.kind)
            .field("id", &self.key.id)
            .field("label", &self.label)
            .field("local", &"<redacted>")
            .field("remote", &"<redacted>")
            .finish()
    }
}

impl SyncConflict {
    pub fn kind(&self) -> SyncEntityKind {
        self.key.kind
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn can_copy_local_session(&self) -> bool {
        self.key.kind == SyncEntityKind::Session && matches!(self.local, EntitySnapshot::Active(_))
    }
}

#[derive(Clone)]
pub struct ThreeWayMerge {
    pub merged: MergedConfig,
    pub conflicts: Vec<SyncConflict>,
    decisions: HashMap<EntityKey, EntitySnapshot>,
    order: Vec<EntityKey>,
    remote_payload: V3SyncPayload,
    extra_sessions: Vec<Session>,
    decrypted_count: u32,
    unavailable_secret_count: u32,
    unavailable_session_secret_count: u32,
    unavailable_managed_key_secret_count: u32,
}

impl ThreeWayMerge {
    pub fn resolve(&mut self, index: usize, resolution: ConflictResolution) -> Result<()> {
        let conflict = self
            .conflicts
            .get(index)
            .cloned()
            .context("sync conflict no longer exists")?;
        let selected = match resolution {
            ConflictResolution::Local => conflict.local,
            ConflictResolution::Remote => conflict.remote,
        };
        self.decisions.insert(conflict.key, selected);
        self.conflicts.remove(index);
        self.rebuild()
    }

    pub fn resolve_all(&mut self, resolution: ConflictResolution) -> Result<()> {
        for conflict in self.conflicts.drain(..) {
            let selected = match resolution {
                ConflictResolution::Local => conflict.local,
                ConflictResolution::Remote => conflict.remote,
            };
            self.decisions.insert(conflict.key, selected);
        }
        self.rebuild()
    }

    pub fn copy_local_session(&mut self, index: usize, name_suffix: &str) -> Result<()> {
        let conflict = self
            .conflicts
            .get(index)
            .cloned()
            .context("sync conflict no longer exists")?;
        if conflict.key.kind != SyncEntityKind::Session {
            anyhow::bail!("only connection conflicts can be copied");
        }
        let EntitySnapshot::Active(value) = conflict.local else {
            anyhow::bail!("the local connection was deleted and cannot be copied");
        };
        let mut session: Session =
            serde_json::from_value(value).context("decode local conflict connection")?;
        session.id = uuid::Uuid::new_v4().to_string();
        session.name = format!("{}{}", session.name, name_suffix);
        self.extra_sessions.push(session);
        self.decisions.insert(conflict.key, conflict.remote);
        self.conflicts.remove(index);
        self.rebuild()
    }

    fn rebuild(&mut self) -> Result<()> {
        self.merged = build_merged_config(
            &self.decisions,
            &self.order,
            &self.remote_payload,
            &self.extra_sessions,
            MergeStats {
                decrypted_count: self.decrypted_count,
                unavailable_secret_count: self.unavailable_secret_count,
                unavailable_session_secret_count: self.unavailable_session_secret_count,
                unavailable_managed_key_secret_count: self.unavailable_managed_key_secret_count,
            },
        )?;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct CategoryHeader {
    name: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct CommandRecord {
    category_id: String,
    command: QuickCommand,
}

#[derive(Clone, Copy)]
struct MergeStats {
    decrypted_count: u32,
    unavailable_secret_count: u32,
    unavailable_session_secret_count: u32,
    unavailable_managed_key_secret_count: u32,
}

struct EntityCatalog {
    states: HashMap<EntityKey, EntitySnapshot>,
    order: Vec<EntityKey>,
}

impl EntityCatalog {
    fn from_config(config: &MergedConfig) -> Result<Self> {
        let mut catalog = Self {
            states: HashMap::new(),
            order: Vec::new(),
        };
        for session in &config.sessions {
            catalog.insert(
                SyncEntityKind::Session,
                session.id.clone(),
                EntitySnapshot::Active(serde_json::to_value(session)?),
            );
        }
        for deleted in &config.deleted_sessions {
            catalog.insert(
                SyncEntityKind::Session,
                deleted.session.id.clone(),
                EntitySnapshot::Deleted(serde_json::to_value(deleted)?),
            );
        }
        for key in &config.managed_keys {
            catalog.insert(
                SyncEntityKind::ManagedKey,
                key.id.clone(),
                EntitySnapshot::Active(serde_json::to_value(key)?),
            );
        }
        for group in &config.connection_groups {
            catalog.insert(
                SyncEntityKind::ConnectionGroup,
                super::protocol::stable_entity_id("connection-group", group),
                EntitySnapshot::Active(serde_json::to_value(group)?),
            );
        }
        for deleted in &config.deleted_connection_groups {
            catalog.insert(
                SyncEntityKind::ConnectionGroup,
                super::protocol::stable_entity_id("connection-group", &deleted.name),
                EntitySnapshot::Deleted(serde_json::to_value(deleted)?),
            );
        }
        for category in &config.quick_command_categories {
            catalog.insert(
                SyncEntityKind::QuickCommandCategory,
                category.id.clone(),
                EntitySnapshot::Active(serde_json::to_value(CategoryHeader {
                    name: category.name.clone(),
                })?),
            );
            for command in &category.commands {
                catalog.insert(
                    SyncEntityKind::QuickCommand,
                    command.id.clone(),
                    EntitySnapshot::Active(serde_json::to_value(CommandRecord {
                        category_id: category.id.clone(),
                        command: command.clone(),
                    })?),
                );
            }
        }
        Ok(catalog)
    }

    fn insert(&mut self, kind: SyncEntityKind, id: String, snapshot: EntitySnapshot) {
        let key = EntityKey { kind, id };
        if !self.states.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.states.insert(key, snapshot);
    }

    fn get(&self, key: &EntityKey) -> EntitySnapshot {
        self.states
            .get(key)
            .cloned()
            .unwrap_or(EntitySnapshot::Missing)
    }
}

pub fn reconcile_three_way(
    local: MergeLocal<'_>,
    baseline: &V3SyncPayload,
    remote: V3SyncPayload,
    privacy_password: &str,
) -> Result<ThreeWayMerge> {
    let local = MergedConfig {
        sessions: local.sessions.to_vec(),
        deleted_sessions: local.deleted_sessions.to_vec(),
        deleted_connection_groups: local.deleted_connection_groups.to_vec(),
        connection_groups: local.connection_groups.to_vec(),
        managed_keys: local.keys.to_vec(),
        quick_command_categories: local.commands.to_vec(),
        decrypted_count: 0,
        unavailable_secret_count: 0,
        unavailable_session_secret_count: 0,
        unavailable_managed_key_secret_count: 0,
        base_payload: Some(baseline.clone()),
    };
    let baseline_config = super::merge::decode_payload(baseline.clone(), privacy_password);
    let remote_config = super::merge::decode_payload(remote.clone(), privacy_password);
    let stats = MergeStats {
        decrypted_count: remote_config.decrypted_count,
        unavailable_secret_count: remote_config.unavailable_secret_count,
        unavailable_session_secret_count: remote_config.unavailable_session_secret_count,
        unavailable_managed_key_secret_count: remote_config.unavailable_managed_key_secret_count,
    };
    let local = EntityCatalog::from_config(&local)?;
    let baseline = EntityCatalog::from_config(&baseline_config)?;
    let remote_catalog = EntityCatalog::from_config(&remote_config)?;
    let order = combined_order(&remote_catalog, &local, &baseline);
    let mut decisions = HashMap::with_capacity(order.len());
    let mut conflicts = Vec::new();

    for key in &order {
        let base = baseline.get(key);
        let local_value = local.get(key);
        let remote_value = remote_catalog.get(key);
        let selected = if local_value.same_content(&base) {
            remote_value.clone()
        } else if remote_value.same_content(&base) || local_value.same_content(&remote_value) {
            local_value.clone()
        } else {
            conflicts.push(SyncConflict {
                key: key.clone(),
                label: safe_label(key.kind, &local_value, &remote_value),
                local: local_value.clone(),
                remote: remote_value.clone(),
            });
            local_value
        };
        decisions.insert(key.clone(), selected);
    }

    let merged = build_merged_config(&decisions, &order, &remote, &[], stats)?;
    Ok(ThreeWayMerge {
        merged,
        conflicts,
        decisions,
        order,
        remote_payload: remote,
        extra_sessions: Vec::new(),
        decrypted_count: stats.decrypted_count,
        unavailable_secret_count: stats.unavailable_secret_count,
        unavailable_session_secret_count: stats.unavailable_session_secret_count,
        unavailable_managed_key_secret_count: stats.unavailable_managed_key_secret_count,
    })
}

fn combined_order(
    remote: &EntityCatalog,
    local: &EntityCatalog,
    baseline: &EntityCatalog,
) -> Vec<EntityKey> {
    let mut seen = HashSet::new();
    remote
        .order
        .iter()
        .chain(local.order.iter())
        .chain(baseline.order.iter())
        .filter(|key| seen.insert((*key).clone()))
        .cloned()
        .collect()
}

fn safe_label(kind: SyncEntityKind, local: &EntitySnapshot, remote: &EntitySnapshot) -> String {
    snapshot_label(kind, local)
        .or_else(|| snapshot_label(kind, remote))
        .unwrap_or_else(|| "-".to_string())
}

fn snapshot_label(kind: SyncEntityKind, snapshot: &EntitySnapshot) -> Option<String> {
    let value = match snapshot {
        EntitySnapshot::Active(value) | EntitySnapshot::Deleted(value) => value,
        EntitySnapshot::Missing => return None,
    };
    let label = match kind {
        SyncEntityKind::Session => value
            .get("name")
            .or_else(|| value.get("session").and_then(|session| session.get("name")))
            .and_then(Value::as_str),
        SyncEntityKind::ManagedKey | SyncEntityKind::QuickCommand => value
            .get("name")
            .or_else(|| value.get("command").and_then(|command| command.get("name")))
            .and_then(Value::as_str),
        SyncEntityKind::ConnectionGroup => value
            .as_str()
            .or_else(|| value.get("name").and_then(Value::as_str)),
        SyncEntityKind::QuickCommandCategory => value.get("name").and_then(Value::as_str),
    };
    label.map(str::to_string)
}

fn build_merged_config(
    decisions: &HashMap<EntityKey, EntitySnapshot>,
    order: &[EntityKey],
    remote_payload: &V3SyncPayload,
    extra_sessions: &[Session],
    stats: MergeStats,
) -> Result<MergedConfig> {
    let mut sessions = Vec::new();
    let mut deleted_sessions = Vec::new();
    let mut managed_keys = Vec::new();
    let mut connection_groups = Vec::new();
    let mut deleted_connection_groups = Vec::new();
    let mut category_order = Vec::new();
    let mut categories: HashMap<String, QuickCommandCategory> = HashMap::new();
    let mut commands = Vec::new();

    for key in order {
        let snapshot = decisions.get(key).unwrap_or(&EntitySnapshot::Missing);
        match (key.kind, snapshot) {
            (SyncEntityKind::Session, EntitySnapshot::Active(value)) => {
                sessions.push(serde_json::from_value(value.clone()).context("decode connection")?);
            }
            (SyncEntityKind::Session, EntitySnapshot::Deleted(value)) => deleted_sessions
                .push(serde_json::from_value(value.clone()).context("decode deleted connection")?),
            (SyncEntityKind::ManagedKey, EntitySnapshot::Active(value)) => managed_keys
                .push(serde_json::from_value(value.clone()).context("decode managed key")?),
            (SyncEntityKind::ConnectionGroup, EntitySnapshot::Active(value)) => {
                connection_groups.push(
                    serde_json::from_value(value.clone()).context("decode connection group")?,
                );
            }
            (SyncEntityKind::ConnectionGroup, EntitySnapshot::Deleted(value)) => {
                deleted_connection_groups.push(
                    serde_json::from_value(value.clone())
                        .context("decode deleted connection group")?,
                );
            }
            (SyncEntityKind::QuickCommandCategory, EntitySnapshot::Active(value)) => {
                let header: CategoryHeader = serde_json::from_value(value.clone())
                    .context("decode quick command category")?;
                category_order.push(key.id.clone());
                categories.insert(
                    key.id.clone(),
                    QuickCommandCategory {
                        id: key.id.clone(),
                        name: header.name,
                        commands: Vec::new(),
                    },
                );
            }
            (SyncEntityKind::QuickCommand, EntitySnapshot::Active(value)) => {
                commands.push(
                    serde_json::from_value::<CommandRecord>(value.clone())
                        .context("decode quick command")?,
                );
            }
            (_, EntitySnapshot::Missing | EntitySnapshot::Deleted(_)) => {}
        }
    }
    sessions.extend_from_slice(extra_sessions);
    for record in commands {
        let category = categories
            .get_mut(&record.category_id)
            .with_context(|| format!("quick command category {} is missing", record.category_id))?;
        category.commands.push(record.command);
    }
    let quick_command_categories = category_order
        .into_iter()
        .filter_map(|id| categories.remove(&id))
        .collect();

    let merged = MergedConfig {
        sessions,
        deleted_sessions,
        deleted_connection_groups,
        connection_groups,
        managed_keys,
        quick_command_categories,
        decrypted_count: stats.decrypted_count,
        unavailable_secret_count: stats.unavailable_secret_count,
        unavailable_session_secret_count: stats.unavailable_session_secret_count,
        unavailable_managed_key_secret_count: stats.unavailable_managed_key_secret_count,
        base_payload: Some(remote_payload.clone()),
    };
    Ok(super::merge::normalize_merged_config(&merged))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::config::{AuthMethod, DeletedSession, ManagedKey};

    fn session(id: &str, name: &str) -> Session {
        Session {
            id: id.into(),
            name: name.into(),
            host: format!("{id}.example.test"),
            port: 22,
            user: "alice".into(),
            auth: AuthMethod::Password,
            password: String::new(),
            private_key_path: String::new(),
            private_key_inline: String::new(),
            passphrase: String::new(),
            managed_key_id: None,
            last_used: None,
            group: None,
            proxy_type: "none".into(),
            proxy_host: String::new(),
            proxy_port: None,
            proxy_user: String::new(),
            proxy_password: String::new(),
        }
    }

    fn config(sessions: Vec<Session>, deleted_sessions: Vec<DeletedSession>) -> MergedConfig {
        MergedConfig {
            sessions,
            deleted_sessions,
            deleted_connection_groups: Vec::new(),
            connection_groups: Vec::new(),
            managed_keys: Vec::new(),
            quick_command_categories: Vec::new(),
            decrypted_count: 0,
            unavailable_secret_count: 0,
            unavailable_session_secret_count: 0,
            unavailable_managed_key_secret_count: 0,
            base_payload: None,
        }
    }

    fn payload(device: &str, config: &MergedConfig, base: Option<&V3SyncPayload>) -> V3SyncPayload {
        V3SyncPayload::from_config(device.into(), config, false, "", base).unwrap()
    }

    fn managed_key(id: &str) -> ManagedKey {
        ManagedKey {
            id: id.into(),
            name: id.into(),
            key_type: "ed25519".into(),
            fingerprint: "SHA256:shared".into(),
            inline_content: String::new(),
            passphrase: String::new(),
            created_at: 1,
        }
    }

    fn local<'a>(config: &'a MergedConfig, base: &'a V3SyncPayload) -> MergeLocal<'a> {
        MergeLocal {
            sessions: &config.sessions,
            deleted_sessions: &config.deleted_sessions,
            connection_groups: &config.connection_groups,
            deleted_connection_groups: &config.deleted_connection_groups,
            keys: &config.managed_keys,
            commands: &config.quick_command_categories,
            base_payload: Some(base),
        }
    }

    #[test]
    fn local_only_change_is_selected_without_conflict() {
        let base_config = config(vec![session("one", "Base")], vec![]);
        let baseline = payload("base-device", &base_config, None);
        let local_config = config(vec![session("one", "Local")], vec![]);
        let remote = payload("remote-device", &base_config, Some(&baseline));

        let result =
            reconcile_three_way(local(&local_config, &baseline), &baseline, remote, "").unwrap();

        assert!(result.conflicts.is_empty());
        assert_eq!(result.merged.sessions[0].name, "Local");
    }

    #[test]
    fn changes_to_different_entities_are_combined() {
        let base_config = config(vec![session("one", "One"), session("two", "Two")], vec![]);
        let baseline = payload("base-device", &base_config, None);
        let local_config = config(
            vec![session("one", "Local One"), session("two", "Two")],
            vec![],
        );
        let remote_config = config(
            vec![session("one", "One"), session("two", "Remote Two")],
            vec![],
        );
        let remote = payload("remote-device", &remote_config, Some(&baseline));

        let result =
            reconcile_three_way(local(&local_config, &baseline), &baseline, remote, "").unwrap();

        assert!(result.conflicts.is_empty());
        assert_eq!(result.merged.sessions[0].name, "Local One");
        assert_eq!(result.merged.sessions[1].name, "Remote Two");
    }

    #[test]
    fn different_changes_to_the_same_entity_create_a_conflict() {
        let base_config = config(vec![session("one", "Base")], vec![]);
        let baseline = payload("base-device", &base_config, None);
        let local_config = config(vec![session("one", "Local")], vec![]);
        let remote_config = config(vec![session("one", "Remote")], vec![]);
        let remote = payload("remote-device", &remote_config, Some(&baseline));

        let result =
            reconcile_three_way(local(&local_config, &baseline), &baseline, remote, "").unwrap();

        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].kind(), SyncEntityKind::Session);
        assert_eq!(result.conflicts[0].label(), "Local");
    }

    #[test]
    fn delete_and_remote_edit_create_a_conflict() {
        let original = session("one", "Base");
        let base_config = config(vec![original.clone()], vec![]);
        let baseline = payload("base-device", &base_config, None);
        let local_config = config(
            vec![],
            vec![DeletedSession {
                session: original,
                deleted_at: 10,
            }],
        );
        let remote_config = config(vec![session("one", "Remote")], vec![]);
        let remote = payload("remote-device", &remote_config, Some(&baseline));

        let result =
            reconcile_three_way(local(&local_config, &baseline), &baseline, remote, "").unwrap();

        assert_eq!(result.conflicts.len(), 1);
    }

    #[test]
    fn resolving_conflict_with_remote_rebuilds_the_merged_config() {
        let base_config = config(vec![session("one", "Base")], vec![]);
        let baseline = payload("base-device", &base_config, None);
        let local_config = config(vec![session("one", "Local")], vec![]);
        let remote_config = config(vec![session("one", "Remote")], vec![]);
        let remote = payload("remote-device", &remote_config, Some(&baseline));
        let mut result =
            reconcile_three_way(local(&local_config, &baseline), &baseline, remote, "").unwrap();

        result.resolve(0, ConflictResolution::Remote).unwrap();

        assert!(result.conflicts.is_empty());
        assert_eq!(result.merged.sessions[0].name, "Remote");
    }

    #[test]
    fn independent_deletions_of_the_same_entity_do_not_conflict() {
        let original = session("one", "Base");
        let base_config = config(vec![original.clone()], vec![]);
        let baseline = payload("base-device", &base_config, None);
        let local_config = config(
            vec![],
            vec![DeletedSession {
                session: original.clone(),
                deleted_at: 10,
            }],
        );
        let remote_config = config(
            vec![],
            vec![DeletedSession {
                session: original,
                deleted_at: 20,
            }],
        );
        let remote = payload("remote-device", &remote_config, Some(&baseline));

        let result =
            reconcile_three_way(local(&local_config, &baseline), &baseline, remote, "").unwrap();

        assert!(result.conflicts.is_empty());
        assert!(result.merged.sessions.is_empty());
        assert_eq!(result.merged.deleted_sessions.len(), 1);
    }

    #[test]
    fn copying_a_connection_keeps_remote_original_and_creates_local_duplicate() {
        let base_config = config(vec![session("one", "Base")], vec![]);
        let baseline = payload("base-device", &base_config, None);
        let local_config = config(vec![session("one", "Local")], vec![]);
        let remote_config = config(vec![session("one", "Remote")], vec![]);
        let remote = payload("remote-device", &remote_config, Some(&baseline));
        let mut result =
            reconcile_three_way(local(&local_config, &baseline), &baseline, remote, "").unwrap();

        result.copy_local_session(0, " Copy").unwrap();

        assert!(result.conflicts.is_empty());
        assert_eq!(result.merged.sessions.len(), 2);
        assert!(
            result
                .merged
                .sessions
                .iter()
                .any(|session| session.id == "one" && session.name == "Remote")
        );
        assert!(
            result
                .merged
                .sessions
                .iter()
                .any(|session| session.id != "one" && session.name == "Local Copy")
        );
    }

    #[test]
    fn independently_added_matching_keys_are_deduplicated_and_references_are_rewritten() {
        let base_config = config(vec![], vec![]);
        let baseline = payload("base-device", &base_config, None);
        let mut local_session = session("local-session", "Local");
        local_session.managed_key_id = Some("local-key".into());
        let mut local_config = config(vec![local_session], vec![]);
        local_config.managed_keys.push(managed_key("local-key"));
        let mut remote_session = session("remote-session", "Remote");
        remote_session.managed_key_id = Some("remote-key".into());
        let mut remote_config = config(vec![remote_session], vec![]);
        remote_config.managed_keys.push(managed_key("remote-key"));
        let remote = payload("remote-device", &remote_config, Some(&baseline));

        let result =
            reconcile_three_way(local(&local_config, &baseline), &baseline, remote, "").unwrap();

        assert!(result.conflicts.is_empty());
        assert_eq!(result.merged.managed_keys.len(), 1);
        let canonical_id = &result.merged.managed_keys[0].id;
        assert!(
            result
                .merged
                .sessions
                .iter()
                .all(|session| session.managed_key_id.as_ref() == Some(canonical_id))
        );
    }
}
