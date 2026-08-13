use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    crypto::{open_bytes_with_hardware_key, seal_bytes_with_hardware_key},
    session::config::{ConfigStore, hardware_uuid},
    sync::{SyncBackendCredentials, protocol::V3SyncPayload},
};

const STATE_FILE_NAME: &str = "sync-state.json";
const STATE_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncTargetKey {
    pub backend: String,
    pub target: String,
}

impl SyncTargetKey {
    pub fn from_credentials(credentials: &SyncBackendCredentials) -> Self {
        match credentials {
            SyncBackendCredentials::WebDav { endpoint, .. } => Self::webdav(endpoint),
            SyncBackendCredentials::S3 {
                endpoint,
                region,
                bucket,
                object_key,
                ..
            } => Self::s3(endpoint, region, bucket, object_key),
        }
    }

    pub fn webdav(endpoint: &str) -> Self {
        Self {
            backend: "webdav".to_string(),
            target: normalize_endpoint(endpoint),
        }
    }

    pub fn s3(endpoint: &str, region: &str, bucket: &str, object_key: &str) -> Self {
        Self {
            backend: "s3".to_string(),
            target: format!(
                "{}|{}|{}|{}",
                normalize_endpoint(endpoint),
                region.trim(),
                bucket.trim(),
                object_key.trim().trim_start_matches('/')
            ),
        }
    }

