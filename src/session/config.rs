use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result};
use directories::BaseDirs;
use uuid::Uuid;

use crate::session::{
    config_file::{
        default_sync_interval_minutes, default_terminal_font_size, default_ui_font_size,
    },
    crypto::{decrypt_config, encrypt_config},
};

pub use crate::session::config_file::{
    ConfigFile, CursorStyle, SavedWindowBounds, TerminalDisplayStyle, TitleBarStyle,
    UpdateCheckMode,
};
pub(crate) use crate::session::crypto::hardware_uuid;
pub use crate::session::proxy::{ENV_PROXY, EnvProxy, active_proxy, connect_proxy};
pub use crate::session::session_types::{
    AuthMethod, ConnectionType, DeletedConnectionGroup, DeletedSession, ManagedKey, QuickCommand,
    QuickCommandCategory, Session, SftpFooterVisibility, SftpToolbarVisibility,
};

#[derive(Clone)]
pub struct ConfigStore {
    path: PathBuf,
    pub(crate) cache: ConfigFile,
}

/// Process-scoped temporary storage owned by TinyShell.
///
/// A workspace always uses a unique child of the supplied root, so starting a
/// second window or process never clears files that are still in use elsewhere.
/// Clones share ownership and the directory is removed only after the final
/// handle is dropped.
#[derive(Clone, Debug)]
pub struct TempWorkspace {
    inner: Arc<TempWorkspaceInner>,
}

#[derive(Debug)]
struct TempWorkspaceInner {
    path: PathBuf,
}

impl Drop for TempWorkspaceInner {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                path = %self.path.display(),
                "failed to clean TinyShell temporary workspace: {error}"
            );
        }
    }
}

impl TempWorkspace {
    /// Create a unique process workspace below `root` without touching sibling
    /// workspaces. Supplying the root explicitly keeps this usable in tests and
    /// by portable/platform-specific launchers.
    pub fn initialize_in(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        ensure_private_directory(root)?;
        let path = root.join(format!("runtime-{}-{}", std::process::id(), Uuid::new_v4()));
        ensure_private_directory(&path)?;
        Ok(Self {
            inner: Arc::new(TempWorkspaceInner { path }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Allocate an isolated directory for one operation. The returned guard
    /// keeps the process workspace alive and removes only its own directory.
    pub fn allocate(&self, purpose: &str) -> Result<TempTaskDirectory> {
        let purpose = sanitized_temp_component(purpose);
        let path = self.path().join(format!("{purpose}-{}", Uuid::new_v4()));
        ensure_private_directory(&path)?;
        Ok(TempTaskDirectory {
            path,
            _workspace: self.clone(),
        })
    }
}

#[derive(Debug)]
pub struct TempTaskDirectory {
    path: PathBuf,
    _workspace: TempWorkspace,
}

impl TempTaskDirectory {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempTaskDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                path = %self.path.display(),
                "failed to clean TinyShell temporary task directory: {error}"
            );
        }
    }
}

static PROCESS_TEMP_WORKSPACE: OnceLock<TempWorkspace> = OnceLock::new();

fn ensure_private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create temporary directory {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to secure temporary directory {}", path.display()))?;
    }

    Ok(())
}

fn sanitized_temp_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let sanitized = sanitized.trim_matches('-');
    if sanitized.is_empty() {
        "task".to_string()
    } else {
        sanitized.to_string()
    }
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

