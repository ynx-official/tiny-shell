#![allow(dead_code)]

use serde::{Deserialize, Serialize};

use super::v2::{
    SyncDeletedConnectionGroup, SyncDeletedSession, SyncManagedKey, SyncSecret, SyncSession,
};

pub const V3_FORMAT_VERSION: u32 = 3;
pub const ENTITY_VERSION_FORMAT: u32 = 1;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
