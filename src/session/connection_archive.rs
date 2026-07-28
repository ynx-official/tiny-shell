#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use argon2::Argon2;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};

use super::config::{ConfigStore, Session};

const ARCHIVE_VERSION: u32 = 1;
const ARCHIVE_KIND: &str = "tiny-shell.connection-archive";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionArchive {
    pub kind: String,
    pub version: u32,
    #[serde(default)]
    pub connection_groups: Vec<String>,
    #[serde(default)]
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedArchive {
    kind: String,
    version: u32,
    kdf: String,
    cipher: String,
    include_secrets: bool,
    salt: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionArchiveImportSummary {
    pub imported_groups: usize,
    pub imported_sessions: usize,
}

impl ConnectionArchive {
    pub fn new(connection_groups: &[String], sessions: &[Session], include_secrets: bool) -> Self {
        let sessions = sessions
            .iter()
            .cloned()
            .map(|mut session| {
                if !include_secrets {
                    clear_session_secrets(&mut session);
                }
                session
            })
            .collect();
        Self {
            kind: ARCHIVE_KIND.to_string(),
            version: ARCHIVE_VERSION,
            connection_groups: connection_groups.to_vec(),
            sessions,
        }
    }

    pub fn export_json(&self, password: &str) -> Result<String> {
        if password.is_empty() {
            bail!("archive password cannot be empty");
        }
        if self.kind != ARCHIVE_KIND || self.version != ARCHIVE_VERSION {
            bail!("unsupported connection archive version");
        }

        let mut salt = [0_u8; 16];
        let mut nonce = [0_u8; 24];
        OsRng.fill_bytes(&mut salt);
        OsRng.fill_bytes(&mut nonce);
        let key = derive_key(password, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| anyhow!("failed to initialize archive cipher"))?;
        let plaintext = serde_json::to_vec(self)?;
        let ciphertext = cipher
            .encrypt(XNonce::from_slice(&nonce), plaintext.as_ref())
            .map_err(|_| anyhow!("failed to encrypt connection archive"))?;
        let envelope = EncryptedArchive {
            kind: ARCHIVE_KIND.to_string(),
            version: ARCHIVE_VERSION,
            kdf: "argon2id".to_string(),
            cipher: "xchacha20-poly1305".to_string(),
            include_secrets: contains_secrets(&self.sessions),
            salt: STANDARD.encode(salt),
            nonce: STANDARD.encode(nonce),
            ciphertext: STANDARD.encode(ciphertext),
        };
        Ok(serde_json::to_string_pretty(&envelope)?)
    }
}

pub fn import_json(json: &str, password: &str) -> Result<ConnectionArchive> {
    if password.is_empty() {
        bail!("archive password cannot be empty");
    }
    let envelope: EncryptedArchive = serde_json::from_str(json)
        .map_err(|error| anyhow!("invalid connection archive: {error}"))?;
    if envelope.kind != ARCHIVE_KIND || envelope.version != ARCHIVE_VERSION {
        bail!("unsupported connection archive version");
    }
    if envelope.kdf != "argon2id" || envelope.cipher != "xchacha20-poly1305" {
        bail!("unsupported connection archive encryption");
    }
    let salt = decode_fixed::<16>(&envelope.salt, "archive salt")?;
    let nonce = decode_fixed::<24>(&envelope.nonce, "archive nonce")?;
    let ciphertext = STANDARD
        .decode(envelope.ciphertext)
        .map_err(|error| anyhow!("invalid archive ciphertext: {error}"))?;
    let key = derive_key(password, &salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|_| anyhow!("failed to initialize archive cipher"))?;
    let plaintext = cipher
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| anyhow!("wrong archive password or corrupted archive"))?;
    let archive: ConnectionArchive = serde_json::from_slice(&plaintext)
        .map_err(|error| anyhow!("invalid decrypted connection archive: {error}"))?;
    if archive.kind != ARCHIVE_KIND || archive.version != ARCHIVE_VERSION {
        bail!("unsupported decrypted connection archive version");
    }
    Ok(archive)
}

pub fn apply_import(
    config: &mut ConfigStore,
    archive: ConnectionArchive,
) -> ConnectionArchiveImportSummary {
    let mut groups = archive.connection_groups;
    groups.sort_by_key(|group| (group.split('/').count(), group.to_lowercase()));
    groups.dedup();

    let mut group_mapping = HashMap::new();
    for group in groups {
        let requested = group
            .rsplit_once('/')
            .and_then(|(parent, leaf)| {
                group_mapping
                    .get(parent)
                    .map(|mapped_parent: &String| format!("{mapped_parent}/{leaf}"))
            })
            .unwrap_or_else(|| group.clone());
        let imported = unique_name(
            config.connection_groups().iter().map(String::as_str),
            &requested,
        );
        config.add_connection_group(imported.clone());
        group_mapping.insert(group, imported);
    }

    let imported_groups = group_mapping.len();
    let imported_sessions = archive.sessions.len();
    for mut session in archive.sessions {
        session.id = uuid::Uuid::new_v4().to_string();
        session.group = session
            .group
            .and_then(|group| group_mapping.get(&group).cloned().or(Some(group)));
        session.name = unique_name(
            config
                .sessions()
                .iter()
                .filter(|existing| existing.group == session.group)
                .map(|existing| existing.name.as_str()),
            &session.name,
        );
        config.upsert(session);
    }

    ConnectionArchiveImportSummary {
        imported_groups,
        imported_sessions,
    }
}

fn unique_name<'a>(existing: impl Iterator<Item = &'a str>, requested: &str) -> String {
    let existing = existing.collect::<std::collections::HashSet<_>>();
    if !existing.contains(requested) {
        return requested.to_string();
    }
    (2_u32..)
        .map(|suffix| format!("{requested} ({suffix})"))
        .find(|candidate| !existing.contains(candidate.as_str()))
        .unwrap_or_else(|| format!("{requested} ({})", uuid::Uuid::new_v4()))
}

