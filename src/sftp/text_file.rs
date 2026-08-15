use anyhow::{Context, Result, anyhow};
use russh_sftp::{client::SftpSession, protocol::FileAttributes};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

pub(crate) const EDITOR_SOFT_LIMIT_BYTES: u64 = 1024 * 1024;
pub(crate) const EDITOR_HARD_LIMIT_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteFileRevision {
    pub size: u64,
    pub modified: Option<u32>,
    pub permissions: Option<u32>,
    pub sha256: String,
}

impl RemoteFileRevision {
    pub fn from_bytes(bytes: &[u8], modified: Option<u32>, permissions: Option<u32>) -> Self {
        Self {
            size: bytes.len() as u64,
            modified,
            permissions,
            sha256: hex::encode(Sha256::digest(bytes)),
        }
    }

    pub fn same_content(&self, other: &Self) -> bool {
        self.size == other.size && self.sha256 == other.sha256
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextEncoding {
    Utf8,
    Utf8Bom,
}

impl TextEncoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RemoteTextFormat {
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
}

#[derive(Debug, Clone)]
pub(crate) struct RemoteTextFile {
    pub content: String,
    pub revision: RemoteFileRevision,
    pub format: RemoteTextFormat,
    pub large_file: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RemoteTextSave {
    pub remote_path: String,
    pub content: String,
    pub expected_revision: RemoteFileRevision,
    pub format: RemoteTextFormat,
    pub force: bool,
}

pub(crate) fn decode_remote_text(
    bytes: Vec<u8>,
    modified: Option<u32>,
    permissions: Option<u32>,
) -> Result<RemoteTextFile> {
    if bytes.len() as u64 > EDITOR_HARD_LIMIT_BYTES {
        return Err(anyhow!(
            "file too large for in-memory editor ({} bytes, max {} bytes)",
            bytes.len(),
            EDITOR_HARD_LIMIT_BYTES
        ));
    }

    let revision = RemoteFileRevision::from_bytes(&bytes, modified, permissions);
    let (encoding, body) = if let Some(body) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (TextEncoding::Utf8Bom, body)
    } else {
        (TextEncoding::Utf8, bytes.as_slice())
    };
    let text = std::str::from_utf8(body).map_err(|_| anyhow!("file is not valid UTF-8 text"))?;
    let line_ending = if text.contains("\r\n") {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    };

    Ok(RemoteTextFile {
        content: text.replace("\r\n", "\n"),
        revision,
        format: RemoteTextFormat {
            encoding,
            line_ending,
        },
        large_file: bytes.len() as u64 > EDITOR_SOFT_LIMIT_BYTES,
    })
}

pub(crate) fn encode_remote_text(content: &str, format: RemoteTextFormat) -> Vec<u8> {
    let normalized = content.replace("\r\n", "\n");
    let body = match format.line_ending {
        LineEnding::Lf => normalized,
        LineEnding::Crlf => normalized.replace('\n', "\r\n"),
    };
    let mut bytes = Vec::with_capacity(body.len() + 3);
    if format.encoding == TextEncoding::Utf8Bom {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

pub(super) enum SaveRemoteTextOutcome {
    Saved(RemoteFileRevision),
    Conflict(RemoteTextFile),
}

pub(super) async fn read_remote_text_file(
    sftp: &SftpSession,
    remote: &str,
) -> Result<RemoteTextFile> {
    let metadata = sftp
        .metadata(remote)
        .await
        .with_context(|| format!("stat remote {remote}"))?;
    if metadata
        .size
        .is_some_and(|size| size > EDITOR_HARD_LIMIT_BYTES)
    {
        return Err(anyhow!(
            "file too large for in-memory editor ({} bytes, max {} bytes)",
            metadata.size.unwrap_or_default(),
            EDITOR_HARD_LIMIT_BYTES
        ));
    }

    let mut file = sftp
        .open(remote)
        .await
        .with_context(|| format!("open remote {remote}"))?;
    let mut bytes = Vec::with_capacity(metadata.size.unwrap_or_default() as usize);
    file.read_to_end(&mut bytes)
        .await
        .with_context(|| format!("read remote {remote}"))?;
    decode_remote_text(bytes, metadata.mtime, metadata.permissions)
        .with_context(|| format!("decode remote {remote}"))
}

async fn write_remote_temp_file(
    sftp: &SftpSession,
    remote: &str,
    bytes: &[u8],
    permissions: Option<u32>,
) -> Result<()> {
    let mut file = sftp
        .create(remote)
        .await
        .with_context(|| format!("create remote temporary file {remote}"))?;
    file.write_all(bytes)
        .await
        .with_context(|| format!("write remote temporary file {remote}"))?;
    file.flush()
        .await
        .with_context(|| format!("flush remote temporary file {remote}"))?;
    file.sync_all()
        .await
        .with_context(|| format!("sync remote temporary file {remote}"))?;
    drop(file);

    if let Some(permissions) = permissions {
        let mut attributes = FileAttributes::empty();
        attributes.permissions = Some(permissions);
        sftp.set_metadata(remote, attributes)
            .await
            .with_context(|| format!("preserve permissions on {remote}"))?;
    }
    Ok(())
}

async fn remove_remote_file_if_present(sftp: &SftpSession, remote: &str) {
    if sftp.try_exists(remote).await.unwrap_or(false) {
        let _ = sftp.remove_file(remote).await;
    }
}

async fn replace_remote_file(
    sftp: &SftpSession,
    remote: &str,
    temp: &str,
    backup: &str,
) -> Result<()> {
    match sftp.rename(temp, remote).await {
        Ok(()) => return Ok(()),
        Err(direct_error) => {
            if !sftp.try_exists(temp).await.unwrap_or(false) {
                tracing::warn!(
                    "SFTP rename reported an error after temporary file disappeared; verifying target: {direct_error}"
                );
                return Ok(());
            }
        }
    }

    sftp.rename(remote, backup)
        .await
        .with_context(|| format!("move original {remote} to recovery backup {backup}"))?;
    if let Err(replace_error) = sftp.rename(temp, remote).await {
        let rollback = sftp.rename(backup, remote).await;
        return match rollback {
            Ok(()) => Err(anyhow!(
                "replace remote {remote} failed and original file was restored: {replace_error}"
            )),
            Err(rollback_error) => Err(anyhow!(
                "replace remote {remote} failed ({replace_error}); recovery backup remains at {backup} because rollback failed ({rollback_error})"
            )),
        };
    }

    if let Err(error) = sftp.remove_file(backup).await {
        tracing::warn!("failed to remove SFTP save backup {backup}: {error}");
    }
    Ok(())
}

pub(super) async fn save_remote_text_file(
    sftp: &SftpSession,
    save: RemoteTextSave,
) -> Result<SaveRemoteTextOutcome> {
    let current = read_remote_text_file(sftp, &save.remote_path).await?;
    if !save.force && !save.expected_revision.same_content(&current.revision) {
        return Ok(SaveRemoteTextOutcome::Conflict(current));
    }

    let bytes = encode_remote_text(&save.content, save.format);
    if bytes.len() as u64 > EDITOR_HARD_LIMIT_BYTES {
        return Err(anyhow!(
            "edited file is too large to save ({} bytes, max {} bytes)",
            bytes.len(),
            EDITOR_HARD_LIMIT_BYTES
        ));
    }

    let suffix = Uuid::new_v4();
    let temp = format!("{}.tiny-shell-save-{suffix}.tmp", save.remote_path);
    let backup = format!("{}.tiny-shell-save-{suffix}.bak", save.remote_path);
    if let Err(error) =
        write_remote_temp_file(sftp, &temp, &bytes, current.revision.permissions).await
    {
        remove_remote_file_if_present(sftp, &temp).await;
        return Err(error);
    }

    let before_replace = match read_remote_text_file(sftp, &save.remote_path).await {
        Ok(file) => file,
        Err(error) => {
            remove_remote_file_if_present(sftp, &temp).await;
            return Err(error);
        }
    };
    if !save.force
        && !save
            .expected_revision
            .same_content(&before_replace.revision)
    {
        remove_remote_file_if_present(sftp, &temp).await;
        return Ok(SaveRemoteTextOutcome::Conflict(before_replace));
    }

    if let Err(error) = replace_remote_file(sftp, &save.remote_path, &temp, &backup).await {
        remove_remote_file_if_present(sftp, &temp).await;
        return Err(error);
    }

    let saved = read_remote_text_file(sftp, &save.remote_path).await?;
    let expected_saved =
        RemoteFileRevision::from_bytes(&bytes, saved.revision.modified, saved.revision.permissions);
    if !expected_saved.same_content(&saved.revision) {
        return Err(anyhow!(
            "remote save verification failed for {}",
            save.remote_path
        ));
    }
    Ok(SaveRemoteTextOutcome::Saved(saved.revision))
}

#[cfg(test)]
mod tests {
    use super::{
        EDITOR_HARD_LIMIT_BYTES, EDITOR_SOFT_LIMIT_BYTES, LineEnding, RemoteFileRevision,
        TextEncoding, decode_remote_text, encode_remote_text,
    };

    #[test]
    fn utf8_bom_and_crlf_round_trip() {
        let source = b"\xEF\xBB\xBFfirst\r\nsecond\r\n".to_vec();
        let file = decode_remote_text(source.clone(), Some(7), Some(0o100644)).unwrap();

        assert_eq!(file.content, "first\nsecond\n");
        assert_eq!(file.format.encoding, TextEncoding::Utf8Bom);
        assert_eq!(file.format.line_ending, LineEnding::Crlf);
        assert_eq!(encode_remote_text(&file.content, file.format), source);
    }

    #[test]
    fn rejects_non_utf8_content() {
        assert!(decode_remote_text(vec![0xFF, 0xFE], None, None).is_err());
    }

    #[test]
    fn marks_soft_limit_and_rejects_hard_limit() {
        let soft = vec![b'a'; EDITOR_SOFT_LIMIT_BYTES as usize + 1];
        assert!(decode_remote_text(soft, None, None).unwrap().large_file);

        let hard = vec![b'a'; EDITOR_HARD_LIMIT_BYTES as usize + 1];
        assert!(decode_remote_text(hard, None, None).is_err());
    }

    #[test]
    fn revision_compares_content_not_metadata() {
        let original = RemoteFileRevision::from_bytes(b"content", Some(1), Some(0o100644));
        let metadata_changed = RemoteFileRevision::from_bytes(b"content", Some(2), Some(0o100600));
        let content_changed = RemoteFileRevision::from_bytes(b"changed", Some(1), Some(0o100644));

        assert!(original.same_content(&metadata_changed));
        assert!(!original.same_content(&content_changed));
    }
}
