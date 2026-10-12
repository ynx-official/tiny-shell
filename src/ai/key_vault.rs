use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use tiny_shell_mcp::storage::{atomic_bytes, private_bytes};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema_version: u32,
    ciphertext: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredKey {
    pub provider_id: String,
    pub base_url: String,
    pub key: String,
}

impl StoredKey {
    pub fn new(id: &str, base_url: &str, key: &str) -> Self {
        Self {
            provider_id: id.to_owned(),
            base_url: base_url.to_owned(),
            key: key.to_owned(),
        }
    }
    pub fn resolve(&self, id: &str, base_url: &str) -> Result<String> {
        ensure!(
            self.provider_id == id && self.base_url == base_url,
            "AI_ENDPOINT_KEY_REQUIRED"
        );
        Ok(self.key.clone())
    }
}

/// Only the remembered cloud privacy password unlocks these keys. The history
/// master key remains in the native credential store and cannot open this vault.
pub(super) struct KeyVault {
    path: PathBuf,
    ciphertext: Option<String>,
    password: Option<String>,
    values: BTreeMap<String, StoredKey>,
}

impl KeyVault {
    pub fn open(path: PathBuf) -> Result<Self> {
        let ciphertext = if path.exists() {
            let saved: Envelope = serde_json::from_slice(&private_bytes(&path, 1024 * 1024)?)
                .map_err(|_| anyhow!("AI_KEY_VAULT_CORRUPT"))?;
            ensure!(
                saved.schema_version == 1 && !saved.ciphertext.is_empty(),
                "AI_KEY_VAULT_CORRUPT"
            );
            Some(saved.ciphertext)
        } else {
            None
        };
        Ok(Self {
            path,
            ciphertext,
            password: None,
            values: BTreeMap::new(),
        })
    }

    pub fn unlock(&mut self, password: &str) -> Result<()> {
        ensure!(password.chars().count() >= 8, "AI_SYNC_PASSWORD_REQUIRED");
        if self.password.as_deref() == Some(password) {
            return Ok(());
        }
        let values: BTreeMap<String, StoredKey> = if let Some(ciphertext) = &self.ciphertext {
            let plaintext = crate::crypto::decrypt_field(ciphertext, password)
                .map_err(|_| anyhow!("AI_KEY_LOCKED"))?;
            serde_json::from_str(&plaintext).map_err(|_| anyhow!("AI_KEY_VAULT_CORRUPT"))?
        } else {
            BTreeMap::new()
        };
        ensure!(
            values.iter().all(|(id, record)| &record.provider_id == id
                && !record.key.is_empty()
                && record.key.len() <= 16384),
            "AI_KEY_VAULT_CORRUPT"
        );
        self.values = values;
        self.password = Some(password.to_owned());
        Ok(())
    }

    pub fn unlocked(&self) -> bool {
        self.password.is_some()
    }
    pub fn uses_password(&self, password: &str) -> bool {
        self.password.as_deref() == Some(password)
    }

    pub fn values(&self) -> Result<&BTreeMap<String, StoredKey>> {
        ensure!(self.unlocked(), "AI_KEY_LOCKED");
        Ok(&self.values)
    }

    pub fn replace(&mut self, values: BTreeMap<String, StoredKey>) -> Result<()> {
        let password = self
            .password
            .as_deref()
            .ok_or_else(|| anyhow!("AI_SYNC_PASSWORD_REQUIRED"))?;
        self.write(values, password.to_owned())
    }

    pub fn rotate(&mut self, password: &str) -> Result<()> {
        ensure!(self.unlocked(), "AI_KEY_LOCKED");
        ensure!(password.chars().count() >= 8, "AI_SYNC_PASSWORD_REQUIRED");
        self.write(self.values.clone(), password.to_owned())
    }

    fn write(&mut self, values: BTreeMap<String, StoredKey>, password: String) -> Result<()> {
        ensure!(
            values.iter().all(|(id, record)| &record.provider_id == id
                && !record.key.is_empty()
                && record.key.len() <= 16384),
            "AI_KEY_REQUIRED"
        );
        let ciphertext = crate::crypto::encrypt_field(&serde_json::to_string(&values)?, &password)?;
        let bytes = serde_json::to_vec(&Envelope {
            schema_version: 1,
            ciphertext: ciphertext.clone(),
        })?;
        ensure!(bytes.len() <= 1024 * 1024, "AI_STORAGE_ERROR");
        atomic_bytes(&self.path, &bytes).map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        self.ciphertext = Some(ciphertext);
        self.values = values;
        self.password = Some(password);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_is_required_after_reopening_and_failure_preserves_ciphertext() -> Result<()> {
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-keys-{}", uuid::Uuid::new_v4()));
        tiny_shell_mcp::storage::private_directory(&directory)?;
        let mut vault = KeyVault::open(directory.join("keys.enc"))?;
        assert!(
            vault
                .replace(BTreeMap::from([(
                    "provider".into(),
                    StoredKey::new(
                        "provider",
                        "https://fixture.example.test/v1",
                        "test-api-secret"
                    )
                )]))
                .is_err()
        );
        vault.unlock("sync-password")?;
        vault.replace(BTreeMap::from([(
            "provider".into(),
            StoredKey::new(
                "provider",
                "https://fixture.example.test/v1",
                "test-api-secret",
            ),
        )]))?;
        let bytes = std::fs::read(directory.join("keys.enc"))?;
        assert!(!String::from_utf8_lossy(&bytes).contains("test-api-secret"));
        let mut reopened = KeyVault::open(directory.join("keys.enc"))?;
        assert!(reopened.values().is_err());
        assert!(reopened.unlock("wrong-password").is_err());
        assert_eq!(std::fs::read(directory.join("keys.enc"))?, bytes);
        reopened.unlock("sync-password")?;
        assert_eq!(
            reopened
                .values()?
                .get("provider")
                .map(|record| record.key.as_str()),
            Some("test-api-secret")
        );
        reopened.rotate("new-sync-password")?;
        let mut rotated = KeyVault::open(directory.join("keys.enc"))?;
        assert!(rotated.unlock("sync-password").is_err());
        rotated.unlock("new-sync-password")?;
        assert!(rotated.values()? == reopened.values()?);
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn tampering_never_becomes_an_empty_vault() -> Result<()> {
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-keys-{}", uuid::Uuid::new_v4()));
        tiny_shell_mcp::storage::private_directory(&directory)?;
        std::fs::write(
            directory.join("keys.enc"),
            br#"{"schema_version":1,"ciphertext":"broken"}"#,
        )?;
        let mut vault = KeyVault::open(directory.join("keys.enc"))?;
        assert!(vault.unlock("sync-password").is_err());
        assert!(vault.replace(BTreeMap::new()).is_err());
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}