fn derive_key(password: &str, salt: &[u8; 16]) -> Result<[u8; 32]> {
    let mut key = [0_u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|error| anyhow!("failed to derive archive key: {error}"))?;
    Ok(key)
}

fn decode_fixed<const N: usize>(value: &str, label: &str) -> Result<[u8; N]> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|error| anyhow!("invalid {label}: {error}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("invalid {label} length"))
}

fn clear_session_secrets(session: &mut Session) {
    session.password.clear();
    session.private_key_inline.clear();
    session.passphrase.clear();
    session.proxy_password.clear();
}

fn contains_secrets(sessions: &[Session]) -> bool {
    sessions.iter().any(|session| {
        !session.password.is_empty()
            || !session.private_key_inline.is_empty()
            || !session.passphrase.is_empty()
            || !session.proxy_password.is_empty()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let mut value = Session::password(
            "example.com".to_string(),
            22,
            "alice".to_string(),
            "secret-password".to_string(),
        );
        value.name = "production".to_string();
        value.private_key_inline = "PRIVATE KEY".to_string();
        value.passphrase = "key-passphrase".to_string();
        value.proxy_password = "proxy-secret".to_string();
        value
    }

    #[test]
    fn encrypted_archive_round_trips_with_password() {
        let original = ConnectionArchive::new(&["prod".to_string()], &[session()], true);
        let json = original.export_json("correct horse").unwrap();
        let restored = import_json(&json, "correct horse").unwrap();
        assert_eq!(restored.kind, original.kind);
        assert_eq!(restored.version, original.version);
        assert_eq!(restored.connection_groups, original.connection_groups);
        assert_eq!(restored.sessions.len(), original.sessions.len());
        assert_eq!(restored.sessions[0].name, original.sessions[0].name);
        assert_eq!(restored.sessions[0].password, original.sessions[0].password);
        assert!(!json.contains("secret-password"));
        assert!(!json.contains("PRIVATE KEY"));
    }

    #[test]
    fn wrong_password_and_tampering_fail_without_partial_data() {
        let archive = ConnectionArchive::new(&[], &[session()], true);
        let json = archive.export_json("password").unwrap();
        assert!(import_json(&json, "wrong").is_err());

        let mut envelope: serde_json::Value = serde_json::from_str(&json).unwrap();
        let ciphertext = envelope["ciphertext"].as_str().unwrap().to_string();
        envelope["ciphertext"] = serde_json::Value::String(format!("{ciphertext}x"));
        let tampered = serde_json::to_string(&envelope).unwrap();
        assert!(import_json(&tampered, "password").is_err());
    }

    #[test]
    fn public_export_strips_sensitive_fields_before_encryption() {
        let archive = ConnectionArchive::new(&[], &[session()], false);
        let json = archive.export_json("password").unwrap();
        let restored = import_json(&json, "password").unwrap();
        let restored_session = &restored.sessions[0];
        assert!(restored_session.password.is_empty());
        assert!(restored_session.private_key_inline.is_empty());
        assert!(restored_session.passphrase.is_empty());
        assert!(restored_session.proxy_password.is_empty());
    }

    #[test]
    fn incompatible_version_is_rejected_before_decryption() {
        let archive = ConnectionArchive::new(&[], &[], false);
        let json = archive.export_json("password").unwrap();
        let mut envelope: serde_json::Value = serde_json::from_str(&json).unwrap();
        envelope["version"] = serde_json::Value::from(999);
        assert!(import_json(&serde_json::to_string(&envelope).unwrap(), "password").is_err());
    }

    #[test]
    fn import_renames_conflicting_group_tree_and_sessions() {
        let mut config = ConfigStore::in_memory();
        config.add_connection_group("prod".to_string());
        let mut existing = session();
        existing.id = "existing".to_string();
        existing.group = Some("prod".to_string());
        config.upsert(existing);

        let mut imported_root = session();
        imported_root.id = "archive-root".to_string();
        imported_root.group = Some("prod".to_string());
        let mut imported_child = session();
        imported_child.id = "archive-child".to_string();
        imported_child.group = Some("prod/eu".to_string());
        let archive = ConnectionArchive::new(
            &["prod".to_string(), "prod/eu".to_string()],
            &[imported_root, imported_child],
            true,
        );

        let summary = apply_import(&mut config, archive);

        assert_eq!(summary.imported_groups, 2);
        assert_eq!(summary.imported_sessions, 2);
        assert!(
            config
                .connection_groups()
                .iter()
                .any(|group| group == "prod (2)/eu")
        );
        assert!(config.sessions().iter().any(|item| {
            item.id != "archive-root"
                && item.group.as_deref() == Some("prod (2)")
                && item.name == "production"
        }));
        assert!(config.sessions().iter().any(|item| {
            item.id != "archive-child"
                && item.group.as_deref() == Some("prod (2)/eu")
                && item.name == "production"
        }));
    }
}