    pub fn stable_id(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.backend.as_bytes());
        digest.update([0]);
        digest.update(self.target.as_bytes());
        let digest = digest.finalize();
        let encoded = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("target:{encoded}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBaseline {
    pub target_id: String,
    pub protocol_version: u32,
    pub payload: V3SyncPayload,
    pub remote_revision: String,
    pub remote_etag: Option<String>,
    pub synced_at: i64,
}

impl SyncBaseline {
    pub fn from_remote_payload(
        target: &SyncTargetKey,
        payload: V3SyncPayload,
        remote_etag: Option<String>,
        synced_at: i64,
    ) -> Self {
        let remote_revision = payload.revision.clone();
        Self {
            target_id: target.stable_id(),
            protocol_version: crate::sync::protocol::V3_FORMAT_VERSION,
            payload,
            remote_revision,
            remote_etag,
            synced_at,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SyncStateFile {
    format_version: u32,
    #[serde(default)]
    baselines: Vec<SyncBaseline>,
}

trait SyncStateIo: Send + Sync {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>>;
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()>;
}

#[derive(Debug, Default)]
struct FileSyncStateIo;

impl SyncStateIo for FileSyncStateIo {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("read sync state {}", path.display())),
        }
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let parent = path
            .parent()
            .context("sync state path has no parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("create sync state directory {}", parent.display()))?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("sync state path has invalid file name")?;
        let temporary = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .with_context(|| format!("create temporary sync state {}", temporary.display()))?;
            file.write_all(bytes)
                .with_context(|| format!("write temporary sync state {}", temporary.display()))?;
            file.sync_all()
                .with_context(|| format!("sync temporary sync state {}", temporary.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
                    .with_context(|| format!("secure sync state {}", temporary.display()))?;
            }
            fs::rename(&temporary, path).with_context(|| {
                format!(
                    "replace sync state {} with {}",
                    path.display(),
                    temporary.display()
                )
            })?;
            #[cfg(unix)]
            OpenOptions::new()
                .read(true)
                .open(parent)
                .and_then(|directory| directory.sync_all())
                .with_context(|| format!("sync sync state directory {}", parent.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

pub struct SyncStateRepository {
    path: PathBuf,
    io: Arc<dyn SyncStateIo>,
}

impl SyncStateRepository {
    pub fn new() -> Result<Self> {
        Ok(Self {
            path: ConfigStore::config_directory()?.join(STATE_FILE_NAME),
            io: Arc::new(FileSyncStateIo),
        })
    }

    #[cfg(test)]
    fn with_io(path: PathBuf, io: Arc<dyn SyncStateIo>) -> Self {
        Self { path, io }
    }

    pub fn load_for(&self, target: &SyncTargetKey) -> Result<Option<SyncBaseline>> {
        let target_id = target.stable_id();
        Ok(self
            .load_file()?
            .baselines
            .into_iter()
            .find(|baseline| baseline.target_id == target_id))
    }

    pub fn save(&self, target: &SyncTargetKey, mut baseline: SyncBaseline) -> Result<()> {
        baseline.target_id = target.stable_id();
        baseline.protocol_version = crate::sync::protocol::V3_FORMAT_VERSION;
        let mut state = self.load_file()?;
        if let Some(existing) = state
            .baselines
            .iter_mut()
            .find(|item| item.target_id == baseline.target_id)
        {
            *existing = baseline;
        } else {
            state.baselines.push(baseline);
        }
        self.save_file(&state)
    }

    fn load_file(&self) -> Result<SyncStateFile> {
        let Some(raw) = self.io.read(&self.path)? else {
            return Ok(SyncStateFile {
                format_version: STATE_FORMAT_VERSION,
                baselines: Vec::new(),
            });
        };
        let plaintext = open_bytes_with_hardware_key(&raw, &hardware_uuid())
            .context("decrypt local sync state")?;
        let state: SyncStateFile =
            serde_json::from_slice(&plaintext).context("parse local sync state")?;
        if state.format_version != STATE_FORMAT_VERSION {
            return Err(anyhow!(
                "unsupported local sync state version {}",
                state.format_version
            ));
        }
        Ok(state)
    }

    fn save_file(&self, state: &SyncStateFile) -> Result<()> {
        let plaintext = serde_json::to_vec(state).context("serialize local sync state")?;
        let encrypted = seal_bytes_with_hardware_key(&plaintext, &hardware_uuid())
            .context("encrypt local sync state")?;
        self.io.write_atomic(&self.path, &encrypted)
    }
}

fn normalize_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim();
    let Ok(mut url) = reqwest::Url::parse(trimmed) else {
        return trimmed.trim_end_matches('/').to_string();
    };
    url.set_fragment(None);
    let normalized_path = url.path().trim_end_matches('/').to_string();
    if normalized_path.is_empty() {
        url.set_path("/");
    } else {
        url.set_path(&normalized_path);
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::{
        session::config::AuthMethod,
        sync::{
            model::{SyncManagedKey, SyncSecret, SyncSession},
            protocol::{EntityVersion, SyncEntity, V3SyncPayload},
        },
    };

    #[derive(Default)]
    struct MemoryIo {
        bytes: Mutex<Option<Vec<u8>>>,
    }

    impl SyncStateIo for MemoryIo {
        fn read(&self, _path: &Path) -> Result<Option<Vec<u8>>> {
            self.bytes
                .lock()
                .map(|bytes| bytes.clone())
                .map_err(|_| anyhow!("memory state lock poisoned"))
        }

        fn write_atomic(&self, _path: &Path, bytes: &[u8]) -> Result<()> {
            self.bytes
                .lock()
                .map(|mut current| *current = Some(bytes.to_vec()))
                .map_err(|_| anyhow!("memory state lock poisoned"))
        }
    }

    #[derive(Default)]
    struct FailingWriteIo;

    impl SyncStateIo for FailingWriteIo {
        fn read(&self, _path: &Path) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }

        fn write_atomic(&self, _path: &Path, _bytes: &[u8]) -> Result<()> {
            Err(anyhow!("injected sync state write failure"))
        }
    }

    fn baseline() -> SyncBaseline {
        SyncBaseline {
            target_id: String::new(),
            protocol_version: 0,
            payload: V3SyncPayload {
                schema_version: 3,
                revision: "revision".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                device_id: "device".into(),
                sessions: Vec::new(),
                managed_keys: Vec::new(),
                connection_groups: Vec::new(),
                quick_command_categories: Vec::new(),
                quick_commands: Vec::new(),
                deleted_sessions: Vec::new(),
                deleted_managed_keys: Vec::new(),
                deleted_connection_groups: Vec::new(),
                deleted_quick_command_categories: Vec::new(),
                deleted_quick_commands: Vec::new(),
                deleted_session_snapshots: Vec::new(),
                deleted_group_snapshots: Vec::new(),
                privacy_password_verifier: None,
                secrets: Vec::new(),
            },
            remote_revision: "revision".into(),
            remote_etag: Some("etag".into()),
            synced_at: 1,
        }
    }

    #[test]
    fn target_keys_normalize_and_isolate_targets() {
        let webdav_a = SyncTargetKey::webdav("HTTPS://example.test/sync/");
        let webdav_b = SyncTargetKey::webdav("https://example.test/sync");
        let case_sensitive_path = SyncTargetKey::webdav("https://example.test/Sync");
        let s3 = SyncTargetKey::s3("https://example.test", "us-east-1", "bucket", "/one");
        let normalized_s3 =
            SyncTargetKey::s3("HTTPS://example.test/", "us-east-1", "bucket", "one");

        assert_eq!(webdav_a, webdav_b);
        assert_ne!(webdav_a, case_sensitive_path);
        assert_eq!(s3, normalized_s3);
        assert_ne!(webdav_a.stable_id(), s3.stable_id());
        assert!(!webdav_a.stable_id().contains("example"));
    }

    #[test]
    fn save_and_load_are_target_scoped() {
        let io = Arc::new(MemoryIo::default());
        let io_for_repository: Arc<dyn SyncStateIo> = io.clone();
        let repository = SyncStateRepository::with_io(PathBuf::from("memory"), io_for_repository);
        let target_a = SyncTargetKey::webdav("https://a.test/");
        let target_b = SyncTargetKey::webdav("https://b.test/");

        repository.save(&target_a, baseline()).unwrap();
        let mut second = baseline();
        second.remote_revision = "revision-2".into();
        repository.save(&target_b, second).unwrap();

        assert_eq!(
            repository
                .load_for(&target_a)
                .unwrap()
                .unwrap()
                .remote_revision,
            "revision"
        );
        assert_eq!(
            repository
                .load_for(&target_b)
                .unwrap()
                .unwrap()
                .remote_revision,
            "revision-2"
        );
        assert!(repository.load_for(&target_a).unwrap().is_some());
        assert!(repository.load_for(&target_b).unwrap().is_some());
    }

    #[test]
    fn corrupted_and_wrong_device_state_are_rejected() {
        let io = Arc::new(MemoryIo::default());
        let io_for_repository: Arc<dyn SyncStateIo> = io.clone();
        let repository = SyncStateRepository::with_io(PathBuf::from("memory"), io_for_repository);
        *io.bytes.lock().unwrap() = Some(b"corrupted".to_vec());
        assert!(
            repository
                .load_for(&SyncTargetKey::webdav("https://example.test"))
                .is_err()
        );

        let wrong_device = crate::crypto::seal_bytes_with_hardware_key(
            br#"{"format_version":1,"baselines":[]}"#,
            "different-hardware-id",
        )
        .unwrap();
        *io.bytes.lock().unwrap() = Some(wrong_device);
        assert!(
            repository
                .load_for(&SyncTargetKey::webdav("https://example.test"))
                .is_err()
        );
    }

    #[test]
    fn write_failure_is_returned_without_reporting_success() {
        let repository =
            SyncStateRepository::with_io(PathBuf::from("memory"), Arc::new(FailingWriteIo));

        assert!(
            repository
                .save(&SyncTargetKey::webdav("https://example.test"), baseline())
                .is_err()
        );
    }

    #[test]
    fn encrypted_state_does_not_contain_sensitive_payload_values() {
        let io = Arc::new(MemoryIo::default());
        let io_for_repository: Arc<dyn SyncStateIo> = io.clone();
        let repository = SyncStateRepository::with_io(PathBuf::from("memory"), io_for_repository);
        let mut value = baseline();
        let version = EntityVersion::initial("device", 1);
        value.payload.sessions.push(SyncEntity {
            id: "session-1".into(),
            version: version.clone(),
            value: SyncSession {
                id: "session-1".into(),
                name: "Sensitive session".into(),
                host: "example.test".into(),
                port: 22,
                user: "alice".into(),
                auth: AuthMethod::Key,
                password: SyncSecret::LegacyPlaintext("session-password-marker".into()),
                private_key_path: String::new(),
                private_key_inline: SyncSecret::LegacyPlaintext("inline-key-marker".into()),
                passphrase: SyncSecret::LegacyPlaintext("passphrase-marker".into()),
                managed_key_id: Some("key-1".into()),
                last_used: None,
                group: None,
                proxy_type: "socks5".into(),
                proxy_host: "proxy.test".into(),
                proxy_port: Some(1080),
                proxy_user: "proxy-user".into(),
                proxy_password: SyncSecret::LegacyPlaintext("proxy-password-marker".into()),
            },
        });
        value.payload.managed_keys.push(SyncEntity {
            id: "key-1".into(),
            version,
            value: SyncManagedKey {
                id: "key-1".into(),
                name: "Sensitive key".into(),
                key_type: "ed25519".into(),
                fingerprint: "SHA256:test".into(),
                inline_content: SyncSecret::LegacyPlaintext("managed-key-marker".into()),
                passphrase: SyncSecret::LegacyPlaintext("managed-passphrase-marker".into()),
                created_at: 1,
            },
        });
        repository
            .save(&SyncTargetKey::webdav("https://example.test"), value)
            .unwrap();
        let bytes = io.bytes.lock().unwrap().clone().unwrap();
        let encrypted = String::from_utf8_lossy(&bytes);

        for marker in [
            "session-password-marker",
            "inline-key-marker",
            "passphrase-marker",
            "proxy-password-marker",
            "managed-key-marker",
            "managed-passphrase-marker",
        ] {
            assert!(!encrypted.contains(marker));
        }
    }

    #[test]
    fn file_repository_overwrites_atomically_without_leaving_temporary_files() {
        let directory = std::env::temp_dir().join(format!(
            "tiny-shell-sync-state-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(STATE_FILE_NAME);
        let repository = SyncStateRepository::with_io(path.clone(), Arc::new(FileSyncStateIo));
        let target = SyncTargetKey::webdav("https://example.test/Sync");

        repository.save(&target, baseline()).unwrap();
        let mut updated = baseline();
        updated.remote_revision = "updated-revision".into();
        repository.save(&target, updated).unwrap();

        assert_eq!(
            repository
                .load_for(&target)
                .unwrap()
                .map(|baseline| baseline.remote_revision),
            Some("updated-revision".into())
        );
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }
}
