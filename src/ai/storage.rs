use super::{
    Conversation, FollowupState, Settings, TitleState, TurnState,
    key_vault::{KeyVault, StoredKey},
};
use anyhow::{Result, anyhow, ensure};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::PathBuf, sync::Mutex};
use tiny_shell_mcp::storage::{atomic_bytes, private_bytes, private_directory};

pub(super) fn seal(key: &[u8; 32], id: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut nonce);
    let cipher = XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: bytes,
                aad: id.as_bytes(),
            },
        )
        .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
    let mut sealed = b"TSAI1".to_vec();
    sealed.extend_from_slice(&nonce);
    sealed.extend(cipher);
    Ok(sealed)
}
pub(super) fn unseal(key: &[u8; 32], id: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() >= 45 && &bytes[..5] == b"TSAI1",
        "AI_HISTORY_CORRUPT"
    );
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(&bytes[5..29]),
            Payload {
                msg: &bytes[29..],
                aad: id.as_bytes(),
            },
        )
        .map_err(|_| anyhow!("AI_HISTORY_CORRUPT"))
}
pub(super) struct Store {
    root: PathBuf,
    service: String,
    master: Option<[u8; 32]>,
    volatile: Mutex<HashMap<String, StoredKey>>,
    vault: Mutex<KeyVault>,
}
type Opened = (Store, Settings, Vec<Conversation>, Option<String>);
fn recover_background_requests(c: &mut Conversation) {
    // Auxiliary requests can still be running after the chat has already become Idle.
    if c.title_state == TitleState::Generating {
        c.title_state = TitleState::Failed;
    }
    if let Some(followups) = &mut c.followups
        && followups.state == FollowupState::Generating
    {
        followups.state = FollowupState::Failed;
        followups.items.clear();
    }
    for request in &mut c.requests {
        if matches!(request.state.as_str(), "pending" | "running") {
            request.state = "interrupted".into();
            request.finished_ms = Some(tiny_shell_mcp::audit::now_ms());
        }
    }
}
impl Store {
    #[cfg(test)]
    pub fn fixture(
        directory: PathBuf,
        master: Option<[u8; 32]>,
        id: &str,
        base_url: &str,
        key: &str,
    ) -> Result<Self> {
        let root = directory.join("ai");
        private_directory(&root)?;
        private_directory(&root.join("conversations"))?;
        Ok(Self {
            vault: Mutex::new(KeyVault::open(root.join("keys.enc"))?),
            root,
            service: format!("tiny-shell-test-{}", uuid::Uuid::new_v4()),
            master,
            volatile: Mutex::new(HashMap::from([(
                id.to_owned(),
                StoredKey::new(id, base_url, key),
            )])),
        })
    }
    pub fn open(directory: PathBuf) -> Result<Opened> {
        let root = directory.join("ai");
        private_directory(&root)?;
        private_directory(&root.join("conversations"))?;
        let canonical = root.canonicalize()?;
        let service = format!(
            "tiny-shell.ai.{}",
            hex::encode(Sha256::digest(canonical.to_string_lossy().as_bytes()))
        );
        let settings: Settings = if root.join("providers.json").exists() {
            serde_json::from_slice(&private_bytes(&root.join("providers.json"), 1024 * 1024)?)
                .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
        } else {
            Settings::default()
        };
        settings.execution.validate()?;
        let has_history = std::fs::read_dir(root.join("conversations"))?
            .filter_map(Result::ok)
            .any(|e| e.path().extension().is_some_and(|x| x == "enc"));
        let master = Self::entry(&service, "history-key")
            .and_then(|entry| match entry.get_secret() {
                Ok(bytes) => bytes.try_into().map_err(|_| anyhow!("AI_HISTORY_LOCKED")),
                Err(keyring::Error::NoEntry) if !has_history => {
                    let mut key = [0; 32];
                    OsRng.fill_bytes(&mut key);
                    entry
                        .set_secret(&key)
                        .map_err(|_| anyhow!("AI_VAULT_UNAVAILABLE"))?;
                    Ok(key)
                }
                Err(_) => Err(anyhow!("AI_VAULT_UNAVAILABLE")),
            })
            .ok();
        let store = Self {
            vault: Mutex::new(KeyVault::open(root.join("keys.enc"))?),
            root,
            service,
            master,
            volatile: Mutex::new(HashMap::new()),
        };
        let mut warning = if has_history && store.master.is_none() {
            Some("AI_HISTORY_LOCKED".into())
        } else {
            None
        };
        let mut conversations = Vec::new();
        if let Some(key) = &store.master {
            for entry in std::fs::read_dir(store.root.join("conversations"))? {
                let path = entry?.path();
                if path.extension().is_none_or(|x| x != "enc") {
                    continue;
                }
                let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if uuid::Uuid::parse_str(id).is_err() {
                    continue;
                }
                let loaded = (|| {
                    let bytes = private_bytes(&path, 16 * 1024 * 1024)?;
                    let mut c: Conversation = serde_json::from_slice(&unseal(key, id, &bytes)?)?;
                    ensure!(c.id == id, "AI_HISTORY_CORRUPT");
                    if c.state.active() {
                        c.state = TurnState::Interrupted;
                        c.continuation = None;
                        c.error = Some("AI_INTERRUPTED".into());
                        if let Some(turn) = c.turns.last_mut() {
                            turn.state = "interrupted".into();
                            turn.finished_ms = Some(tiny_shell_mcp::audit::now_ms());
                        }
                        for activity in &mut c.activities {
                            if matches!(
                                activity.state.as_str(),
                                "running" | "queued" | "awaiting_user" | "cancel_requested"
                            ) {
                                activity.state = "interrupted".into();
                            }
                        }
                    }
                    recover_background_requests(&mut c);
                    Ok(c)
                })();
                match loaded {
                    Ok(c) => conversations.push(c),
                    Err(_) => warning = Some("AI_HISTORY_CORRUPT".into()),
                }
            }
        }
        conversations.sort_by_key(|c| std::cmp::Reverse(c.updated));
        Ok((store, settings, conversations, warning))
    }
    fn entry(service: &str, id: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(service, id).map_err(|_| anyhow!("AI_VAULT_UNAVAILABLE"))
    }
    pub fn temporary(&self) -> bool {
        self.master.is_none()
    }
    pub fn key_temporary(&self, id: &str) -> bool {
        self.volatile.lock().is_ok_and(|keys| keys.contains_key(id))
    }
    pub fn key_saved(&self, id: &str) -> bool {
        if self
            .vault
            .lock()
            .is_ok_and(|vault| vault.values().is_ok_and(|values| values.contains_key(id)))
        {
            return true;
        }
        false
    }
    pub fn key(&self, id: &str, base_url: &str) -> Result<String> {
        if let Some(key) = self
            .volatile
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
            .get(id)
        {
            return key.resolve(id, base_url);
        }
        let vault = self.vault.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        vault
            .values()?
            .get(id)
            .map_or_else(|| Ok(String::new()), |key| key.resolve(id, base_url))
    }
    pub fn set_key(&self, id: &str, base_url: &str, key: &str) -> Result<()> {
        ensure!(!key.is_empty() && key.len() <= 16384, "AI_KEY_REQUIRED");
        let mut vault = self.vault.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        if !vault.unlocked() {
            self.volatile
                .lock()
                .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
                .insert(id.into(), StoredKey::new(id, base_url, key));
        } else {
            let mut values = vault.values()?.clone();
            values.insert(id.to_owned(), StoredKey::new(id, base_url, key));
            vault.replace(values)?;
            self.volatile
                .lock()
                .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
                .remove(id);
        }
        Ok(())
    }
    pub fn delete_key(&self, id: &str) -> Result<()> {
        let mut vault = self.vault.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        if vault.unlocked() {
            let mut values = vault.values()?.clone();
            values.remove(id);
            vault.replace(values)?;
        } else if self.root.join("keys.enc").exists() {
            return Err(anyhow!("AI_KEY_LOCKED"));
        }
        drop(vault);
        let mut volatile = self
            .volatile
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        let deletion =
            Self::entry(&self.service, id).and_then(|entry| match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err(anyhow!("AI_VAULT_UNAVAILABLE")),
            });
        if !self.has_key_file() {
            deletion?;
        }
        volatile.remove(id);
        Ok(())
    }

    pub fn unlock_keys(&self, password: &str, settings: &Settings) -> Result<()> {
        let mut vault = self.vault.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        vault.unlock(password)?;
        let mut values = vault.values()?.clone();
        let mut migrated = Vec::new();
        for provider in &settings.providers {
            // Once a password vault exists it is authoritative. Native leftovers
            // from a failed cleanup must never resurrect a cleared or removed key.
            if self.has_key_file() {
                break;
            }
            if let Ok(entry) = Self::entry(&self.service, &provider.id)
                && let Ok(key) = entry.get_password()
                && !key.is_empty()
            {
                values
                    .entry(provider.id.clone())
                    .or_insert_with(|| StoredKey::new(&provider.id, &provider.base_url, &key));
                migrated.push(entry);
            }
        }
        let mut volatile = self
            .volatile
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        for (id, key) in volatile.iter() {
            values.insert(id.clone(), key.clone());
        }
        if values != *vault.values()?
            || (!migrated.is_empty() && !self.root.join("keys.enc").exists())
        {
            vault.replace(values)?;
        }
        volatile.clear();
        // Delete legacy entries only after the password-encrypted file is durable.
        for entry in migrated {
            entry
                .delete_credential()
                .map_err(|_| anyhow!("AI_VAULT_UNAVAILABLE"))?;
        }
        Ok(())
    }

    pub fn rotate_keys(&self, password: &str) -> Result<()> {
        self.vault
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
            .rotate(password)
    }

    /// Preflight reads legacy keys only after cloud password verification. A
    /// failed upload must not delete native credentials or create a new vault.
    pub fn prepare_keys(&self, password: &str, settings: &Settings) -> Result<()> {
        let mut vault = self.vault.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        if !vault.unlocked() {
            vault.unlock(password)?;
        }
        let mut volatile = self
            .volatile
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        for provider in &settings.providers {
            if self.has_key_file() {
                break;
            }
            if vault.values()?.contains_key(&provider.id) || volatile.contains_key(&provider.id) {
                continue;
            }
            if let Ok(entry) = Self::entry(&self.service, &provider.id)
                && let Ok(key) = entry.get_password()
                && !key.is_empty()
            {
                volatile.insert(
                    provider.id.clone(),
                    StoredKey::new(&provider.id, &provider.base_url, &key),
                );
            }
        }
        Ok(())
    }

    pub fn keys_locked(&self) -> bool {
        self.vault.lock().is_ok_and(|vault| !vault.unlocked())
    }
    pub fn has_key_file(&self) -> bool {
        self.root.join("keys.enc").exists()
    }
    pub fn adopt_password(&self, password: &str, settings: &Settings) -> Result<()> {
        if self.keys_locked() {
            return self.unlock_keys(password, settings);
        }
        {
            let mut vault = self.vault.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
            if !vault.uses_password(password) {
                vault.rotate(password)?;
            }
        }
        self.unlock_keys(password, settings)
    }

    pub fn replace_keys(
        &self,
        keys: &std::collections::BTreeMap<String, String>,
        settings: &Settings,
    ) -> Result<()> {
        let values: std::collections::BTreeMap<_, _> = settings
            .providers
            .iter()
            .filter_map(|provider| {
                keys.get(&provider.id)
                    .filter(|key| !key.is_empty())
                    .map(|key| {
                        (
                            provider.id.clone(),
                            StoredKey::new(&provider.id, &provider.base_url, key),
                        )
                    })
            })
            .collect();
        let mut vault = self.vault.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        let mut volatile = self
            .volatile
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        if vault.unlocked() {
            vault.replace(values)?;
            volatile.clear();
        } else {
            ensure!(!self.root.join("keys.enc").exists(), "AI_KEY_LOCKED");
            *volatile = values.into_iter().collect();
        }
        Ok(())
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        atomic_bytes(
            &self.root.join("providers.json"),
            &serde_json::to_vec_pretty(settings)?,
        )
        .map_err(|_| anyhow!("AI_STORAGE_ERROR"))
    }
    fn path(&self, id: &str) -> Result<PathBuf> {
        ensure!(uuid::Uuid::parse_str(id).is_ok(), "AI_CONVERSATION_MISSING");
        Ok(self.root.join("conversations").join(format!("{id}.enc")))
    }
    pub fn save(&self, c: &Conversation) -> Result<()> {
        let Some(key) = &self.master else {
            return Ok(());
        };
        let bytes = serde_json::to_vec(c)?;
        ensure!(bytes.len() < 16 * 1024 * 1024, "AI_HISTORY_LIMIT");
        atomic_bytes(&self.path(&c.id)?, &seal(key, &c.id, &bytes)?)
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))
    }
    pub fn delete(&self, id: &str) -> Result<()> {
        let path = self.path(id)?;
        if path.exists() {
            let _ = private_bytes(&path, 16 * 1024 * 1024)?;
            std::fs::remove_file(path).map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unfinished_followups_recover_without_replaying_the_completed_chat() {
        let mut c = Conversation::new(None, &Settings::default());
        c.followups = Some(crate::ai::Followups {
            turn_id: "turn".into(),
            request_id: "request".into(),
            state: FollowupState::Generating,
            items: Vec::new(),
        });
        recover_background_requests(&mut c);
        assert!(c.state == TurnState::Idle);
        assert_eq!(c.followups.unwrap().state, FollowupState::Failed);
        assert!(c.messages.is_empty());
    }

    #[test]
    fn idle_chat_recovers_an_interrupted_title_without_replaying_or_losing_usage() -> Result<()> {
        let mut c = Conversation::new(None, &Settings::default());
        c.title = "原有临时标题".into();
        c.title_state = TitleState::Generating;
        c.requests.push(serde_json::from_value(serde_json::json!({
            "id":"request","conversation_id":c.id,"turn_id":"turn","purpose":"title",
            "provider_id":"provider","provider_name":"fixture","model":"fixture",
            "conversation_title":c.title,"started_ms":1,"state":"pending",
            "usage":{"input":3,"output":2,"total":5}
        }))?);
        recover_background_requests(&mut c);
        assert_eq!(c.title_state, TitleState::Failed);
        assert_eq!(c.title, "原有临时标题");
        assert!(c.state == TurnState::Idle);
        assert_eq!(c.requests[0].state, "interrupted");
        assert!(c.requests[0].finished_ms.is_some());
        assert_eq!(c.usage_summary().total, 5);
        assert_eq!(c.usage_summary().pending, 0);
        Ok(())
    }

    #[test]
    fn passwordless_keys_are_temporary_and_addresses_are_bound() -> Result<()> {
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4().to_string();
        let url = "https://fixture.example.test/v1";
        let store = Store::fixture(directory.clone(), None, &id, url, "synthetic-key")?;
        assert!(store.key_temporary(&id));
        assert!(!store.has_key_file());
        assert_eq!(store.key(&id, url)?, "synthetic-key");
        assert!(store.key(&id, "https://other.example.test/v1").is_err());
        store.unlock_keys("sync-password", &Settings::default())?;
        assert!(!store.key_temporary(&id));
        assert!(store.key_saved(&id));
        assert!(
            !String::from_utf8_lossy(&std::fs::read(directory.join("ai/keys.enc"))?)
                .contains("synthetic-key")
        );
        let reopened = Store::fixture(directory.clone(), None, "unused", url, "temporary")?;
        assert!(reopened.key(&id, url).is_err());
        assert!(
            reopened
                .unlock_keys("wrong-password", &Settings::default())
                .is_err()
        );
        reopened.unlock_keys("sync-password", &Settings::default())?;
        assert_eq!(reopened.key(&id, url)?, "synthetic-key");
        assert!(reopened.key(&id, "https://other.example.test/v1").is_err());
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires the current Windows Credential Manager"]
    fn native_vault_roundtrip() -> Result<()> {
        let name = format!("tiny-shell-ai-vault-test-{}", uuid::Uuid::new_v4());
        let entry = Store::entry(&name, "synthetic")?;
        struct Cleanup(keyring::Entry);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = self.0.delete_credential();
            }
        }
        let guard = Cleanup(entry);
        guard
            .0
            .set_secret(&[17; 32])
            .map_err(|_| anyhow!("AI_VAULT_UNAVAILABLE"))?;
        assert_eq!(
            guard
                .0
                .get_secret()
                .map_err(|_| anyhow!("AI_VAULT_UNAVAILABLE"))?,
            [17; 32]
        );
        guard
            .0
            .set_password("synthetic-test-only")
            .map_err(|_| anyhow!("AI_VAULT_UNAVAILABLE"))?;
        assert_eq!(
            guard
                .0
                .get_password()
                .map_err(|_| anyhow!("AI_VAULT_UNAVAILABLE"))?,
            "synthetic-test-only"
        );
        guard
            .0
            .delete_credential()
            .map_err(|_| anyhow!("AI_VAULT_UNAVAILABLE"))?;
        assert!(matches!(
            guard.0.get_password(),
            Err(keyring::Error::NoEntry)
        ));
        Ok(())
    }

    #[test]
    fn temporary_history_never_overwrites_existing_ciphertext() -> Result<()> {
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
        let store = Store::fixture(
            directory.clone(),
            Some([19; 32]),
            "fixture",
            "https://fixture.example.test/v1",
            "fixture-key",
        )?;
        let mut c = Conversation::new(None, &Settings::default());
        c.messages
            .push(super::super::Message::text("user", "preserve this"));
        store.save(&c)?;
        let path = store.path(&c.id)?;
        let cipher = std::fs::read(&path)?;
        let locked = Store::fixture(
            directory.clone(),
            None,
            "fixture",
            "https://fixture.example.test/v1",
            "temporary-key",
        )?;
        c.messages
            .push(super::super::Message::text("user", "memory only"));
        locked.save(&c)?;
        assert_eq!(std::fs::read(path)?, cipher);
        let settings = Settings::default();
        locked.save_settings(&settings)?;
        assert!(
            !String::from_utf8(std::fs::read(directory.join("ai/providers.json"))?)?
                .contains("temporary-key")
        );
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}
