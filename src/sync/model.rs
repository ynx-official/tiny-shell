use anyhow::Result;
#[cfg(test)]
use anyhow::{Context, anyhow};
#[cfg(test)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
#[cfg(test)]
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use uuid::Uuid;

#[cfg(test)]
use crate::session::config::QuickCommandCategory;
use crate::{
    crypto,
    session::config::{
        AuthMethod, ConnectionType, DeletedConnectionGroup, DeletedSession, ManagedKey, Session,
    },
};

#[cfg(test)]
pub const FORMAT_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum SyncSecret {
    Omitted,
    Empty,
    Encrypted(String),
    LegacyPlaintext(String),
}

impl SyncSecret {
    pub fn export(value: &str, include_secrets: bool, password: &str) -> Result<Self> {
        if !include_secrets {
            return Ok(Self::Omitted);
        }
        if value.is_empty() {
            return Ok(Self::Empty);
        }
        crypto::encrypt_field(value, password).map(Self::Encrypted)
    }

    #[cfg(test)]
    fn from_legacy(value: String) -> Self {
        if value.is_empty() {
            Self::Omitted
        } else if crypto::is_sealed_field(&value) {
            Self::Encrypted(value)
        } else {
            Self::LegacyPlaintext(value)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSession {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub connection_type: ConnectionType,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: AuthMethod,
    pub password: SyncSecret,
    pub private_key_path: String,
    pub private_key_inline: SyncSecret,
    pub passphrase: SyncSecret,
    pub managed_key_id: Option<String>,
    pub last_used: Option<String>,
    pub group: Option<String>,
    pub proxy_type: String,
    pub proxy_host: String,
    pub proxy_port: Option<u16>,
    pub proxy_user: String,
    pub proxy_password: SyncSecret,
}

impl SyncSession {
    pub fn export(session: Session, include_secrets: bool, password: &str) -> Result<Self> {
        Ok(Self {
            id: session.id,
            name: session.name,
            connection_type: session.connection_type,
            host: session.host,
            port: session.port,
            user: session.user,
            auth: session.auth,
            password: SyncSecret::export(&session.password, include_secrets, password)?,
            private_key_path: session.private_key_path,
            private_key_inline: SyncSecret::export(
                &session.private_key_inline,
                include_secrets,
                password,
            )?,
            passphrase: SyncSecret::export(&session.passphrase, include_secrets, password)?,
            managed_key_id: session.managed_key_id,
            last_used: session.last_used,
            group: session.group,
            proxy_type: session.proxy_type,
            proxy_host: session.proxy_host,
            proxy_port: session.proxy_port,
            proxy_user: session.proxy_user,
            proxy_password: SyncSecret::export(&session.proxy_password, include_secrets, password)?,
        })
    }

    #[cfg(test)]
    fn from_legacy(session: Session) -> Self {
        Self {
            id: session.id,
            name: session.name,
            connection_type: session.connection_type,
            host: session.host,
            port: session.port,
            user: session.user,
            auth: session.auth,
            password: SyncSecret::from_legacy(session.password),
            private_key_path: session.private_key_path,
            private_key_inline: SyncSecret::from_legacy(session.private_key_inline),
            passphrase: SyncSecret::from_legacy(session.passphrase),
            managed_key_id: session.managed_key_id,
            last_used: session.last_used,
            group: session.group,
            proxy_type: session.proxy_type,
            proxy_host: session.proxy_host,
            proxy_port: session.proxy_port,
            proxy_user: session.proxy_user,
            proxy_password: SyncSecret::from_legacy(session.proxy_password),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDeletedSession {
    pub session: SyncSession,
    pub deleted_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDeletedConnectionGroup {
    pub name: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub sessions: Vec<SyncSession>,
    pub deleted_at: i64,
}

impl SyncDeletedSession {
    pub(crate) fn export(
        value: DeletedSession,
        include_secrets: bool,
        password: &str,
    ) -> Result<Self> {
        Ok(Self {
            session: SyncSession::export(value.session, include_secrets, password)?,
            deleted_at: value.deleted_at,
        })
    }
}

impl SyncDeletedConnectionGroup {
    pub(crate) fn export(
        value: DeletedConnectionGroup,
        include_secrets: bool,
        password: &str,
    ) -> Result<Self> {
        Ok(Self {
            name: value.name,
            groups: value.groups,
            sessions: value
                .sessions
                .into_iter()
                .map(|session| SyncSession::export(session, include_secrets, password))
                .collect::<Result<Vec<_>>>()?,
            deleted_at: value.deleted_at,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManagedKey {
    pub id: String,
    pub name: String,
    pub key_type: String,
    pub fingerprint: String,
    pub inline_content: SyncSecret,
    pub passphrase: SyncSecret,
    pub created_at: i64,
}

impl SyncManagedKey {
    pub fn export(key: ManagedKey, include_secrets: bool, password: &str) -> Result<Self> {
        Ok(Self {
            id: key.id,
            name: key.name,
            key_type: key.key_type,
            fingerprint: key.fingerprint,
            inline_content: SyncSecret::export(&key.inline_content, include_secrets, password)?,
            passphrase: SyncSecret::export(&key.passphrase, include_secrets, password)?,
            created_at: key.created_at,
        })
    }

    #[cfg(test)]
    fn from_legacy(key: ManagedKey) -> Self {
        Self {
            id: key.id,
            name: key.name,
            key_type: key.key_type,
            fingerprint: key.fingerprint,
            inline_content: SyncSecret::from_legacy(key.inline_content),
            passphrase: SyncSecret::from_legacy(key.passphrase),
            created_at: key.created_at,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyPasswordStatus {
    NotConfigured,
    Missing,
    Verified,
    Mismatch,
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPayload {
    pub schema_version: u32,
    pub revision: String,
    pub updated_at: String,
    pub device_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy_password_verifier: Option<String>,
    #[serde(default)]
    pub connection_groups: Vec<String>,
    #[serde(default)]
    pub sessions: Vec<SyncSession>,
    #[serde(default)]
    pub deleted_sessions: Vec<SyncDeletedSession>,
    #[serde(default)]
    pub deleted_connection_groups: Vec<SyncDeletedConnectionGroup>,
    #[serde(default)]
    pub managed_keys: Vec<SyncManagedKey>,
    #[serde(default)]
    pub quick_command_categories: Vec<QuickCommandCategory>,
}

#[cfg(test)]
/// Input bundle for constructing an encrypted sync payload.
pub struct SyncPayloadInput {
    pub device_id: String,
    pub sessions: Vec<Session>,
    pub deleted_sessions: Vec<DeletedSession>,
    pub connection_groups: Vec<String>,
    pub deleted_connection_groups: Vec<DeletedConnectionGroup>,
    pub managed_keys: Vec<ManagedKey>,
    pub quick_command_categories: Vec<QuickCommandCategory>,
    pub include_secrets: bool,
    pub privacy_password: String,
}

#[cfg(test)]
impl SyncPayloadInput {
    /// Convenience constructor for payloads without deleted records.
    #[cfg(test)]
    pub fn no_deleted(
        device_id: String,
        sessions: Vec<Session>,
        connection_groups: Vec<String>,
        managed_keys: Vec<ManagedKey>,
        quick_command_categories: Vec<QuickCommandCategory>,
        include_secrets: bool,
        privacy_password: String,
    ) -> Self {
        Self {
            device_id,
            sessions,
            deleted_sessions: Vec::new(),
            connection_groups,
            deleted_connection_groups: Vec::new(),
            managed_keys,
            quick_command_categories,
            include_secrets,
            privacy_password,
        }
    }
}

#[cfg(test)]
impl SyncPayload {
    #[cfg(test)]
    pub fn new(
        device_id: String,
        sessions: Vec<Session>,
        connection_groups: Vec<String>,
        managed_keys: Vec<ManagedKey>,
        quick_command_categories: Vec<QuickCommandCategory>,
        include_secrets: bool,
        privacy_password: &str,
    ) -> Result<Self> {
        Self::new_with_deleted(SyncPayloadInput::no_deleted(
            device_id,
            sessions,
            connection_groups,
            managed_keys,
            quick_command_categories,
            include_secrets,
            privacy_password.to_string(),
        ))
    }

    #[cfg(test)]
    pub fn new_with_deleted(input: SyncPayloadInput) -> Result<Self> {
        if input.include_secrets && input.privacy_password.chars().count() < 8 {
            return Err(anyhow!(
                "privacy encryption password must be at least 8 characters"
            ));
        }
        let privacy_password_verifier = input
            .include_secrets
            .then(|| crypto::hash_privacy_password(&input.privacy_password))
            .transpose()?;
        let sessions = input
            .sessions
            .into_iter()
            .map(|session| {
                SyncSession::export(session, input.include_secrets, &input.privacy_password)
            })
            .collect::<Result<Vec<_>>>()?;
        let deleted_sessions = input
            .deleted_sessions
            .into_iter()
            .map(|session| {
                SyncDeletedSession::export(session, input.include_secrets, &input.privacy_password)
            })
            .collect::<Result<Vec<_>>>()?;
        let deleted_connection_groups = input
            .deleted_connection_groups
            .into_iter()
            .map(|group| {
                SyncDeletedConnectionGroup::export(
                    group,
                    input.include_secrets,
                    &input.privacy_password,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let managed_keys = input
            .managed_keys
            .into_iter()
            .map(|key| SyncManagedKey::export(key, input.include_secrets, &input.privacy_password))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            schema_version: FORMAT_VERSION,
            revision: Uuid::new_v4().to_string(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            device_id: input.device_id,
            privacy_password_verifier,
            connection_groups: input.connection_groups,
            sessions,
            deleted_sessions,
            deleted_connection_groups,
            managed_keys,
            quick_command_categories: input.quick_command_categories,
        })
    }

    #[cfg(test)]
    pub fn privacy_password_status(&self, password: &str) -> Result<PrivacyPasswordStatus> {
        let Some(verifier) = self.privacy_password_verifier.as_deref() else {
            return Ok(PrivacyPasswordStatus::NotConfigured);
        };
        if password.is_empty() {
            return Ok(PrivacyPasswordStatus::Missing);
        }
        if crypto::verify_privacy_password(password, verifier)? {
            Ok(PrivacyPasswordStatus::Verified)
        } else {
            Ok(PrivacyPasswordStatus::Mismatch)
        }
    }
}

#[cfg(test)]
#[derive(Deserialize)]
struct PayloadVersion {
    schema_version: u32,
}

#[cfg(test)]
#[derive(Deserialize)]
struct LegacyEncryptedEnvelope {
    format_version: u32,
    kdf: String,
    cipher: String,
    salt: String,
    nonce: String,
    payload: String,
}

#[cfg(test)]
#[derive(Deserialize)]
struct LegacyPayloadV1 {
    revision: String,
    updated_at: String,
    device_id: String,
    #[serde(default)]
    sessions: Vec<Session>,
    #[serde(default)]
    managed_keys: Vec<ManagedKey>,
}

#[cfg(test)]
pub fn serialize_payload(payload: &SyncPayload) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(payload).context("serialize sync payload")
}

#[cfg(test)]
pub fn parse_payload(raw: &[u8], legacy_password: &str) -> Result<SyncPayload> {
    if let Ok(version) = serde_json::from_slice::<PayloadVersion>(raw) {
        return parse_versioned_payload(raw, version.schema_version);
    }

    let plaintext = decrypt_legacy_envelope(raw, legacy_password)?;
    let version: PayloadVersion = serde_json::from_slice(&plaintext)
        .context("parse legacy encrypted configuration version")?;
    parse_versioned_payload(&plaintext, version.schema_version)
}

#[cfg(test)]
fn parse_versioned_payload(raw: &[u8], version: u32) -> Result<SyncPayload> {
    match version {
        FORMAT_VERSION => {
            serde_json::from_slice(raw).context("parse synchronized configuration JSON")
        }
        1 => {
            let legacy: LegacyPayloadV1 =
                serde_json::from_slice(raw).context("parse legacy synchronized configuration")?;
            Ok(SyncPayload {
                schema_version: FORMAT_VERSION,
                revision: legacy.revision,
                updated_at: legacy.updated_at,
                device_id: legacy.device_id,
                privacy_password_verifier: None,
                connection_groups: Vec::new(),
                sessions: legacy
                    .sessions
                    .into_iter()
                    .map(SyncSession::from_legacy)
                    .collect(),
                deleted_sessions: Vec::new(),
                deleted_connection_groups: Vec::new(),
                managed_keys: legacy
                    .managed_keys
                    .into_iter()
                    .map(SyncManagedKey::from_legacy)
                    .collect(),
                quick_command_categories: Vec::new(),
            })
        }
        version => Err(anyhow!(
            "unsupported synchronized configuration version {version}"
        )),
    }
}

#[cfg(test)]
fn decrypt_legacy_envelope(raw: &[u8], password: &str) -> Result<Vec<u8>> {
    let envelope: LegacyEncryptedEnvelope =
        serde_json::from_slice(raw).context("parse synchronized configuration JSON")?;
    if envelope.format_version != 1
        || envelope.kdf != "argon2id"
        || envelope.cipher != "xchacha20poly1305"
    {
        return Err(anyhow!("unsupported legacy encrypted sync format"));
    }
    if password.is_empty() {
        return Err(anyhow!(
            "legacy encrypted synchronized configuration requires the privacy password"
        ));
    }

    let salt = STANDARD.decode(envelope.salt).context("decode sync salt")?;
    let nonce = STANDARD
        .decode(envelope.nonce)
        .context("decode sync nonce")?;
    if nonce.len() != 24 {
        return Err(anyhow!("invalid sync nonce"));
    }
    let ciphertext = STANDARD
        .decode(envelope.payload)
        .context("decode encrypted sync payload")?;
    let key = crypto::derive_key(password, &salt)?;
    XChaCha20Poly1305::new((&key).into())
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| anyhow!("cannot decrypt legacy remote configuration; check the password"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::config::{AuthMethod, QuickCommand};

    fn session(password: &str) -> Session {
        Session {
            id: "session-1".into(),
            name: "Production".into(),
            connection_type: ConnectionType::Ssh,
            host: "example.test".into(),
            port: 22,
            user: "alice".into(),
            auth: AuthMethod::Password,
            password: password.into(),
            private_key_path: String::new(),
            private_key_inline: "private-key-content".into(),
            passphrase: "key-passphrase".into(),
            managed_key_id: Some("key-1".into()),
            last_used: None,
            group: Some("Servers/Production".into()),
            proxy_type: "socks5".into(),
            proxy_host: "proxy.test".into(),
            proxy_port: Some(1080),
            proxy_user: "proxy-user".into(),
            proxy_password: "proxy-password".into(),
        }
    }

    fn managed_key() -> ManagedKey {
        ManagedKey {
            id: "key-1".into(),
            name: "Deployment key".into(),
            key_type: "ed25519".into(),
            fingerprint: "SHA256:test".into(),
            inline_content: "managed-private-key".into(),
            passphrase: "managed-key-passphrase".into(),
            created_at: 42,
        }
    }

    fn commands() -> Vec<QuickCommandCategory> {
        vec![QuickCommandCategory {
            id: "category-1".into(),
            name: "Operations".into(),
            commands: vec![QuickCommand {
                id: "command-1".into(),
                name: "Inspect logs".into(),
                remark: "Read only".into(),
                command: "journalctl -n 100".into(),
            }],
        }]
    }

    fn encrypt_legacy_payload(payload: &serde_json::Value, password: &str) -> Vec<u8> {
        let salt = [7_u8; 16];
        let nonce = [9_u8; 24];
        let key = crypto::derive_key(password, &salt).unwrap();
        let plaintext = serde_json::to_vec(payload).unwrap();
        let ciphertext = XChaCha20Poly1305::new((&key).into())
            .encrypt(XNonce::from_slice(&nonce), plaintext.as_ref())
            .unwrap();
        serde_json::to_vec(&serde_json::json!({
            "format_version": 1,
            "kdf": "argon2id",
            "cipher": "xchacha20poly1305",
            "salt": STANDARD.encode(salt),
            "nonce": STANDARD.encode(nonce),
            "payload": STANDARD.encode(ciphertext),
        }))
        .unwrap()
    }

    #[test]
    fn v2_round_trip_keeps_public_domains_readable_and_secrets_encrypted() {
        let payload = SyncPayload::new(
            "device-1".into(),
            vec![session("session-password")],
            vec!["Servers".into(), "Servers/Production".into()],
            vec![managed_key()],
            commands(),
            true,
            "privacy-password",
        )
        .unwrap();

        let serialized = serialize_payload(&payload).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&serialized).unwrap();
        let text = String::from_utf8(serialized.clone()).unwrap();

        assert_eq!(json["schema_version"], FORMAT_VERSION);
        assert!(json["privacy_password_verifier"].is_string());
        assert_eq!(
            payload.privacy_password_status("privacy-password").unwrap(),
            PrivacyPasswordStatus::Verified
        );
        assert_eq!(
            payload.privacy_password_status("").unwrap(),
            PrivacyPasswordStatus::Missing
        );
        assert_eq!(
            payload.privacy_password_status("wrong-password").unwrap(),
            PrivacyPasswordStatus::Mismatch
        );
        assert_eq!(json["sessions"][0]["host"], "example.test");
        assert_eq!(
            json["quick_command_categories"][0]["commands"][0]["command"],
            "journalctl -n 100"
        );
        assert!(!text.contains("session-password"));
        assert!(!text.contains("private-key-content"));
        assert!(!text.contains("managed-private-key"));
        assert!(!text.contains("managed-key-passphrase"));

        let parsed = parse_payload(&serialized, "").unwrap();
        assert_eq!(parsed.connection_groups.len(), 2);
        assert_eq!(parsed.quick_command_categories.len(), 1);
        assert_eq!(parsed.managed_keys.len(), 1);
    }

    #[test]
    fn secrets_disabled_uses_omitted_state_and_excludes_managed_keys() {
        let payload = SyncPayload::new(
            "device-1".into(),
            vec![session("session-password")],
            Vec::new(),
            vec![managed_key()],
            commands(),
            false,
            "",
        )
        .unwrap();

        assert_eq!(payload.sessions[0].password, SyncSecret::Omitted);
        assert!(payload.privacy_password_verifier.is_none());
        assert_eq!(
            payload.privacy_password_status("").unwrap(),
            PrivacyPasswordStatus::NotConfigured
        );
        assert_eq!(payload.managed_keys.len(), 1);
        assert_eq!(payload.managed_keys[0].inline_content, SyncSecret::Omitted);
        assert_eq!(payload.managed_keys[0].passphrase, SyncSecret::Omitted);
        assert_eq!(payload.quick_command_categories.len(), 1);
    }

    #[test]
    fn v1_payload_is_migrated_and_unknown_fields_are_ignored() {
        let legacy = serde_json::json!({
            "schema_version": 1,
            "revision": "legacy-revision",
            "updated_at": "2026-01-01T00:00:00Z",
            "device_id": "legacy-device",
            "unknown_future_field": true,
            "sessions": [session("legacy-password")],
            "managed_keys": [managed_key()]
        });

        let payload = parse_payload(&serde_json::to_vec(&legacy).unwrap(), "").unwrap();

        assert_eq!(payload.schema_version, FORMAT_VERSION);
        assert_eq!(
            payload.sessions[0].password,
            SyncSecret::LegacyPlaintext("legacy-password".into())
        );
        assert_eq!(payload.managed_keys.len(), 1);
        assert!(payload.connection_groups.is_empty());
        assert!(payload.quick_command_categories.is_empty());
    }

    #[test]
    fn legacy_encrypted_payload_has_a_read_only_migration_path() {
        let legacy = serde_json::json!({
            "schema_version": 1,
            "revision": "encrypted-legacy-revision",
            "updated_at": "2026-01-01T00:00:00Z",
            "device_id": "legacy-device",
            "sessions": [session("legacy-password")],
            "managed_keys": [managed_key()]
        });
        let encrypted = encrypt_legacy_payload(&legacy, "legacy-password");

        assert!(parse_payload(&encrypted, "wrong-password").is_err());
        let payload = parse_payload(&encrypted, "legacy-password").unwrap();

        assert_eq!(payload.schema_version, FORMAT_VERSION);
        assert_eq!(payload.revision, "encrypted-legacy-revision");
        assert_eq!(
            payload.sessions[0].password,
            SyncSecret::LegacyPlaintext("legacy-password".into())
        );
    }

    #[test]
    fn secret_export_requires_a_strong_enough_password() {
        assert!(
            SyncPayload::new(
                "device-1".into(),
                vec![session("password")],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                true,
                "short",
            )
            .is_err()
        );
    }
}