impl ConfigStore {
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        Self::load_from_path(path)
    }

    /// Read a configuration snapshot without creating, deleting, or rewriting
    /// any file-system entries.
    pub(crate) fn load_from_path(path: PathBuf) -> Result<Self> {
        let mut cache = if path.exists() {
            let raw_bytes =
                fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
            match decrypt_config(&raw_bytes, &hardware_uuid()) {
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

        cache.migrate_sync_interval();
        if cache.sync_device_id.is_empty() {
            cache.sync_device_id = Uuid::new_v4().to_string();
        }
        for session in &mut cache.sessions {
            normalize_key_auth_state(session);
        }
        Ok(Self { path, cache })
    }

    /// Initialize and retain the process-wide temporary workspace.
    ///
    /// Unlike [`Self::load`], this is an explicit mutating operation and should
    /// be called once during application startup.
    pub fn initialize_temp_workspace() -> Result<&'static TempWorkspace> {
        if let Some(workspace) = PROCESS_TEMP_WORKSPACE.get() {
            return Ok(workspace);
        }
        let root = Self::config_path()?
            .parent()
            .context("configuration path has no parent directory")?
            .join("tmp");
        let workspace = TempWorkspace::initialize_in(root)?;
        let _ = PROCESS_TEMP_WORKSPACE.set(workspace);
        PROCESS_TEMP_WORKSPACE
            .get()
            .context("failed to initialize process temporary workspace")
    }

    pub fn temp_workspace() -> Option<&'static TempWorkspace> {
        PROCESS_TEMP_WORKSPACE.get()
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

    #[allow(dead_code)]
    pub(crate) fn config_directory() -> Result<PathBuf> {
        Self::config_path()?
            .parent()
            .map(Path::to_path_buf)
            .context("configuration path has no parent directory")
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

    pub fn replace_sessions(&mut self, sessions: Vec<Session>) {
        self.cache.sessions = sessions
            .into_iter()
            .map(|mut session| {
                normalize_key_auth_state(&mut session);
                session
            })
            .collect();
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

    pub fn sync_interval_minutes(&self) -> u32 {
        self.cache
            .sync_interval_minutes
            .or_else(|| {
                self.cache
                    .sync_interval_hours
                    .map(|hours| hours.saturating_mul(60))
            })
            .unwrap_or_else(default_sync_interval_minutes)
            .clamp(1, 525_600)
    }

    pub fn set_sync_interval_minutes(&mut self, minutes: u32) {
        let minutes = minutes.clamp(1, 525_600);
        self.cache.sync_interval_minutes = Some(minutes);
        self.cache.sync_interval_hours = Some(minutes.div_ceil(60));
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
                .saturating_add(i64::from(self.sync_interval_minutes()).saturating_mul(60))
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

    pub fn body_panels(&self) -> Option<&Vec<f32>> {
        self.cache.body_panels.as_ref()
    }

    pub fn set_body_panels(&mut self, body_panels: Option<Vec<f32>>) {
        self.cache.body_panels = body_panels;
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

    pub fn quick_commands_builtin_version(&self) -> u32 {
        self.cache.quick_commands_builtin_version
    }

    pub fn set_quick_commands_builtin_version(&mut self, version: u32) {
        self.cache.quick_commands_builtin_version = version;
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
        self.cache.sidebar_collapsed = source.cache.sidebar_collapsed;
        self.cache.sftp_panel_minimized = source.cache.sftp_panel_minimized;
        self.cache.sftp_panel_view = source.cache.sftp_panel_view.clone();
        self.cache.sftp_toolbar_visibility = source.cache.sftp_toolbar_visibility;
        self.cache.sftp_footer_visibility = source.cache.sftp_footer_visibility;
        self.cache.quick_command_categories = source.cache.quick_command_categories.clone();
        self.cache.quick_commands_builtin_version = source.cache.quick_commands_builtin_version;
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
        self.cache.sync_interval_minutes = source.cache.sync_interval_minutes;
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
        let mut session = session;
        normalize_key_auth_state(&mut session);
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
        let encrypted_bytes = encrypt_config(&self.cache, &hardware_uuid())?;
        write_config_atomically(&self.path, &encrypted_bytes)
    }
}

fn normalize_key_auth_state(session: &mut Session) {
    if session.auth == AuthMethod::Key
        && session.managed_key_id.is_none()
        && session.private_key_path.is_empty()
        && session.private_key_inline.is_empty()
    {
        session.auth = AuthMethod::KeyPending;
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    use crate::session::crypto::decrypt_config;

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

        store.set_sync_interval_minutes(6);
        store.set_sync_last_synced_at(1_700_000_000);

        assert_eq!(store.sync_next_at(), 1_700_000_360);
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
        let decrypted = decrypt_config(&encrypted, &hardware_uuid()).unwrap();
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
