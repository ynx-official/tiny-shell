use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::session::highlight_rules::{
    BUILTIN_HIGHLIGHT_PACK_VERSION, HighlightRule, HighlightRulePack,
    default_enabled_highlight_packs, default_highlight_rules,
};
use crate::session::session_types::{
    DeletedConnectionGroup, DeletedSession, ManagedKey, QuickCommandCategory, Session,
    SftpFooterVisibility, SftpToolbarVisibility,
};
use crate::terminal::Transfer;

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
    #[serde(default = "default_highlight_rules")]
    pub highlight_rules: Vec<HighlightRule>,
    #[serde(default)]
    pub highlight_pack_version: u32,
    #[serde(default = "default_enabled_highlight_packs")]
    pub enabled_highlight_packs: Vec<HighlightRulePack>,
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
    pub transfers: Vec<Transfer>,
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
    pub quick_commands_builtin_version: u32,
    #[serde(default)]
    pub sftp_external_editor: String,
    #[serde(default)]
    pub key_bindings: HashMap<String, String>,
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
    /// New clients store the polling interval in minutes. Keeping the legacy
    /// hours field readable lets older configuration files migrate safely.
    #[serde(default)]
    pub sync_interval_minutes: Option<u32>,
    #[serde(default)]
    pub sync_interval_hours: Option<u32>,
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

pub(crate) fn default_read_env_proxy() -> bool {
    true
}

pub(crate) fn default_global_proxy_type() -> String {
    "socks5".to_string()
}

pub(crate) fn default_monitoring_position() -> String {
    "Sidebar".to_string()
}

pub(crate) fn default_sftp_panel_view() -> String {
    "files".to_string()
}

pub(crate) fn default_s3_region() -> String {
    "us-east-1".to_string()
}

pub(crate) fn default_s3_object_key() -> String {
    "tiny-shell-sync.json".to_string()
}

pub(crate) fn default_follow_system_theme() -> bool {
    true
}

pub(crate) fn default_locale() -> String {
    "system".to_string()
}

pub(crate) fn default_terminal_font_size() -> f32 {
    14.0
}

pub(crate) fn default_ui_font_size() -> f32 {
    14.0
}

pub(crate) fn default_sync_interval_hours() -> u32 {
    24
}

pub(crate) fn default_sync_interval_minutes() -> u32 {
    5
}

pub(crate) fn default_update_interval_hours() -> u32 {
    24
}

pub(crate) fn default_update_notify() -> bool {
    true
}

pub fn default_ui_font_family() -> String {
    // ".SystemUIFont" is a GPUI sentinel that resolves to the platform system UI font.
    // This matches gpui-component's own Theme default.
    ".SystemUIFont".to_string()
}

pub(crate) const SYSTEM_MONO_FONT: &str = ".SystemMonoFont";

pub(crate) fn default_terminal_font_family() -> String {
    SYSTEM_MONO_FONT.to_string()
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
            highlight_rules: default_highlight_rules(),
            highlight_pack_version: BUILTIN_HIGHLIGHT_PACK_VERSION,
            enabled_highlight_packs: default_enabled_highlight_packs(),
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
            quick_commands_builtin_version: 0,
            sftp_external_editor: String::new(),
            key_bindings: HashMap::new(),
            sync_endpoint: String::new(),
            sync_username: String::new(),
            sync_etag: None,
            sync_device_id: String::new(),
            sync_backend: String::new(),
            sync_enabled: false,
            sync_interval_minutes: Some(default_sync_interval_minutes()),
            sync_interval_hours: Some(default_sync_interval_hours()),
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
            read_env_proxy: default_read_env_proxy(),
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

impl ConfigFile {
    pub(crate) fn migrate_sync_interval(&mut self) {
        let minutes = self
            .sync_interval_minutes
            .or_else(|| {
                self.sync_interval_hours
                    .map(|hours| hours.saturating_mul(60))
            })
            .unwrap_or_else(default_sync_interval_minutes)
            .clamp(1, 525_600);
        self.sync_interval_minutes = Some(minutes);
        if self.sync_interval_hours.is_none() {
            self.sync_interval_hours = Some(minutes.div_ceil(60));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_font_sizes_are_14_px() {
        let config = ConfigFile::default();
        assert_eq!(config.terminal_font_size, 14.0);
        assert_eq!(
            config.terminal_display_style,
            TerminalDisplayStyle::Standard
        );
        assert_eq!(config.ui_font_size, 14.0);
        assert_eq!(config.terminal_font_family, SYSTEM_MONO_FONT);
        assert_eq!(config.update_check_mode, UpdateCheckMode::Startup);
        assert_eq!(config.update_interval_hours, 24);
        assert!(config.update_notify);
        assert!(!config.sync_enabled);
        assert_eq!(config.sync_interval_minutes, Some(5));
        assert_eq!(config.sync_interval_hours, Some(24));
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
        assert_eq!(config.terminal_font_family, SYSTEM_MONO_FONT);
    }

    #[test]
    fn missing_highlight_rules_receive_recommended_defaults() {
        let config: ConfigFile = serde_json::from_str("{}").unwrap();

        assert_eq!(config.highlight_rules, default_highlight_rules());
        assert_eq!(
            config.enabled_highlight_packs,
            default_enabled_highlight_packs()
        );
        assert_eq!(config.highlight_pack_version, 0);
    }

    #[test]
    fn explicitly_empty_highlight_rules_are_preserved() {
        let config: ConfigFile = serde_json::from_str(r#"{"highlight_rules":[]}"#).unwrap();

        assert!(config.highlight_rules.is_empty());
    }

    #[test]
    fn legacy_maple_font_preference_is_preserved() {
        let config: ConfigFile =
            serde_json::from_str(r#"{"terminal_font_family":"Maple Mono NF CN"}"#).unwrap();

        assert_eq!(config.terminal_font_family, "Maple Mono NF CN");
    }

    #[test]
    fn legacy_sync_interval_is_converted_from_hours_to_minutes() {
        let mut config: ConfigFile = serde_json::from_str(r#"{"sync_interval_hours":24}"#).unwrap();

        config.migrate_sync_interval();

        assert_eq!(config.sync_interval_minutes, Some(1_440));
    }
}
