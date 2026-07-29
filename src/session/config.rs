use std::{
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use directories::BaseDirs;
use rand::{RngCore, rngs::OsRng};
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Password,
    Key,
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
    #[serde(default = "default_global_proxy_type")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SavedWindowBounds {
    Fullscreen {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    Maximized {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    Windowed {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TitleBarStyle {
    Native,
    #[default]
    Integrated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CursorStyle {
    #[default]
    Default,
    Blink,
    Beam,
    BeamBlink,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalDisplayStyle {
    #[default]
    Standard,
    Compact,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateCheckMode {
    #[default]
    Startup,
    Interval,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default = "default_follow_system_theme")]
    pub follow_system_theme: bool,
    #[serde(default)]
    pub theme_mode: String,
    #[serde(default)]
    pub light_theme_name: String,
    #[serde(default)]
    pub dark_theme_name: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: f32,
    #[serde(default)]
    pub terminal_display_style: TerminalDisplayStyle,
    #[serde(default = "default_ui_font_size")]
    pub ui_font_size: f32,
    #[serde(default)]
    pub keyword_highlight: bool,
    #[serde(default = "default_ui_font_family")]
    pub ui_font_family: String,
    #[serde(default = "default_terminal_font_family")]
    pub terminal_font_family: String,
    #[serde(default)]
    pub title_bar_style: TitleBarStyle,
    #[serde(default)]
    pub cursor_style: CursorStyle,
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub connection_groups: Vec<String>,
    #[serde(default)]
    pub deleted_sessions: Vec<DeletedSession>,
    #[serde(default)]
    pub deleted_connection_groups: Vec<DeletedConnectionGroup>,
    #[serde(default)]
    pub managed_keys: Vec<ManagedKey>,
    #[serde(default)]
    pub window_bounds: Option<SavedWindowBounds>,
    #[serde(default)]
    pub workspace_panels: Option<Vec<f32>>,
    #[serde(default)]
    pub body_panels: Option<Vec<f32>>,
    #[serde(default)]
    pub transfers: Vec<crate::terminal::Transfer>,
    #[serde(default)]
    pub show_hidden_files: bool,
    #[serde(default)]
    pub lock_layout: bool,
    #[serde(default = "default_monitoring_position")]
    pub monitoring_position: String,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default)]
    pub sftp_panel_minimized: bool,
    #[serde(default = "default_sftp_panel_view")]
    pub sftp_panel_view: String,
    #[serde(default)]
    pub sftp_toolbar_visibility: SftpToolbarVisibility,
    #[serde(default)]
    pub sftp_footer_visibility: SftpFooterVisibility,
    #[serde(default)]
    pub quick_command_categories: Option<Vec<QuickCommandCategory>>,
    #[serde(default)]
    pub sftp_external_editor: String,
    #[serde(default)]
    pub key_bindings: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub sync_endpoint: String,
    #[serde(default)]
    pub sync_username: String,
    #[serde(default)]
    pub sync_etag: Option<String>,
    #[serde(default)]
    pub sync_device_id: String,
    #[serde(default)]
    pub sync_backend: String,
    #[serde(default)]
    pub sync_enabled: bool,
    #[serde(default = "default_sync_interval_hours")]
    pub sync_interval_hours: u32,
    #[serde(default)]
    pub sync_last_synced_at: i64,
    #[serde(default)]
    pub sync_webdav_password_sealed: String,
    #[serde(default)]
    pub sync_etag_backend: String,
    #[serde(default)]
    pub sync_etag_target: String,
    /// 是否把会话密码/私钥等敏感信息一并同步（脱敏上传或字段级加密）。
    #[serde(default)]
    pub sync_include_secrets: bool,
    /// 隐私信息加密密码，经硬件 UUID 绑定加密后落盘。
    /// 换设备无法解出，需用户重新输入；丢失后只能本地重置覆盖云端。
    #[serde(default)]
    pub sync_secrets_password_sealed: String,
    /// 隐私信息加密密码的 Argon2id 哈希，用于校验输入一致性（不存明文）。
    #[serde(default)]
    pub sync_secrets_password_hash: String,
    #[serde(default)]
    pub sync_s3_endpoint: String,
    #[serde(default = "default_s3_region")]
    pub sync_s3_region: String,
    #[serde(default)]
    pub sync_s3_bucket: String,
    #[serde(default = "default_s3_object_key")]
    pub sync_s3_object_key: String,
    #[serde(default)]
    pub use_proxy: bool,
    #[serde(default = "default_read_env_proxy")]
    pub read_env_proxy: bool,
    #[serde(default = "default_global_proxy_type")]
    pub global_proxy_type: String,
    #[serde(default)]
    pub global_proxy_host: String,
    #[serde(default)]
    pub global_proxy_port: Option<u16>,
    #[serde(default)]
    pub global_proxy_user: String,
    #[serde(default)]
    pub global_proxy_password: String,
    #[serde(default)]
    pub update_check_mode: UpdateCheckMode,
    #[serde(default = "default_update_interval_hours")]
    pub update_interval_hours: u32,
    #[serde(default = "default_update_notify")]
    pub update_notify: bool,
    #[serde(default)]
    pub update_last_checked_at: i64,
    #[serde(default)]
    pub download_directory: String,
}

fn default_read_env_proxy() -> bool {
    true
}

fn default_global_proxy_type() -> String {
    "socks5".to_string()
}

fn default_monitoring_position() -> String {
    "Sidebar".to_string()
}

fn default_sftp_panel_view() -> String {
    "files".to_string()
}

fn default_s3_region() -> String {
    "us-east-1".to_string()
}

fn default_s3_object_key() -> String {
    "tiny-shell-sync.json".to_string()
}

fn webdav_sync_target(endpoint: &str, username: &str) -> String {
    format!(
        "webdav:{}:{}",
        endpoint.trim().trim_end_matches('/'),
        username.trim()
    )
}

fn s3_sync_target(endpoint: &str, region: &str, bucket: &str, object_key: &str) -> String {
    format!(
        "s3:{}:{}:{}:{}",
        endpoint.trim().trim_end_matches('/'),
        region.trim(),
        bucket.trim(),
        object_key.trim().trim_start_matches('/')
    )
}

fn default_follow_system_theme() -> bool {
    true
}

fn default_locale() -> String {
    "system".to_string()
}

fn default_terminal_font_size() -> f32 {
    14.0
}

fn default_ui_font_size() -> f32 {
    14.0
}

fn default_sync_interval_hours() -> u32 {
    24
}

fn default_update_interval_hours() -> u32 {
    24
}

fn default_update_notify() -> bool {
    true
}

pub fn default_ui_font_family() -> String {
    // ".SystemUIFont" is a GPUI sentinel that resolves to the platform system UI font.
    // This matches gpui-component's own Theme default.
    ".SystemUIFont".to_string()
}

fn default_terminal_font_family() -> String {
    "Maple Mono NF CN".to_string()
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            follow_system_theme: default_follow_system_theme(),
            theme_mode: String::new(),
            light_theme_name: String::new(),
            dark_theme_name: String::new(),
            locale: default_locale(),
            terminal_font_size: default_terminal_font_size(),
            terminal_display_style: TerminalDisplayStyle::default(),
            ui_font_size: default_ui_font_size(),
            keyword_highlight: false,
            ui_font_family: default_ui_font_family(),
            terminal_font_family: default_terminal_font_family(),
            title_bar_style: TitleBarStyle::default(),
            cursor_style: CursorStyle::default(),
            sessions: Vec::new(),
            connection_groups: Vec::new(),
            deleted_sessions: Vec::new(),
            deleted_connection_groups: Vec::new(),
            managed_keys: Vec::new(),
            window_bounds: None,
            workspace_panels: None,
            body_panels: None,
            transfers: Vec::new(),
            show_hidden_files: false,
            lock_layout: false,
            monitoring_position: default_monitoring_position(),
            sidebar_collapsed: false,
            sftp_panel_minimized: false,
            sftp_panel_view: default_sftp_panel_view(),
            sftp_toolbar_visibility: SftpToolbarVisibility::default(),
            sftp_footer_visibility: SftpFooterVisibility::default(),
            quick_command_categories: None,
            sftp_external_editor: String::new(),
            key_bindings: std::collections::HashMap::new(),
            sync_endpoint: String::new(),
            sync_username: String::new(),
            sync_etag: None,
            sync_device_id: String::new(),
            sync_backend: String::new(),
            sync_enabled: false,
            sync_interval_hours: default_sync_interval_hours(),
            sync_last_synced_at: 0,
            sync_webdav_password_sealed: String::new(),
            sync_etag_backend: String::new(),
            sync_etag_target: String::new(),
            sync_include_secrets: false,
            sync_secrets_password_sealed: String::new(),
            sync_secrets_password_hash: String::new(),
            sync_s3_endpoint: String::new(),
            sync_s3_region: default_s3_region(),
            sync_s3_bucket: String::new(),
            sync_s3_object_key: default_s3_object_key(),
            use_proxy: false,
            read_env_proxy: true,
            global_proxy_type: default_global_proxy_type(),
            global_proxy_host: String::new(),
            global_proxy_port: None,
            global_proxy_user: String::new(),
            global_proxy_password: String::new(),
            update_check_mode: UpdateCheckMode::default(),
            update_interval_hours: default_update_interval_hours(),
            update_notify: default_update_notify(),
            update_last_checked_at: 0,
            download_directory: String::new(),
        }
    }
}

#[derive(Clone)]
pub struct ConfigStore {
    path: PathBuf,
    cache: ConfigFile,
}

fn connection_catalog_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}

