use crate::{crypto, sync::model::SyncSecret};

#[derive(Debug, Default)]
pub struct SecretResolutionStats {
    pub decrypted_count: u32,
    pub unavailable_count: u32,
}

pub fn resolve_secret(
    remote: SyncSecret,
    local: &str,
    password: &str,
    stats: &mut SecretResolutionStats,
) -> String {
    match remote {
        SyncSecret::Omitted => local.to_string(),
        SyncSecret::Empty => String::new(),
        SyncSecret::LegacyPlaintext(value) => value,
        SyncSecret::Encrypted(value) => match crypto::decrypt_field(&value, password) {
            Ok(plaintext) => {
                stats.decrypted_count += 1;
                plaintext
            }
            Err(_) => {
                stats.unavailable_count += 1;
                local.to_string()
            }
        },
    }
}
