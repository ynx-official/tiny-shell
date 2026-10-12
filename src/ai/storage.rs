use super::{Conversation, Settings, TurnState};
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
    volatile: Mutex<HashMap<String, String>>,
}
type Opened = (Store, Settings, Vec<Conversation>, Option<String>);
impl Store {
    #[cfg(test)]
    pub fn fixture(
        directory: PathBuf,
        master: Option<[u8; 32]>,
        id: &str,
        key: &str,
    ) -> Result<Self> {
        let root = directory.join("ai");
        private_directory(&root)?;
        private_directory(&root.join("conversations"))?;
        Ok(Self {
            root,
            service: format!("tiny-shell-test-{}", uuid::Uuid::new_v4()),
            master,
            volatile: Mutex::new(HashMap::from([(id.to_owned(), key.to_owned())])),
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
                        for activity in &mut c.activities {
                            if matches!(
                                activity.state.as_str(),
                                "running" | "queued" | "awaiting_user" | "cancel_requested"
                            ) {
                                activity.state = "interrupted".into();
                            }
                        }
                    }
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
        Self::entry(&self.service, id)
            .and_then(|e| {
                e.get_password()
                    .map_err(|_| anyhow!("AI_VAULT_UNAVAILABLE"))
            })
            .is_ok_and(|k| !k.is_empty())
    }
    pub fn key(&self, id: &str) -> Result<String> {
        if let Some(key) = self
            .volatile
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
            .get(id)
        {
            return Ok(key.clone());
        }
        match Self::entry(&self.service, id)?.get_password() {
            Ok(k) => Ok(k),
            Err(keyring::Error::NoEntry) => Ok(String::new()),
            Err(_) => Err(anyhow!("AI_VAULT_UNAVAILABLE")),
        }
    }
    pub fn set_key(&self, id: &str, key: &str) -> Result<()> {
        ensure!(!key.is_empty() && key.len() <= 16384, "AI_KEY_REQUIRED");
        if Self::entry(&self.service, id)
            .and_then(|e| {
                e.set_password(key)
                    .map_err(|_| anyhow!("AI_VAULT_UNAVAILABLE"))
            })
            .is_err()
        {
            self.volatile
                .lock()
                .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
                .insert(id.into(), key.into());
        } else {
            self.volatile
                .lock()
                .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
                .remove(id);
        }
        Ok(())
    }
    pub fn delete_key(&self, id: &str) -> Result<()> {
        let mut volatile = self
            .volatile
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        match Self::entry(&self.service, id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_) => return Err(anyhow!("AI_VAULT_UNAVAILABLE")),
        };
        volatile.remove(id);
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
        let store = Store::fixture(directory.clone(), Some([19; 32]), "fixture", "fixture-key")?;
        let mut c = Conversation::new(None, &Settings::default());
        c.messages
            .push(super::super::Message::text("user", "preserve this"));
        store.save(&c)?;
        let path = store.path(&c.id)?;
        let cipher = std::fs::read(&path)?;
        let locked = Store::fixture(directory.clone(), None, "fixture", "temporary-key")?;
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