fn unique_connection_group_name(existing: &[String], requested: &str) -> String {
    if !existing.iter().any(|group| group == requested) {
        return requested.to_string();
    }
    for suffix in 2..u64::MAX {
        let candidate = format!("{requested} ({suffix})");
        if !existing.iter().any(|group| group == &candidate) {
            return candidate;
        }
    }
    format!("{requested} ({})", Uuid::new_v4())
}

#[allow(dead_code)]
impl ConfigStore {
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config dir {}", parent.display()))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(mut perms) = fs::metadata(parent).map(|m| m.permissions()) {
                    perms.set_mode(0o700);
                    let _ = fs::set_permissions(parent, perms);
                }
            }

            let tmp_dir = parent.join("tmp");
            let _ = fs::remove_dir_all(&tmp_dir);
            let _ = fs::create_dir_all(&tmp_dir);
        }

        let mut cache = if path.exists() {
            let raw_bytes =
                fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
            let hardware_uuid = get_hardware_uuid();
            match decrypt_config(&raw_bytes, &hardware_uuid) {
                Ok(cache) => cache,
                Err(decrypt_err) => {
                    // Fallback to plain text JSON if decryption/parsing failed
                    match serde_json::from_slice::<ConfigFile>(&raw_bytes) {
                        Ok(cache) => cache,
                        Err(json_err) => {
                            let backup_path = path.with_extension("json.bak");
                            if let Err(backup_err) = fs::write(&backup_path, &raw_bytes) {
                                tracing::warn!(
                                    "failed to parse config {} (decrypt err: {decrypt_err:#}, json err: {json_err:#}); backup to {} also failed: {backup_err:#}",
                                    path.display(),
                                    backup_path.display(),
                                );
                            } else {
                                tracing::warn!(
                                    "failed to parse config {} (decrypt err: {decrypt_err:#}, json err: {json_err:#}); backed up the original to {} and loaded defaults",
                                    path.display(),
                                    backup_path.display(),
                                );
                            }
                            ConfigFile::default()
                        }
                    }
                }
            }
        } else {
            ConfigFile::default()
        };

        if cache.sync_device_id.is_empty() {
            cache.sync_device_id = Uuid::new_v4().to_string();
        }
        Ok(Self { path, cache })
    }

    pub fn in_memory() -> Self {
        let cache = ConfigFile {
            sync_device_id: Uuid::new_v4().to_string(),
            ..ConfigFile::default()
        };
        Self {
            path: PathBuf::new(),
            cache,
        }
    }

    fn config_path() -> Result<PathBuf> {
        let dirs = BaseDirs::new().context("could not determine user home directory")?;
        Ok(dirs
            .home_dir()
            .join(".config")
            .join("tiny-shell")
            .join("sessions.json"))
    }

    pub fn sessions(&self) -> &[Session] {
        &self.cache.sessions
    }

    pub fn connection_groups(&self) -> &[String] {
        &self.cache.connection_groups
    }

    pub fn deleted_sessions(&self) -> &[DeletedSession] {
        &self.cache.deleted_sessions
    }

    pub fn deleted_connection_groups(&self) -> &[DeletedConnectionGroup] {
        &self.cache.deleted_connection_groups
    }

    pub fn soft_delete_session(&mut self, id: &str) -> bool {
        let Some(index) = self
            .cache
            .sessions
            .iter()
            .position(|session| session.id == id)
        else {
            return false;
        };
        let session = self.cache.sessions.remove(index);
        let tombstone = DeletedSession {
            session,
            deleted_at: connection_catalog_timestamp(),
        };
        self.cache
            .deleted_sessions
            .retain(|item| item.session.id != id);
        self.cache.deleted_sessions.push(tombstone);
        true
    }

    pub fn soft_delete_connection_group(&mut self, name: &str) -> bool {
        if !self
            .cache
            .connection_groups
            .iter()
            .any(|group| group == name)
        {
            return false;
        }
        let prefix = format!("{name}/");
        let groups = self
            .cache
            .connection_groups
            .iter()
            .filter(|group| *group == name || group.starts_with(&prefix))
            .cloned()
            .collect();
        let sessions = self
            .cache
            .sessions
            .iter()
            .filter(|session| {
                session
                    .group
                    .as_deref()
                    .is_some_and(|group| group == name || group.starts_with(&prefix))
            })
            .cloned()
            .collect();
        self.cache
            .connection_groups
            .retain(|group| group != name && !group.starts_with(&prefix));
        self.cache.sessions.retain(|session| {
            !session
                .group
                .as_deref()
                .is_some_and(|group| group == name || group.starts_with(&prefix))
        });
        self.cache
            .deleted_connection_groups
            .retain(|item| item.name != name);
        self.cache
            .deleted_connection_groups
            .push(DeletedConnectionGroup {
                name: name.to_string(),
                groups,
                sessions,
                deleted_at: connection_catalog_timestamp(),
            });
        true
    }

    pub fn restore_deleted_session(&mut self, id: &str) -> bool {
        let Some(index) = self
            .cache
            .deleted_sessions
            .iter()
            .position(|item| item.session.id == id)
        else {
            return false;
        };
        let tombstone = self.cache.deleted_sessions.remove(index);
        if self.cache.sessions.iter().any(|session| session.id == id) {
            return false;
        }
        if let Some(group) = tombstone.session.group.as_deref() {
            self.ensure_connection_group_path(group);
        }
        self.cache.sessions.push(tombstone.session);
        true
    }

    pub fn restore_deleted_connection_group(&mut self, name: &str) -> bool {
        let Some(index) = self
            .cache
            .deleted_connection_groups
            .iter()
            .position(|item| item.name == name)
        else {
            return false;
        };
        let tombstone = self.cache.deleted_connection_groups.remove(index);
        let restored_name =
            unique_connection_group_name(&self.cache.connection_groups, &tombstone.name);
        let old_prefix = format!("{}/", tombstone.name);
        let new_prefix = format!("{}/", restored_name);
        for group in tombstone.groups {
            let restored_group = if group == tombstone.name {
                restored_name.clone()
            } else if let Some(suffix) = group.strip_prefix(&old_prefix) {
                format!("{new_prefix}{suffix}")
            } else {
                group
            };
            self.add_connection_group(restored_group);
        }
        for mut session in tombstone.sessions {
            if let Some(group) = session.group.as_deref() {
                session.group = Some(if group == tombstone.name {
                    restored_name.clone()
                } else if let Some(suffix) = group.strip_prefix(&old_prefix) {
                    format!("{new_prefix}{suffix}")
                } else {
                    group.to_string()
                });
            }
            if !self.cache.sessions.iter().any(|item| item.id == session.id) {
                self.cache.sessions.push(session);
            }
        }
        true
    }

    pub fn purge_deleted_session(&mut self, id: &str) -> bool {
        let before = self.cache.deleted_sessions.len();
        self.cache
            .deleted_sessions
            .retain(|item| item.session.id != id);
        before != self.cache.deleted_sessions.len()
    }

    fn ensure_connection_group_path(&mut self, group: &str) {
        let mut path = String::new();
        for segment in group.split('/') {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(segment);
            self.add_connection_group(path.clone());
        }
    }

    pub fn add_connection_group(&mut self, name: String) {
        if !name.trim().is_empty()
            && !self
                .cache
                .connection_groups
                .iter()
                .any(|group| group == &name)
        {
            self.cache.connection_groups.push(name);
        }
    }

    pub fn rename_connection_group(&mut self, old_name: &str, new_name: String) {
        if old_name == new_name || new_name.trim().is_empty() {
            return;
        }
        if self
            .cache
            .connection_groups
            .iter()
            .any(|group| group == &new_name)
        {
            return;
        }
        let old_prefix = format!("{old_name}/");
        let new_prefix = format!("{new_name}/");
        for group in &mut self.cache.connection_groups {
            if group == old_name {
                *group = new_name.clone();
            } else if let Some(suffix) = group.strip_prefix(&old_prefix) {
                *group = format!("{new_prefix}{suffix}");
            }
        }
        for session in &mut self.cache.sessions {
            if session.group.as_deref() == Some(old_name) {
                session.group = Some(new_name.clone());
            } else if let Some(suffix) = session
                .group
                .as_deref()
                .and_then(|group| group.strip_prefix(&old_prefix))
            {
                session.group = Some(format!("{new_prefix}{suffix}"));
            }
        }
    }

    /// Move a group (and all of its descendants) before another group at the
    /// same tree level. `None` places it at the end of its siblings.
    pub fn reorder_connection_group(&mut self, name: &str, before: Option<&str>) {
        let prefix = format!("{name}/");
        let parent = name.rsplit_once('/').map(|(parent, _)| parent);
        let same_parent = |group: &str| group.rsplit_once('/').map(|(parent, _)| parent) == parent;

        if !self
            .cache
            .connection_groups
            .iter()
            .any(|group| group == name)
            || before.is_some_and(|target| {
                target == name || target.starts_with(&prefix) || !same_parent(target)
            })
        {
            return;
        }

        let mut moving = Vec::new();
        self.cache.connection_groups.retain(|group| {
            if group == name || group.starts_with(&prefix) {
                moving.push(group.clone());
                false
            } else {
                true
            }
        });
        let insert_at = before
            .and_then(|target| {
                self.cache
                    .connection_groups
                    .iter()
                    .position(|group| group == target)
            })
            .unwrap_or_else(|| {
                self.cache
                    .connection_groups
                    .iter()
                    .rposition(|group| same_parent(group))
                    .map(|index| index + 1)
                    .unwrap_or(self.cache.connection_groups.len())
            });
        self.cache
            .connection_groups
            .splice(insert_at..insert_at, moving);
    }

    pub fn remove_connection_group(&mut self, name: &str) {
        let prefix = format!("{name}/");
        self.cache
            .connection_groups
            .retain(|group| group != name && !group.starts_with(&prefix));
        for session in &mut self.cache.sessions {
            if session
                .group
                .as_deref()
                .is_some_and(|group| group == name || group.starts_with(&prefix))
            {
                session.group = None;
            }
        }
    }

    pub fn move_connection_group(&mut self, name: &str, new_parent: Option<&str>) {
        let leaf = name.rsplit('/').next().unwrap_or(name);
        let destination = new_parent
            .filter(|parent| !parent.is_empty())
            .map(|parent| format!("{parent}/{leaf}"))
            .unwrap_or_else(|| leaf.to_string());
        if destination == name
            || destination.starts_with(&format!("{name}/"))
            || self
                .cache
                .connection_groups
                .iter()
                .any(|group| group == &destination)
        {
            return;
        }
        self.rename_connection_group(name, destination);
    }

    pub fn replace_sessions(&mut self, sessions: Vec<Session>) {
        self.cache.sessions = sessions;
    }

    pub fn replace_deleted_sessions(&mut self, sessions: Vec<DeletedSession>) {
        self.cache.deleted_sessions = sessions;
    }

    pub fn replace_connection_groups(&mut self, groups: Vec<String>) {
        self.cache.connection_groups = groups;
    }

    pub fn replace_deleted_connection_groups(&mut self, groups: Vec<DeletedConnectionGroup>) {
        self.cache.deleted_connection_groups = groups;
    }

    pub fn replace_managed_keys(&mut self, keys: Vec<ManagedKey>) {
        self.cache.managed_keys = keys;
    }

    pub fn sync_endpoint(&self) -> &str {
        &self.cache.sync_endpoint
    }

    pub fn sync_username(&self) -> &str {
        &self.cache.sync_username
    }

    pub fn sync_target_id(&self) -> String {
        if self.sync_backend() == "s3" {
            s3_sync_target(
                self.sync_s3_endpoint(),
                self.sync_s3_region(),
                self.sync_s3_bucket(),
                self.sync_s3_object_key(),
            )
        } else {
            webdav_sync_target(self.sync_endpoint(), self.sync_username())
        }
    }

    pub fn sync_device_id(&self) -> &str {
        &self.cache.sync_device_id
    }

    pub fn sync_backend(&self) -> &str {
        if self.cache.sync_backend == "s3" {
            "s3"
        } else {
            "webdav"
        }
    }

    pub fn set_sync_backend(&mut self, backend: &str) {
        self.cache.sync_backend = if backend == "s3" { "s3" } else { "webdav" }.to_string();
    }

    pub fn sync_enabled(&self) -> bool {
        self.cache.sync_enabled
    }

    pub fn set_sync_enabled(&mut self, enabled: bool) {
        self.cache.sync_enabled = enabled;
    }

    pub fn sync_interval_hours(&self) -> u32 {
        self.cache.sync_interval_hours.clamp(1, 8_760)
    }

    pub fn set_sync_interval_hours(&mut self, hours: u32) {
        self.cache.sync_interval_hours = hours.clamp(1, 8_760);
    }

    pub fn sync_last_synced_at(&self) -> i64 {
        self.cache.sync_last_synced_at
    }

    pub fn set_sync_last_synced_at(&mut self, timestamp: i64) {
        self.cache.sync_last_synced_at = timestamp.max(0);
    }

    pub fn sync_next_at(&self) -> i64 {
        if self.sync_last_synced_at() <= 0 {
            0
        } else {
            self.sync_last_synced_at()
                .saturating_add(i64::from(self.sync_interval_hours()).saturating_mul(3_600))
        }
    }

    pub fn sync_webdav_password_sealed(&self) -> &str {
        &self.cache.sync_webdav_password_sealed
    }

    pub fn set_sync_webdav_password_sealed(&mut self, password: String) {
        self.cache.sync_webdav_password_sealed = password;
    }

    pub fn sync_s3_endpoint(&self) -> &str {
        &self.cache.sync_s3_endpoint
    }

    pub fn sync_s3_region(&self) -> &str {
        if self.cache.sync_s3_region.is_empty() {
            "us-east-1"
        } else {
            &self.cache.sync_s3_region
        }
    }

    pub fn sync_s3_bucket(&self) -> &str {
        &self.cache.sync_s3_bucket
    }

    pub fn sync_s3_object_key(&self) -> &str {
        if self.cache.sync_s3_object_key.is_empty() {
            "tiny-shell-sync.json"
        } else {
            &self.cache.sync_s3_object_key
        }
    }

    pub fn set_sync_connection(&mut self, endpoint: String, username: String) {
        let target = webdav_sync_target(&endpoint, &username);
        if self.cache.sync_etag_target != target {
            self.cache.sync_etag = None;
        }
        self.cache.sync_endpoint = endpoint;
        self.cache.sync_username = username;
    }

    pub fn set_sync_s3_connection(
        &mut self,
        endpoint: String,
        region: String,
        bucket: String,
        object_key: String,
    ) {
        let target = s3_sync_target(&endpoint, &region, &bucket, &object_key);
        if self.cache.sync_etag_target != target {
            self.cache.sync_etag = None;
        }
        self.cache.sync_s3_endpoint = endpoint;
        self.cache.sync_s3_region = region;
        self.cache.sync_s3_bucket = bucket;
        self.cache.sync_s3_object_key = object_key;
    }

    pub fn set_sync_etag(&mut self, etag: Option<String>) {
        self.cache.sync_etag = etag;
        self.cache.sync_etag_backend = self.sync_backend().to_string();
        self.cache.sync_etag_target = self.sync_target_id();
    }

    pub fn sync_include_secrets(&self) -> bool {
        self.cache.sync_include_secrets
    }

    pub fn set_sync_include_secrets(&mut self, include: bool) {
        self.cache.sync_include_secrets = include;
    }

    pub fn sync_secrets_password_sealed(&self) -> &str {
        &self.cache.sync_secrets_password_sealed
    }

    pub fn set_sync_secrets_password_sealed(&mut self, sealed: String) {
        self.cache.sync_secrets_password_sealed = sealed;
    }

    pub fn set_sync_secrets_password_hash(&mut self, hash: String) {
        self.cache.sync_secrets_password_hash = hash;
    }

    pub fn tmp_dir(&self) -> Option<PathBuf> {
        self.path.parent().map(|p| p.join("tmp"))
    }

    pub fn follow_system_theme(&self) -> bool {
        self.cache.follow_system_theme
    }

    pub fn theme_mode(&self) -> &str {
        &self.cache.theme_mode
    }

    pub fn light_theme_name(&self) -> &str {
        &self.cache.light_theme_name
    }

    pub fn dark_theme_name(&self) -> &str {
        &self.cache.dark_theme_name
    }

    pub fn locale(&self) -> &str {
        if self.cache.locale.is_empty() {
            "system"
        } else {
            &self.cache.locale
        }
    }

    pub fn set_locale(&mut self, locale: &str) {
        self.cache.locale = locale.to_string();
    }

    pub fn key_bindings(&self) -> &std::collections::HashMap<String, String> {
        &self.cache.key_bindings
    }

    pub fn set_key_binding(&mut self, action_name: &str, keystroke: &str) {
        self.cache
            .key_bindings
            .insert(action_name.to_string(), keystroke.to_string());
    }

    pub fn monitoring_position(&self) -> &str {
        if self.cache.monitoring_position.is_empty() {
            "Sidebar"
        } else {
            &self.cache.monitoring_position
        }
    }

    pub fn set_monitoring_position(&mut self, pos: &str) {
        self.cache.monitoring_position = pos.to_string();
    }

    pub fn terminal_font_size(&self) -> f32 {
        if self.cache.terminal_font_size <= 0.0 {
            default_terminal_font_size()
        } else {
            self.cache.terminal_font_size
        }
    }

    pub fn terminal_display_style(&self) -> TerminalDisplayStyle {
        self.cache.terminal_display_style
    }

    pub fn set_terminal_display_style(&mut self, style: TerminalDisplayStyle) {
        self.cache.terminal_display_style = style;
    }

    pub fn set_theme_preferences(
        &mut self,
        follow_system_theme: bool,
        theme_mode: impl Into<String>,
        light_theme_name: impl Into<String>,
        dark_theme_name: impl Into<String>,
    ) {
        self.cache.follow_system_theme = follow_system_theme;
        self.cache.theme_mode = theme_mode.into();
        self.cache.light_theme_name = light_theme_name.into();
        self.cache.dark_theme_name = dark_theme_name.into();
    }

    pub fn window_bounds(&self) -> Option<&SavedWindowBounds> {
        self.cache.window_bounds.as_ref()
    }

    pub fn workspace_panels(&self) -> Option<&Vec<f32>> {
        self.cache.workspace_panels.as_ref()
    }

    #[allow(dead_code)]
    pub fn body_panels(&self) -> Option<&Vec<f32>> {
        self.cache.body_panels.as_ref()
    }

    pub fn transfers(&self) -> Vec<crate::terminal::Transfer> {
        self.cache.transfers.clone()
    }

    pub fn set_transfers(&mut self, transfers: Vec<crate::terminal::Transfer>) {
        self.cache.transfers = transfers;
        if let Err(err) = self.save() {
            tracing::error!("failed to save config: {err:#}");
        }
    }

    pub fn set_layout_state(
        &mut self,
        window_bounds: Option<SavedWindowBounds>,
        workspace_panels: Option<Vec<f32>>,
        body_panels: Option<Vec<f32>>,
    ) {
        self.cache.window_bounds = window_bounds;
        self.cache.workspace_panels = workspace_panels;
        self.cache.body_panels = body_panels;
    }

    pub fn set_terminal_font_size(&mut self, terminal_font_size: f32) {
        self.cache.terminal_font_size = terminal_font_size.max(10.0);
    }

    pub fn ui_font_size(&self) -> f32 {
        if self.cache.ui_font_size <= 0.0 {
            default_ui_font_size()
        } else {
            self.cache.ui_font_size
        }
    }

    pub fn set_ui_font_size(&mut self, ui_font_size: f32) {
        self.cache.ui_font_size = ui_font_size.max(8.0);
    }

    pub fn ui_font_family(&self) -> &str {
        if self.cache.ui_font_family.is_empty() {
            ".SystemUIFont"
        } else {
            &self.cache.ui_font_family
        }
    }

    pub fn set_ui_font_family(&mut self, family: &str) {
        self.cache.ui_font_family = family.to_string();
    }

    pub fn keyword_highlight(&self) -> bool {
        self.cache.keyword_highlight
    }

    pub fn set_keyword_highlight(&mut self, val: bool) {
        self.cache.keyword_highlight = val;
    }

    pub fn terminal_font_family(&self) -> &str {
        if self.cache.terminal_font_family.is_empty() {
            "Maple Mono NF CN"
        } else {
            &self.cache.terminal_font_family
        }
    }

    pub fn set_terminal_font_family(&mut self, family: &str) {
        self.cache.terminal_font_family = family.to_string();
    }

    pub fn title_bar_style(&self) -> TitleBarStyle {
        self.cache.title_bar_style
    }

    pub fn set_title_bar_style(&mut self, style: TitleBarStyle) {
        self.cache.title_bar_style = style;
    }

    pub fn cursor_style(&self) -> CursorStyle {
        self.cache.cursor_style
    }

    pub fn set_cursor_style(&mut self, style: CursorStyle) {
        self.cache.cursor_style = style;
    }

    pub fn use_proxy(&self) -> bool {
        self.cache.use_proxy
    }
    pub fn set_use_proxy(&mut self, val: bool) {
        self.cache.use_proxy = val;
    }
    pub fn read_env_proxy(&self) -> bool {
        self.cache.read_env_proxy
    }
    pub fn set_read_env_proxy(&mut self, val: bool) {
        self.cache.read_env_proxy = val;
    }
    pub fn global_proxy_type(&self) -> &str {
        &self.cache.global_proxy_type
    }
    pub fn set_global_proxy_type(&mut self, val: String) {
        self.cache.global_proxy_type = val;
    }
    pub fn global_proxy_host(&self) -> &str {
        &self.cache.global_proxy_host
    }
    pub fn set_global_proxy_host(&mut self, val: String) {
        self.cache.global_proxy_host = val;
    }
    pub fn global_proxy_port(&self) -> Option<u16> {
        self.cache.global_proxy_port
    }
    pub fn set_global_proxy_port(&mut self, val: Option<u16>) {
        self.cache.global_proxy_port = val;
    }
    pub fn global_proxy_user(&self) -> &str {
        &self.cache.global_proxy_user
    }
    pub fn set_global_proxy_user(&mut self, val: String) {
        self.cache.global_proxy_user = val;
    }
    pub fn global_proxy_password(&self) -> &str {
        &self.cache.global_proxy_password
    }
    pub fn set_global_proxy_password(&mut self, val: String) {
        self.cache.global_proxy_password = val;
    }

    pub fn update_check_mode(&self) -> UpdateCheckMode {
        self.cache.update_check_mode
    }

    pub fn set_update_check_mode(&mut self, mode: UpdateCheckMode) {
        self.cache.update_check_mode = mode;
    }

    pub fn update_interval_hours(&self) -> u32 {
        self.cache.update_interval_hours.clamp(1, 8_760)
    }

    pub fn set_update_interval_hours(&mut self, hours: u32) {
        self.cache.update_interval_hours = hours.clamp(1, 8_760);
    }

    pub fn update_notify(&self) -> bool {
        self.cache.update_notify
    }

    pub fn set_update_notify(&mut self, notify: bool) {
        self.cache.update_notify = notify;
    }

    pub fn update_last_checked_at(&self) -> i64 {
        self.cache.update_last_checked_at
    }

    pub fn set_update_last_checked_at(&mut self, timestamp: i64) {
        self.cache.update_last_checked_at = timestamp.max(0);
    }

    pub fn download_directory(&self) -> Option<PathBuf> {
        let path = self.cache.download_directory.trim();
        (!path.is_empty()).then(|| PathBuf::from(path))
    }

    pub fn set_download_directory(&mut self, path: Option<&Path>) {
        self.cache.download_directory = path
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
    }

    pub fn lock_layout(&self) -> bool {
        self.cache.lock_layout
    }

    pub fn set_lock_layout(&mut self, val: bool) {
        self.cache.lock_layout = val;
    }

    pub fn sidebar_collapsed(&self) -> bool {
        self.cache.sidebar_collapsed
    }

    pub fn set_sidebar_collapsed(&mut self, val: bool) {
        self.cache.sidebar_collapsed = val;
    }

    pub fn sftp_panel_minimized(&self) -> bool {
        self.cache.sftp_panel_minimized
    }

    pub fn set_sftp_panel_minimized(&mut self, val: bool) {
        self.cache.sftp_panel_minimized = val;
    }

    pub fn sftp_panel_view(&self) -> &str {
        &self.cache.sftp_panel_view
    }

    pub fn set_sftp_panel_view(&mut self, view: &str) {
        self.cache.sftp_panel_view = view.to_string();
    }

    pub fn sftp_toolbar_visibility(&self) -> SftpToolbarVisibility {
        self.cache.sftp_toolbar_visibility
    }

    pub fn set_sftp_toolbar_visibility(&mut self, visibility: SftpToolbarVisibility) {
        self.cache.sftp_toolbar_visibility = visibility;
    }

    pub fn sftp_footer_visibility(&self) -> SftpFooterVisibility {
        self.cache.sftp_footer_visibility
    }

    pub fn set_sftp_footer_visibility(&mut self, visibility: SftpFooterVisibility) {
        self.cache.sftp_footer_visibility = visibility;
    }

    pub fn quick_command_categories(&self) -> Option<&[QuickCommandCategory]> {
        self.cache.quick_command_categories.as_deref()
    }

    pub fn set_quick_command_categories(&mut self, categories: Vec<QuickCommandCategory>) {
        self.cache.quick_command_categories = Some(categories);
    }

    pub fn upsert_quick_command_category(&mut self, category: QuickCommandCategory) {
        let categories = self
            .cache
            .quick_command_categories
            .get_or_insert_with(Vec::new);
        if let Some(existing) = categories.iter_mut().find(|item| item.id == category.id) {
            *existing = category;
        } else {
            categories.push(category);
        }
    }

    pub fn remove_quick_command_category(&mut self, category_id: &str) {
        self.cache
            .quick_command_categories
            .get_or_insert_with(Vec::new)
            .retain(|category| category.id != category_id);
    }

    pub fn upsert_quick_command(&mut self, category_id: &str, command: QuickCommand) {
        let Some(category) = self
            .cache
            .quick_command_categories
            .get_or_insert_with(Vec::new)
            .iter_mut()
            .find(|category| category.id == category_id)
        else {
            return;
        };
        if let Some(existing) = category
            .commands
            .iter_mut()
            .find(|item| item.id == command.id)
        {
            *existing = command;
        } else {
            category.commands.push(command);
        }
    }

    pub fn remove_quick_command(&mut self, category_id: &str, command_id: &str) {
        if let Some(category) = self
            .cache
            .quick_command_categories
            .get_or_insert_with(Vec::new)
            .iter_mut()
            .find(|category| category.id == category_id)
        {
            category.commands.retain(|command| command.id != command_id);
        }
    }

    pub fn move_quick_command(
        &mut self,
        source_category_id: &str,
        target_category_id: &str,
        command_id: &str,
    ) {
        if source_category_id == target_category_id {
            return;
        }
        let Some(categories) = self.cache.quick_command_categories.as_mut() else {
            return;
        };
        let Some(command) = categories
            .iter_mut()
            .find(|category| category.id == source_category_id)
            .and_then(|category| {
                category
                    .commands
                    .iter()
                    .position(|command| command.id == command_id)
                    .map(|index| category.commands.remove(index))
            })
        else {
            return;
        };
        if let Some(category) = categories
            .iter_mut()
            .find(|category| category.id == target_category_id)
        {
            category.commands.push(command);
        }
    }

    pub fn sftp_external_editor(&self) -> &str {
        &self.cache.sftp_external_editor
    }

    pub fn set_sftp_external_editor(&mut self, value: String) {
        self.cache.sftp_external_editor = value;
    }

    pub fn show_hidden_files(&self) -> bool {
        self.cache.show_hidden_files
    }

    pub fn set_show_hidden_files(&mut self, val: bool) {
        self.cache.show_hidden_files = val;
    }

    pub fn merge_interactive_preferences_from(&mut self, source: &ConfigStore) {
        self.cache.follow_system_theme = source.cache.follow_system_theme;
        self.cache.theme_mode = source.cache.theme_mode.clone();
        self.cache.light_theme_name = source.cache.light_theme_name.clone();
        self.cache.dark_theme_name = source.cache.dark_theme_name.clone();
        self.cache.locale = source.cache.locale.clone();
        self.cache.terminal_font_size = source.cache.terminal_font_size;
        self.cache.terminal_display_style = source.cache.terminal_display_style;
        self.cache.ui_font_size = source.cache.ui_font_size;
        self.cache.keyword_highlight = source.cache.keyword_highlight;
        self.cache.ui_font_family = source.cache.ui_font_family.clone();
        self.cache.terminal_font_family = source.cache.terminal_font_family.clone();
        self.cache.title_bar_style = source.cache.title_bar_style;
        self.cache.cursor_style = source.cache.cursor_style;
        self.cache.show_hidden_files = source.cache.show_hidden_files;
        self.cache.lock_layout = source.cache.lock_layout;
        self.cache.monitoring_position = source.cache.monitoring_position.clone();
        self.cache.sidebar_collapsed = source.cache.sidebar_collapsed;
        self.cache.sftp_panel_minimized = source.cache.sftp_panel_minimized;
        self.cache.sftp_panel_view = source.cache.sftp_panel_view.clone();
        self.cache.sftp_toolbar_visibility = source.cache.sftp_toolbar_visibility;
        self.cache.sftp_footer_visibility = source.cache.sftp_footer_visibility;
        self.cache.quick_command_categories = source.cache.quick_command_categories.clone();
        self.cache.sftp_external_editor = source.cache.sftp_external_editor.clone();
        self.cache.key_bindings = source.cache.key_bindings.clone();
        self.cache.use_proxy = source.cache.use_proxy;
        self.cache.read_env_proxy = source.cache.read_env_proxy;
        self.cache.global_proxy_type = source.cache.global_proxy_type.clone();
        self.cache.global_proxy_host = source.cache.global_proxy_host.clone();
        self.cache.global_proxy_port = source.cache.global_proxy_port;
        self.cache.global_proxy_user = source.cache.global_proxy_user.clone();
        self.cache.global_proxy_password = source.cache.global_proxy_password.clone();
        self.cache.sync_enabled = source.cache.sync_enabled;
        self.cache.sync_interval_hours = source.cache.sync_interval_hours;
        self.cache.sync_last_synced_at = source.cache.sync_last_synced_at;
        self.cache.sync_webdav_password_sealed = source.cache.sync_webdav_password_sealed.clone();
        self.cache.update_check_mode = source.cache.update_check_mode;
        self.cache.update_interval_hours = source.cache.update_interval_hours;
        self.cache.update_notify = source.cache.update_notify;
        self.cache.update_last_checked_at = source.cache.update_last_checked_at;
        self.cache.download_directory = source.cache.download_directory.clone();
    }

    pub fn get(&self, id: &str) -> Option<&Session> {
        self.cache.sessions.iter().find(|s| s.id == id)
    }

    pub fn upsert(&mut self, session: Session) {
        if let Some(existing) = self.cache.sessions.iter_mut().find(|s| s.id == session.id) {
            *existing = session;
        } else {
            self.cache.sessions.push(session);
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.cache.sessions.retain(|s| s.id != id);
    }

    // ── Managed keys CRUD ──────────────────────────────────────────

    pub fn managed_keys(&self) -> &[ManagedKey] {
        &self.cache.managed_keys
    }

    pub fn get_managed_key(&self, id: &str) -> Option<&ManagedKey> {
        self.cache.managed_keys.iter().find(|k| k.id == id)
    }

    pub fn upsert_managed_key(&mut self, key: ManagedKey) {
        if let Some(existing) = self.cache.managed_keys.iter_mut().find(|k| k.id == key.id) {
            *existing = key;
        } else {
            self.cache.managed_keys.push(key);
        }
    }

    pub fn remove_managed_key(&mut self, id: &str) {
        self.cache.managed_keys.retain(|k| k.id != id);
        // Also clear the reference from any session that used this key.
        for s in &mut self.cache.sessions {
            if s.managed_key_id.as_deref() == Some(id) {
                s.managed_key_id = None;
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        let hardware_uuid = get_hardware_uuid();
        let encrypted_bytes = encrypt_config(&self.cache, &hardware_uuid)?;
        write_config_atomically(&self.path, &encrypted_bytes)
    }
}

fn write_config_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("configuration path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create config dir {}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("configuration path has an invalid file name")?;
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));

    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .with_context(|| format!("failed to create {}", temp_path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", temp_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600))
                .with_context(|| format!("failed to secure {}", temp_path.display()))?;
        }

        fs::rename(&temp_path, path).with_context(|| {
            format!(
                "failed to replace configuration {} with {}",
                path.display(),
                temp_path.display()
            )
        })?;

        #[cfg(unix)]
        OpenOptions::new()
            .read(true)
            .open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("failed to sync config dir {}", parent.display()))?;

        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

pub trait ProxyStream:
    tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static
{
}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static> ProxyStream
    for T
{
}

use std::sync::OnceLock;

#[derive(Clone)]
pub struct EnvProxy {
    pub proxy_type: String,
    pub host: String,
    pub port: Option<u16>,
    pub user: String,
    pub pass: String,
}

impl fmt::Debug for EnvProxy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvProxy")
            .field("proxy_type", &self.proxy_type)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("pass", &"<redacted>")
            .finish()
    }
}

