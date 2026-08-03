use std::{
    collections::HashSet,
    fs::File,
    io::{self, Read},
    path::Path,
};

use chrono::{TimeZone, Utc};
use encoding_rs::GBK;
use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;
use zip::{ZipArchive, result::ZipError};

use super::super::config::{AuthMethod, ConfigStore, Session};

const MAX_ENTRIES: usize = 2_000;
const MAX_JSON_SIZE: u64 = 1_048_576;
const MAX_TOTAL_SIZE: u64 = 32 * 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalShellImportErrorKind {
    Open,
    InvalidArchive,
    TooManyEntries,
    TooLarge,
    ReadEntry,
    InvalidFilenameEncoding,
    UnsafePath,
    ReadConnection,
    NoConnections,
}

#[derive(Debug, Error)]
pub(crate) enum FinalShellImportError {
    #[error("failed to open FinalShell backup")]
    Open(#[source] io::Error),
    #[error("invalid FinalShell ZIP archive")]
    InvalidArchive(#[source] ZipError),
    #[error("FinalShell archive contains too many entries")]
    TooManyEntries,
    #[error("FinalShell archive is too large")]
    TooLarge,
    #[error("failed to read FinalShell ZIP entry")]
    ReadEntry(#[source] ZipError),
    #[error("invalid ZIP filename encoding")]
    InvalidFilenameEncoding,
    #[error("unsafe ZIP path")]
    UnsafePath,
    #[error("failed to read FinalShell connection")]
    ReadConnection(#[source] io::Error),
    #[error("no supported FinalShell connections found")]
    NoConnections,
}

impl FinalShellImportError {
    pub fn kind(&self) -> FinalShellImportErrorKind {
        match self {
            Self::Open(_) => FinalShellImportErrorKind::Open,
            Self::InvalidArchive(_) => FinalShellImportErrorKind::InvalidArchive,
            Self::TooManyEntries => FinalShellImportErrorKind::TooManyEntries,
            Self::TooLarge => FinalShellImportErrorKind::TooLarge,
            Self::ReadEntry(_) => FinalShellImportErrorKind::ReadEntry,
            Self::InvalidFilenameEncoding => FinalShellImportErrorKind::InvalidFilenameEncoding,
            Self::UnsafePath => FinalShellImportErrorKind::UnsafePath,
            Self::ReadConnection(_) => FinalShellImportErrorKind::ReadConnection,
            Self::NoConnections => FinalShellImportErrorKind::NoConnections,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FinalShellImportPreview {
    pub groups: Vec<String>,
    pub sessions: Vec<ImportedSession>,
    pub skipped_entries: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ImportedSession {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: AuthMethod,
    pub group: Option<String>,
    pub last_used: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FinalShellImportSummary {
    pub imported_sessions: usize,
    pub skipped_sessions: usize,
    pub imported_groups: usize,
}

#[derive(Debug, Deserialize)]
struct FinalShellConnection {
    #[serde(default)]
    name: String,
    #[serde(default)]
    host: String,
    #[serde(default)]
    port: u64,
    #[serde(default)]
    user_name: String,
    #[serde(default)]
    authentication_type: i64,
    #[serde(default)]
    conection_type: i64,
    #[serde(default)]
    access_time: i64,
}

pub(crate) fn parse_finalshell_zip(
    path: &Path,
) -> Result<FinalShellImportPreview, FinalShellImportError> {
    let file = File::open(path).map_err(FinalShellImportError::Open)?;
    let mut archive = ZipArchive::new(file).map_err(FinalShellImportError::InvalidArchive)?;
    if archive.len() > MAX_ENTRIES {
        return Err(FinalShellImportError::TooManyEntries);
    }

    let mut groups = Vec::new();
    let mut group_set = HashSet::new();
    let mut sessions = Vec::new();
    let mut skipped_entries = 0;
    let mut total_size = 0_u64;
    let mut total_json_size = 0_u64;

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(FinalShellImportError::ReadEntry)?;
        total_size = total_size.saturating_add(entry.size());
        if total_size > MAX_TOTAL_SIZE {
            return Err(FinalShellImportError::TooLarge);
        }

        let path = decode_zip_path(entry.name_raw())?;
        if path.is_empty() {
            continue;
        }
        if entry.is_dir() {
            add_group_path(&path, &mut groups, &mut group_set);
            continue;
        }
        if !path.ends_with("_connect_config.json") {
            continue;
        }
        if entry.size() > MAX_JSON_SIZE {
            skipped_entries += 1;
            continue;
        }

        let mut raw = Vec::with_capacity(entry.size().min(MAX_JSON_SIZE) as usize);
        entry
            .take(MAX_JSON_SIZE + 1)
            .read_to_end(&mut raw)
            .map_err(FinalShellImportError::ReadConnection)?;
        if raw.len() as u64 > MAX_JSON_SIZE {
            skipped_entries += 1;
            continue;
        }
        total_json_size = total_json_size.saturating_add(raw.len() as u64);
        if total_json_size > MAX_TOTAL_SIZE {
            return Err(FinalShellImportError::TooLarge);
        }
        let connection = match serde_json::from_slice::<FinalShellConnection>(&raw) {
            Ok(connection) => connection,
            Err(_) => {
                skipped_entries += 1;
                continue;
            }
        };

        if connection.conection_type != 0 && connection.conection_type != 100 {
            skipped_entries += 1;
            continue;
        }
        let auth = match connection.authentication_type {
            1 => AuthMethod::Password,
            2 => AuthMethod::KeyPending,
            _ => {
                skipped_entries += 1;
                continue;
            }
        };
        if connection.host.trim().is_empty()
            || connection.user_name.trim().is_empty()
            || connection.port == 0
            || connection.port > u16::MAX as u64
        {
            skipped_entries += 1;
            continue;
        }

        let group = path
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_string())
            .filter(|parent| !parent.is_empty());
        if let Some(group) = &group {
            add_group_path(group, &mut groups, &mut group_set);
        }
        let fallback_name = path
            .rsplit('/')
            .next()
            .unwrap_or("FinalShell connection")
            .trim_end_matches("_connect_config.json");
        let name = if connection.name.trim().is_empty() {
            fallback_name.to_string()
        } else {
            connection.name.trim().to_string()
        };
        let last_used = (connection.access_time > 0)
            .then(|| Utc.timestamp_millis_opt(connection.access_time).single())
            .flatten()
            .map(|time| time.to_rfc3339());
        sessions.push(ImportedSession {
            name,
            host: connection.host.trim().to_string(),
            port: connection.port as u16,
            user: connection.user_name.trim().to_string(),
            auth,
            group,
            last_used,
        });
    }

    if sessions.is_empty() {
        return Err(FinalShellImportError::NoConnections);
    }
    Ok(FinalShellImportPreview {
        groups,
        sessions,
        skipped_entries,
    })
}

#[cfg(test)]
fn apply_finalshell_import(
    config: &mut ConfigStore,
    preview: FinalShellImportPreview,
) -> FinalShellImportSummary {
    let selected = vec![true; preview.sessions.len()];
    apply_finalshell_import_selected(config, preview, &selected)
}

pub(crate) fn apply_finalshell_import_selected(
    config: &mut ConfigStore,
    preview: FinalShellImportPreview,
    selected: &[bool],
) -> FinalShellImportSummary {
    let existing_groups = config.connection_groups().to_vec();
    let mut imported_groups = 0;
    for group in &preview.groups {
        if !existing_groups.iter().any(|existing| existing == group) {
            config.add_connection_group(group.clone());
            imported_groups += 1;
        }
    }

    let mut imported_sessions = 0;
    let skipped_sessions = preview.skipped_entries;
    for (index, imported) in preview.sessions.into_iter().enumerate() {
        if !selected.get(index).copied().unwrap_or(false) {
            continue;
        }

        let existing = config
            .sessions()
            .iter()
            .find(|existing| import_matches_existing(existing, &imported))
            .cloned();

        let mut session = match imported.auth {
            AuthMethod::Password => Session::password(
                imported.host.clone(),
                imported.port,
                imported.user.clone(),
                String::new(),
            ),
            AuthMethod::Key | AuthMethod::KeyPending => Session::key(
                imported.host.clone(),
                imported.port,
                imported.user.clone(),
                String::new(),
                String::new(),
                String::new(),
            ),
            AuthMethod::Config => unreachable!("FinalShell only supports password and key auth"),
        };
        if imported.auth == AuthMethod::KeyPending {
            session.auth = AuthMethod::KeyPending;
        }
        session.name = if existing.is_some() {
            imported.name.clone()
        } else {
            unique_session_name(config, &imported.name, imported.group.as_deref())
        };
        if let Some(existing) = existing {
            let existing_has_key = existing.managed_key_id.is_some()
                || !existing.private_key_path.is_empty()
                || !existing.private_key_inline.is_empty();
            session.id = existing.id;
            session.password = existing.password;
            session.private_key_path = existing.private_key_path;
            session.private_key_inline = existing.private_key_inline;
            session.passphrase = existing.passphrase;
            session.managed_key_id = existing.managed_key_id;
            session.proxy_type = existing.proxy_type;
            session.proxy_host = existing.proxy_host;
            session.proxy_port = existing.proxy_port;
            session.proxy_user = existing.proxy_user;
            session.proxy_password = existing.proxy_password;
            session.auth = if existing.auth == AuthMethod::Key && !existing_has_key {
                AuthMethod::KeyPending
            } else {
                existing.auth
            };
        }
        session.group = imported.group;
        session.last_used = imported.last_used;
        config.upsert(session);
        imported_sessions += 1;
    }

    FinalShellImportSummary {
        imported_sessions,
        skipped_sessions,
        imported_groups,
    }
}

pub(crate) fn import_matches_existing(existing: &Session, imported: &ImportedSession) -> bool {
    existing.group == imported.group
        && existing.name == imported.name
        && existing.host == imported.host
        && existing.port == imported.port
        && existing.user == imported.user
        && equivalent_import_auth(existing.auth, imported.auth)
}

fn unique_session_name(config: &ConfigStore, requested: &str, group: Option<&str>) -> String {
    if !config
        .sessions()
        .iter()
        .any(|session| session.group.as_deref() == group && session.name == requested)
    {
        return requested.to_string();
    }
    (2_u32..)
        .map(|suffix| format!("{requested} ({suffix})"))
        .find(|candidate| {
            !config
                .sessions()
                .iter()
                .any(|session| session.group.as_deref() == group && session.name == *candidate)
        })
        .unwrap_or_else(|| format!("{requested} ({})", Uuid::new_v4()))
}

fn equivalent_import_auth(existing: AuthMethod, imported: AuthMethod) -> bool {
    match imported {
        AuthMethod::Password => existing == AuthMethod::Password,
        AuthMethod::Key | AuthMethod::KeyPending => {
            matches!(existing, AuthMethod::Key | AuthMethod::KeyPending)
        }
        AuthMethod::Config => existing == AuthMethod::Config,
    }
}

fn add_group_path(path: &str, groups: &mut Vec<String>, group_set: &mut HashSet<String>) {
    let mut current = String::new();
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(segment);
        if group_set.insert(current.clone()) {
            groups.push(current.clone());
        }
    }
}

fn decode_zip_path(raw: &[u8]) -> Result<String, FinalShellImportError> {
    let decoded = match std::str::from_utf8(raw) {
        Ok(value) => value.to_string(),
        Err(_) => {
            let (value, _, had_errors) = GBK.decode(raw);
            if had_errors {
                return Err(FinalShellImportError::InvalidFilenameEncoding);
            }
            value.into_owned()
        }
    };
    let normalized = decoded.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.contains('\0')
        || normalized
            .split('/')
            .any(|part| part == ".." || part.contains(':'))
    {
        return Err(FinalShellImportError::UnsafePath);
    }
    Ok(normalized.trim_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn decodes_legacy_gbk_path() {
        let raw = [0xcd, 0xac, 0xbb, 0xd4, b'/'];
        assert_eq!(decode_zip_path(&raw).unwrap(), "同辉");
    }

    #[test]
    fn rejects_traversal_path() {
        assert!(decode_zip_path(b"../secret.json").is_err());
        assert!(decode_zip_path(b"C:/secret.json").is_err());
    }

    #[test]
    fn adds_parent_groups_in_order() {
        let mut groups = Vec::new();
        let mut set = HashSet::new();
        add_group_path("a/b/c", &mut groups, &mut set);
        assert_eq!(groups, ["a", "a/b", "a/b/c"]);
    }

    #[test]
    fn parses_only_connection_fields_from_zip_json() {
        let path = std::env::temp_dir().join(format!("tiny-shell-final-{0}.zip", Uuid::new_v4()));
        let file = File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        writer.add_directory("prod/eu/", options).unwrap();
        writer
            .start_file("prod/eu/server_connect_config.json", options)
            .unwrap();
        writer
            .write_all(
                br#"{
                    "name":"server",
                    "host":"example.test",
                    "port":22,
                    "user_name":"alice",
                    "authentication_type":2,
                    "conection_type":100,
                    "password":"must-not-be-imported"
                }"#,
            )
            .unwrap();
        writer.finish().unwrap();

        let preview = parse_finalshell_zip(&path).unwrap();
        assert_eq!(preview.groups, ["prod", "prod/eu"]);
        assert_eq!(preview.sessions.len(), 1);
        assert_eq!(preview.sessions[0].auth, AuthMethod::KeyPending);
        assert_eq!(preview.sessions[0].group.as_deref(), Some("prod/eu"));
        assert!(std::fs::remove_file(path).is_ok());
    }

    #[test]
    fn imports_without_persisting_credentials() {
        let mut config = ConfigStore::in_memory();
        let preview = FinalShellImportPreview {
            groups: vec!["prod".to_string()],
            sessions: vec![ImportedSession {
                name: "server".to_string(),
                host: "example.test".to_string(),
                port: 22,
                user: "alice".to_string(),
                auth: AuthMethod::Password,
                group: Some("prod".to_string()),
                last_used: None,
            }],
            skipped_entries: 0,
        };

        let summary = apply_finalshell_import(&mut config, preview);
        assert_eq!(summary.imported_sessions, 1);
        let session = &config.sessions()[0];
        assert!(session.password.is_empty());
        assert!(session.private_key_inline.is_empty());
        assert!(session.managed_key_id.is_none());
    }

    #[test]
    fn reimport_overwrites_key_connection_after_credentials_are_selected() {
        let mut config = ConfigStore::in_memory();
        let preview = FinalShellImportPreview {
            groups: vec!["prod".to_string()],
            sessions: vec![ImportedSession {
                name: "server".to_string(),
                host: "example.test".to_string(),
                port: 22,
                user: "alice".to_string(),
                auth: AuthMethod::KeyPending,
                group: Some("prod".to_string()),
                last_used: None,
            }],
            skipped_entries: 0,
        };

        let first = apply_finalshell_import(&mut config, preview.clone());
        assert_eq!(first.imported_sessions, 1);

        let id = config.sessions()[0].id.clone();
        let mut completed = config.get(&id).cloned().unwrap();
        completed.auth = AuthMethod::Key;
        completed.managed_key_id = Some("managed-key".to_string());
        config.upsert(completed);

        let second = apply_finalshell_import(&mut config, preview);
        assert_eq!(second.imported_sessions, 1);
        assert_eq!(second.skipped_sessions, 0);
        assert_eq!(config.sessions().len(), 1);
        assert_eq!(config.sessions()[0].id, id);
        assert_eq!(
            config.sessions()[0].managed_key_id.as_deref(),
            Some("managed-key")
        );
    }
}
