use crate::{crypto, sync::model::SyncSecret};

#[derive(Debug, Default)]
pub struct SecretResolutionStats {
    pub decrypted_count: u32,
    pub unavailable_count: u32,
}

pub fn resolve_secret(
    remote: SyncSecret,
    local: &str,
    password: Option<&str>,
    stats: &mut SecretResolutionStats,
) -> String {
    let Some(password) = password else {
        return local.to_string();
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_only_resolution_preserves_local_sensitive_values() {
        for remote in [
            SyncSecret::Omitted,
            SyncSecret::Empty,
            SyncSecret::Encrypted("invalid-ciphertext".into()),
            SyncSecret::LegacyPlaintext("remote-plaintext".into()),
        ] {
            let mut stats = SecretResolutionStats::default();
            assert_eq!(
                resolve_secret(remote, "local-secret", None, &mut stats),
                "local-secret"
            );
            assert_eq!(stats.decrypted_count, 0);
            assert_eq!(stats.unavailable_count, 0);
        }
    }
}