pub static ENV_PROXY: OnceLock<Option<EnvProxy>> = OnceLock::new();

pub async fn connect_proxy(session: &Session) -> Result<Box<dyn ProxyStream>> {
    let target_host = session.host.clone();
    let target_port = session.port;
    let session = session.clone();

    let connect_fut = async move {
        let target_host = &target_host;
        let config = ConfigStore::load().unwrap_or_else(|_| ConfigStore::in_memory());
        let (proxy_type, proxy_host, proxy_port, proxy_user, proxy_password) = {
            if !session.proxy_type.is_empty() && session.proxy_type != "none" {
                (
                    session.proxy_type.clone(),
                    session.proxy_host.clone(),
                    session.proxy_port,
                    session.proxy_user.clone(),
                    session.proxy_password.clone(),
                )
            } else if config.cache.read_env_proxy
                && ENV_PROXY.get().and_then(|opt| opt.as_ref()).is_some()
            {
                let env_p = ENV_PROXY.get().and_then(|opt| opt.as_ref()).unwrap();
                (
                    env_p.proxy_type.clone(),
                    env_p.host.clone(),
                    env_p.port,
                    env_p.user.clone(),
                    env_p.pass.clone(),
                )
            } else if config.cache.use_proxy {
                (
                    config.cache.global_proxy_type.clone(),
                    config.cache.global_proxy_host.clone(),
                    config.cache.global_proxy_port,
                    config.cache.global_proxy_user.clone(),
                    config.cache.global_proxy_password.clone(),
                )
            } else {
                (
                    "none".to_string(),
                    String::new(),
                    None,
                    String::new(),
                    String::new(),
                )
            }
        };

        if proxy_type != "none" && (proxy_host.is_empty() || proxy_port.is_none()) {
            let addr = format!("{}:{}", target_host, target_port);
            let stream = tokio::net::TcpStream::connect(&addr).await?;
            return Ok(Box::new(stream) as Box<dyn ProxyStream>);
        }

        match proxy_type.as_str() {
            "socks5" | "socks5h" => {
                let proxy_port = proxy_port.unwrap_or(1080);
                let proxy_addr = format!("{}:{}", proxy_host, proxy_port);

                if !proxy_user.is_empty() {
                    let stream = tokio_socks::tcp::Socks5Stream::connect_with_password(
                        proxy_addr.as_str(),
                        (target_host.as_str(), target_port),
                        &proxy_user,
                        &proxy_password,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("SOCKS5 proxy connection failed: {}", e))?;
                    Ok(Box::new(stream) as Box<dyn ProxyStream>)
                } else {
                    let stream = tokio_socks::tcp::Socks5Stream::connect(
                        proxy_addr.as_str(),
                        (target_host.as_str(), target_port),
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("SOCKS5 proxy connection failed: {}", e))?;
                    Ok(Box::new(stream) as Box<dyn ProxyStream>)
                }
            }
            "http" => {
                let proxy_port = proxy_port.unwrap_or(8080);
                let proxy_addr = format!("{}:{}", proxy_host, proxy_port);

                use tokio::io::AsyncWriteExt;
                let mut stream = tokio::net::TcpStream::connect(&proxy_addr)
                    .await
                    .map_err(|e| anyhow::anyhow!("HTTP proxy connection failed: {}", e))?;

                let mut request = format!(
                    "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n",
                    target_host, target_port, target_host, target_port
                );
                if !proxy_user.is_empty() {
                    use base64::Engine as _;
                    let auth = format!("{}:{}", proxy_user, proxy_password);
                    let encoded = base64::engine::general_purpose::STANDARD.encode(auth);
                    request.push_str(&format!("Proxy-Authorization: Basic {}\r\n", encoded));
                }
                request.push_str("\r\n");

                stream.write_all(request.as_bytes()).await?;

                let mut response = [0u8; 1024];
                let n = tokio::io::AsyncReadExt::read(&mut stream, &mut response).await?;
                let resp_str = String::from_utf8_lossy(&response[..n]);
                if !resp_str.contains("200") && !resp_str.contains("established") {
                    return Err(anyhow::anyhow!("HTTP proxy CONNECT failed: {}", resp_str));
                }

                Ok(Box::new(stream) as Box<dyn ProxyStream>)
            }
            _ => {
                let addr = format!("{}:{}", target_host, target_port);
                let stream = tokio::net::TcpStream::connect(&addr).await?;
                Ok(Box::new(stream) as Box<dyn ProxyStream>)
            }
        }
    };

    tokio::time::timeout(std::time::Duration::from_secs(16), connect_fut)
        .await
        .map_err(|_| anyhow::anyhow!("connection timed out after 16 seconds"))?
}

