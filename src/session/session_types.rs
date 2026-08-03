use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct SftpToolbarVisibility {
    pub sync_cwd: bool,
    pub hidden_files: bool,
    pub refresh: bool,
    pub new_folder: bool,
    pub delete: bool,
    pub upload_file: bool,
    pub upload_folder: bool,
    pub download: bool,
}

impl Default for SftpToolbarVisibility {
    fn default() -> Self {
        Self {
            sync_cwd: true,
            hidden_files: true,
            refresh: true,
            new_folder: false,
            delete: false,
            upload_file: false,
            upload_folder: false,
            download: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct SftpFooterVisibility {
    pub webdav: bool,
    pub latency: bool,
    pub transfers: bool,
    pub panel_toggle: bool,
}

impl Default for SftpFooterVisibility {
    fn default() -> Self {
        Self {
            webdav: true,
            latency: true,
            transfers: true,
            panel_toggle: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickCommand {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub remark: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickCommandCategory {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub commands: Vec<QuickCommand>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Password,
    Key,
    #[serde(rename = "key-pending")]
    KeyPending,
    Config,
}

/// A user-imported SSH private key managed by tiny-shell.
///
/// The key file content is copied into `inline_content` at import time,
/// so deleting the original file does not affect connections that use
/// this managed key.
#[derive(Clone, Serialize, Deserialize)]
pub struct ManagedKey {
    pub id: String,
    /// User-given name / remark for this key.
    pub name: String,
    /// Detected key type: "ed25519", "rsa", "ecdsa", "dsa", or "unknown".
    #[serde(default)]
    pub key_type: String,
    /// SHA256 fingerprint string (e.g. "SHA256:xxxx...") for display & dedup.
    #[serde(default)]
    pub fingerprint: String,
    /// The actual private key file content (copy of the original).
    pub inline_content: String,
    /// Passphrase for the key (stored in plaintext, same security level
    /// as Session.password — file is 0o600 on unix).
    #[serde(default)]
    pub passphrase: String,
    /// Import timestamp (unix epoch seconds).
    #[serde(default)]
    pub created_at: i64,
}

impl fmt::Debug for ManagedKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedKey")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("key_type", &self.key_type)
            .field("fingerprint", &self.fingerprint)
            .field("inline_content", &"<redacted>")
            .field("passphrase", &"<redacted>")
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: AuthMethod,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub private_key_path: String,
    #[serde(default)]
    pub private_key_inline: String,
    #[serde(default)]
    pub passphrase: String,
    /// Reference to a ManagedKey.id. When set, the key content is resolved
    /// from the managed key at connection time (not stored in the session).
    #[serde(default)]
    pub managed_key_id: Option<String>,
    #[serde(default)]
    pub last_used: Option<String>,
    /// Optional user-created folder in the connection manager.
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default = "crate::session::config_file::default_global_proxy_type")]
    pub proxy_type: String, // "none", "socks5", "http"
    #[serde(default)]
    pub proxy_host: String,
    #[serde(default)]
    pub proxy_port: Option<u16>,
    #[serde(default)]
    pub proxy_user: String,
    #[serde(default)]
    pub proxy_password: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DeletedSession {
    pub session: Session,
    #[serde(default)]
    pub deleted_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DeletedConnectionGroup {
    pub name: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub deleted_at: i64,
}

impl fmt::Debug for DeletedSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeletedSession")
            .field("session", &self.session.name)
            .field("deleted_at", &self.deleted_at)
            .finish()
    }
}

impl fmt::Debug for DeletedConnectionGroup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeletedConnectionGroup")
            .field("name", &self.name)
            .field("group_count", &self.groups.len())
            .field("session_count", &self.sessions.len())
            .field("deleted_at", &self.deleted_at)
            .finish()
    }
}

impl Session {
    pub fn requires_credential_prompt(&self) -> bool {
        match self.auth {
            AuthMethod::Password => self.password.is_empty(),
            AuthMethod::KeyPending => true,
            AuthMethod::Key => {
                self.managed_key_id.is_none()
                    && self.private_key_path.is_empty()
                    && self.private_key_inline.is_empty()
            }
            AuthMethod::Config => false,
        }
    }

    pub fn password(host: String, port: u16, user: String, password: String) -> Self {
        let name = format!("{user}@{host}");
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            host,
            port,
            user,
            auth: AuthMethod::Password,
            password,
            private_key_path: String::new(),
            private_key_inline: String::new(),
            passphrase: String::new(),
            managed_key_id: None,
            last_used: None,
            group: None,
            proxy_type: "none".to_string(),
            proxy_host: String::new(),
            proxy_port: None,
            proxy_user: String::new(),
            proxy_password: String::new(),
        }
    }

    pub fn key(
        host: String,
        port: u16,
        user: String,
        private_key_path: String,
        private_key_inline: String,
        passphrase: String,
    ) -> Self {
        let name = format!("{user}@{host}");
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            host,
            port,
            user,
            auth: AuthMethod::Key,
            password: String::new(),
            private_key_path,
            private_key_inline,
            passphrase,
            managed_key_id: None,
            last_used: None,
            group: None,
            proxy_type: "none".to_string(),
            proxy_host: String::new(),
            proxy_port: None,
            proxy_user: String::new(),
            proxy_password: String::new(),
        }
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Session")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("auth", &self.auth)
            .field("password", &"<redacted>")
            .field("private_key_path", &self.private_key_path)
            .field("private_key_inline", &"<redacted>")
            .field("passphrase", &"<redacted>")
            .field("managed_key_id", &self.managed_key_id)
            .field("last_used", &self.last_used)
            .field("group", &self.group)
            .field("proxy_type", &self.proxy_type)
            .field("proxy_host", &self.proxy_host)
            .field("proxy_port", &self.proxy_port)
            .field("proxy_user", &self.proxy_user)
            .field("proxy_password", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_debug_output_is_redacted() {
        let mut session = Session::password(
            "host".to_string(),
            22,
            "user".to_string(),
            "session-password".to_string(),
        );
        session.private_key_inline = "private-key-content".to_string();
        session.passphrase = "key-passphrase".to_string();
        session.proxy_password = "proxy-password".to_string();
        let key = ManagedKey {
            id: "key-id".to_string(),
            name: "key-name".to_string(),
            key_type: "ed25519".to_string(),
            fingerprint: "fingerprint".to_string(),
            inline_content: "managed-private-key".to_string(),
            passphrase: "managed-passphrase".to_string(),
            created_at: 0,
        };

        let debug = format!("{session:?} {key:?}");
        for secret in [
            "session-password",
            "private-key-content",
            "key-passphrase",
            "proxy-password",
            "managed-private-key",
            "managed-passphrase",
        ] {
            assert!(!debug.contains(secret));
        }
    }

    #[test]
    fn only_incomplete_imported_credentials_require_a_prompt() {
        let empty_password =
            Session::password("example.test".into(), 22, "alice".into(), String::new());
        assert!(empty_password.requires_credential_prompt());

        let password =
            Session::password("example.test".into(), 22, "alice".into(), "secret".into());
        assert!(!password.requires_credential_prompt());

        let default_key = Session::key(
            "example.test".into(),
            22,
            "alice".into(),
            String::new(),
            String::new(),
            String::new(),
        );
        assert!(default_key.requires_credential_prompt());

        let mut pending_key = default_key;
        pending_key.auth = AuthMethod::KeyPending;
        assert!(pending_key.requires_credential_prompt());
    }

    #[test]
    fn sftp_footer_visibility_defaults_missing_sync_status_to_visible() {
        let visibility: SftpFooterVisibility =
            serde_json::from_str(r#"{"latency":false,"transfers":false,"panel_toggle":false}"#)
                .unwrap();

        assert!(visibility.webdav);
        assert!(!visibility.latency);
        assert!(!visibility.transfers);
        assert!(!visibility.panel_toggle);
    }
}
