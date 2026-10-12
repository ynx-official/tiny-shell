mod v3;

pub(crate) use v3::stable_entity_id;
#[cfg(test)]
pub use v3::{CURRENT_FORMAT_VERSION, V3_FORMAT_VERSION};
pub use v3::{
    EntityVersion, SyncEntity, SyncTombstone, V3SyncPayload, parse_payload, serialize_payload,
};