pub fn active_proxy(session: &Session) -> Option<(String, String, Option<u16>)> {
    let config = ConfigStore::load().unwrap_or_else(|_| ConfigStore::in_memory());
    let (proxy_type, proxy_host, proxy_port, _, _) = {
        if !session.proxy_type.is_empty() && session.proxy_type != "none" {
            (
                session.proxy_type.clone(),
                session.proxy_host.clone(),
                session.proxy_port,
                session.proxy_user.clone(),
                session.proxy_password.clone(),
            )
        } else if config.cache.read_env_proxy
            && ENV_PROXY.get().and_then(|opt| opt.as_ref()).is_some()
        {
            let env_p = ENV_PROXY.get().and_then(|opt| opt.as_ref()).unwrap();
            (
                env_p.proxy_type.clone(),
                env_p.host.clone(),
                env_p.port,
                env_p.user.clone(),
                env_p.pass.clone(),
            )
        } else if config.cache.use_proxy {
            (
                config.cache.global_proxy_type.clone(),
                config.cache.global_proxy_host.clone(),
                config.cache.global_proxy_port,
                config.cache.global_proxy_user.clone(),
                config.cache.global_proxy_password.clone(),
            )
        } else {
            (
                "none".to_string(),
                String::new(),
                None,
                String::new(),
                String::new(),
            )
        }
    };

    if proxy_type != "none" && !proxy_host.is_empty() && proxy_port.is_some() {
        Some((proxy_type, proxy_host, proxy_port))
    } else {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedConfigEnvelope {
    format_version: u32,
    kdf: String,
    cipher: String,
    salt: String,
    nonce: String,
    payload: String,
}

static HARDWARE_UUID: OnceLock<String> = OnceLock::new();

fn get_hardware_uuid() -> String {
    HARDWARE_UUID.get_or_init(query_hardware_uuid).clone()
}

/// 对外暴露的硬件 UUID，供 `crypto` 模块做硬件绑定加密使用。
pub(crate) fn hardware_uuid() -> String {
    get_hardware_uuid()
}

fn query_hardware_uuid() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("IOPlatformUUID") {
                    if let Some(uuid) = line.split('"').nth(3) {
                        let uuid = uuid.trim().to_string();
                        if !uuid.is_empty() {
                            return uuid;
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(uuid) = std::fs::read_to_string("/sys/class/dmi/id/product_uuid") {
            let uuid = uuid.trim().to_string();
            if !uuid.is_empty() {
                return uuid;
            }
        }
        if let Ok(id) = std::fs::read_to_string("/etc/machine-id") {
            let id = id.trim().to_string();
            if !id.is_empty() {
                return id;
            }
        }
        if let Ok(id) = std::fs::read_to_string("/var/lib/dbus/machine-id") {
            let id = id.trim().to_string();
            if !id.is_empty() {
                return id;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};

        if let Ok(key) =
            RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\Microsoft\\Cryptography")
            && let Ok(guid) = key.get_value::<String, _>("MachineGuid")
        {
            let guid = guid.trim().to_string();
            if !guid.is_empty() {
                return guid;
            }
        }
    }

    "tiny-shell-default-hardware-uuid-fallback".to_string()
}

fn encrypt_config(config: &ConfigFile, password: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);

    let key = crate::crypto::derive_key(password, &salt)?;
    let plaintext = serde_json::to_vec(config).context("serialize config")?;
    let ciphertext = XChaCha20Poly1305::new((&key).into())
        .encrypt(XNonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| anyhow::anyhow!("encrypt config payload"))?;

    serde_json::to_vec_pretty(&EncryptedConfigEnvelope {
        format_version: 1,
        kdf: "argon2id".to_string(),
        cipher: "xchacha20poly1305".to_string(),
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        payload: STANDARD.encode(ciphertext),
    })
    .context("serialize encrypted config envelope")
}

fn decrypt_config(raw: &[u8], password: &str) -> Result<ConfigFile> {
    let envelope: EncryptedConfigEnvelope =
        serde_json::from_slice(raw).context("parse encrypted config envelope")?;
    if envelope.format_version != 1
        || envelope.kdf != "argon2id"
        || envelope.cipher != "xchacha20poly1305"
    {
        return Err(anyhow::anyhow!("unsupported encrypted config format"));
    }
    let salt = STANDARD
        .decode(envelope.salt)
        .context("decode config salt")?;
    let nonce = STANDARD
        .decode(envelope.nonce)
        .context("decode config nonce")?;
    if nonce.len() != 24 {
        return Err(anyhow::anyhow!("invalid config nonce"));
    }
    let ciphertext = STANDARD
        .decode(envelope.payload)
        .context("decode encrypted config payload")?;

    let key = crate::crypto::derive_key(password, &salt)?;
    let plaintext = XChaCha20Poly1305::new((&key).into())
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| {
            anyhow::anyhow!("cannot decrypt config; hardware UUID mismatch or corrupted data")
        })?;

    serde_json::from_slice(&plaintext).context("parse decrypted config")
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
    fn sftp_footer_visibility_defaults_missing_sync_status_to_visible() {
        let visibility: SftpFooterVisibility =
            serde_json::from_str(r#"{"latency":false,"transfers":false,"panel_toggle":false}"#)
                .unwrap();

        assert!(visibility.webdav);
        assert!(!visibility.latency);
        assert!(!visibility.transfers);
        assert!(!visibility.panel_toggle);
    }

    #[test]
    fn default_font_sizes_are_14_px() {
        let config = ConfigFile::default();
        assert_eq!(config.terminal_font_size, 14.0);
        assert_eq!(
            config.terminal_display_style,
            TerminalDisplayStyle::Standard
        );
        assert_eq!(config.ui_font_size, 14.0);
        assert_eq!(config.update_check_mode, UpdateCheckMode::Startup);
        assert_eq!(config.update_interval_hours, 24);
        assert!(config.update_notify);
        assert!(!config.sync_enabled);
        assert_eq!(config.sync_interval_hours, 24);
        assert_eq!(config.sync_last_synced_at, 0);
        assert!(config.sync_webdav_password_sealed.is_empty());
        assert!(config.download_directory.is_empty());
    }

    #[test]
    fn terminal_display_style_defaults_to_standard_when_missing() {
        let config: ConfigFile = serde_json::from_str("{}").unwrap();

        assert_eq!(
            config.terminal_display_style,
            TerminalDisplayStyle::Standard
        );
    }

    #[test]
    fn merging_preferences_preserves_connection_data() {
        let mut latest = ConfigStore::in_memory();
        latest.cache.connection_groups = vec!["production".to_string()];
        let mut source = ConfigStore::in_memory();
        source.cache.connection_groups = vec!["stale".to_string()];
        source.set_ui_font_size(18.0);
        source.set_terminal_display_style(TerminalDisplayStyle::Compact);
        source.set_update_check_mode(UpdateCheckMode::Interval);
        source.set_update_interval_hours(12);
        source.set_update_notify(false);
        source.set_download_directory(Some(Path::new("downloads")));
        latest.merge_interactive_preferences_from(&source);

        assert_eq!(latest.cache.connection_groups, ["production"]);
        assert_eq!(latest.ui_font_size(), 18.0);
        assert_eq!(
            latest.terminal_display_style(),
            TerminalDisplayStyle::Compact
        );
        assert_eq!(latest.update_check_mode(), UpdateCheckMode::Interval);
        assert_eq!(latest.update_interval_hours(), 12);
        assert!(!latest.update_notify());
        assert_eq!(
            latest.download_directory(),
            Some(PathBuf::from("downloads"))
        );
    }

    #[test]
    fn quick_command_categories_and_commands_support_crud() {
        let mut store = ConfigStore::in_memory();
        let category = QuickCommandCategory {
            id: "category-1".to_string(),
            name: "System".to_string(),
            commands: Vec::new(),
        };
        store.set_quick_command_categories(vec![category]);
        store.upsert_quick_command(
            "category-1",
            QuickCommand {
                id: "command-1".to_string(),
                name: "Uptime".to_string(),
                remark: String::new(),
                command: "uptime".to_string(),
            },
        );

        let categories = store.quick_command_categories().unwrap();
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0].commands[0].command, "uptime");

        store.remove_quick_command("category-1", "command-1");
        assert!(
            store.quick_command_categories().unwrap()[0]
                .commands
                .is_empty()
        );
        store.remove_quick_command_category("category-1");
        assert!(store.quick_command_categories().unwrap().is_empty());
    }

    #[test]
    fn synchronization_next_time_is_derived_from_last_success() {
        let mut store = ConfigStore::in_memory();
        assert_eq!(store.sync_next_at(), 0);

        store.set_sync_interval_hours(6);
        store.set_sync_last_synced_at(1_700_000_000);

        assert_eq!(store.sync_next_at(), 1_700_021_600);
    }

    #[test]
    fn test_get_hardware_uuid() {
        let uuid = get_hardware_uuid();
        assert!(!uuid.is_empty());
    }

    #[test]
    fn test_config_encryption_roundtrip() {
        let config = ConfigFile::default();
        let password = "test-password-123";
        let encrypted = encrypt_config(&config, password).unwrap();

        // Ensure it doesn't contain plain text fields of default config
        let encrypted_str = String::from_utf8_lossy(&encrypted);
        assert!(!encrypted_str.contains("Maple Mono NF CN"));
        assert!(encrypted_str.contains("argon2id"));

        let decrypted = decrypt_config(&encrypted, password).unwrap();
        assert_eq!(decrypted.terminal_font_family, config.terminal_font_family);

        // Decrypt with wrong password should fail
        assert!(decrypt_config(&encrypted, "wrong-password").is_err());
    }

    #[test]
    fn config_save_replaces_existing_file_without_leaving_temp_files() {
        let dir = std::env::temp_dir().join(format!("tiny-shell-config-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sessions.json");
        fs::write(&path, b"old config").unwrap();

        let store = ConfigStore {
            path: path.clone(),
            cache: ConfigFile::default(),
        };
        store.save().unwrap();

        let encrypted = fs::read(&path).unwrap();
        let decrypted = decrypt_config(&encrypted, &get_hardware_uuid()).unwrap();
        assert_eq!(
            decrypted.terminal_font_family,
            store.cache.terminal_font_family
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sync_etag_is_bound_to_the_exact_remote_target() {
        let mut store = ConfigStore::in_memory();
        store.set_sync_connection("https://dav.example.test/first/".into(), "alice".into());
        store.set_sync_etag(Some("etag-first".into()));
        assert_eq!(store.cache.sync_etag.as_deref(), Some("etag-first"));

        store.set_sync_connection("https://dav.example.test/first".into(), "bob".into());
        assert!(store.cache.sync_etag.is_none());

        store.set_sync_etag(Some("etag-bob".into()));
        store.set_sync_connection("https://dav.example.test/second/".into(), "bob".into());
        assert!(store.cache.sync_etag.is_none());
    }

    #[test]
    fn switching_s3_objects_invalidates_the_previous_etag() {
        let mut store = ConfigStore::in_memory();
        store.set_sync_backend("s3");
        store.set_sync_s3_connection(
            "https://s3.example.test".into(),
            "us-east-1".into(),
            "configs".into(),
            "first.json".into(),
        );
        store.set_sync_etag(Some("etag-first".into()));

        store.set_sync_s3_connection(
            "https://s3.example.test/".into(),
            "us-east-1".into(),
            "configs".into(),
            "/second.json".into(),
        );
        assert!(store.cache.sync_etag.is_none());
    }

    #[test]
    fn connection_group_order_is_created_and_reordered_explicitly() {
        let mut store = ConfigStore {
            path: PathBuf::from("unused.json"),
            cache: ConfigFile::default(),
        };
        store.add_connection_group("first".into());
        store.add_connection_group("second".into());
        store.add_connection_group("third".into());
        assert_eq!(store.connection_groups(), ["first", "second", "third"]);

        store.add_connection_group("second/child".into());
        store.reorder_connection_group("second", Some("first"));
        assert_eq!(
            store.connection_groups(),
            ["second", "second/child", "first", "third"]
        );

        store.reorder_connection_group("third", None);
        assert_eq!(
            store.connection_groups(),
            ["second", "second/child", "first", "third"]
        );
    }
}
