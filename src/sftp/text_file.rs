use anyhow::{Context, Result, anyhow};
use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{
    BIG5, EUC_JP, EUC_KR, Encoding, GB18030, GBK, ISO_2022_JP, SHIFT_JIS, UTF_8, UTF_16BE,
    UTF_16LE, WINDOWS_1252,
};
use russh_sftp::{client::SftpSession, protocol::FileAttributes};
use rust_i18n::t;
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
    Utf16Le,
    Utf16Be,
    Gb18030,
    Gbk,
    Big5,
    ShiftJis,
    EucJp,
    Iso2022Jp,
    EucKr,
    Windows1252,
    Iso8859_1,
    Other(&'static Encoding),
}

impl TextEncoding {
    pub const ALL: [Self; 13] = [
        Self::Utf8,
        Self::Utf8Bom,
        Self::Utf16Le,
        Self::Utf16Be,
        Self::Gb18030,
        Self::Gbk,
        Self::Big5,
        Self::ShiftJis,
        Self::EucJp,
        Self::Iso2022Jp,
        Self::EucKr,
        Self::Windows1252,
        Self::Iso8859_1,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "Unicode (UTF-8)",
            Self::Utf8Bom => "Unicode (UTF-8 with BOM)",
            Self::Utf16Le => "Unicode (UTF-16 LE)",
            Self::Utf16Be => "Unicode (UTF-16 BE)",
            Self::Gb18030 => "GB18030",
            Self::Gbk => "GBK",
            Self::Big5 => "Big5",
            Self::ShiftJis => "Shift_JIS",
            Self::EucJp => "EUC-JP",
            Self::Iso2022Jp => "ISO-2022-JP",
            Self::EucKr => "EUC-KR / CP949",
            Self::Windows1252 => "Windows-1252",
            Self::Iso8859_1 => "ISO-8859-1",
            Self::Other(encoding) => encoding.name(),
        }
    }

    pub fn locale_key(self) -> Option<&'static str> {
        match self {
            Self::Utf8 => Some("editor_encoding_utf8"),
            Self::Utf8Bom => Some("editor_encoding_utf8_bom"),
            Self::Utf16Le => Some("editor_encoding_utf16_le"),
            Self::Utf16Be => Some("editor_encoding_utf16_be"),
            Self::Gb18030 => Some("editor_encoding_gb18030"),
            Self::Gbk => Some("editor_encoding_gbk"),
            Self::Big5 => Some("editor_encoding_big5"),
            Self::ShiftJis => Some("editor_encoding_shift_jis"),
            Self::EucJp => Some("editor_encoding_euc_jp"),
            Self::Iso2022Jp => Some("editor_encoding_iso_2022_jp"),
            Self::EucKr => Some("editor_encoding_euc_kr"),
            Self::Windows1252 => Some("editor_encoding_windows_1252"),
            Self::Iso8859_1 => Some("editor_encoding_iso_8859_1"),
            Self::Other(_) => None,
        }
    }

    fn encoding_rs(self) -> &'static Encoding {
        match self {
            Self::Utf8 | Self::Utf8Bom => UTF_8,
            Self::Utf16Le => UTF_16LE,
            Self::Utf16Be => UTF_16BE,
            Self::Gb18030 => GB18030,
            Self::Gbk => GBK,
            Self::Big5 => BIG5,
            Self::ShiftJis => SHIFT_JIS,
            Self::EucJp => EUC_JP,
            Self::Iso2022Jp => ISO_2022_JP,
            Self::EucKr => EUC_KR,
            Self::Windows1252 => WINDOWS_1252,
            Self::Iso8859_1 => WINDOWS_1252,
            Self::Other(encoding) => encoding,
        }
    }

    fn from_encoding_rs(encoding: &'static Encoding) -> Self {
        if encoding == GB18030 {
            Self::Gb18030
        } else if encoding == GBK {
            // chardetng intentionally reports GB18030 as GBK. Decode with the
            // superset so four-byte GB18030 sequences are preserved.
            Self::Gb18030
        } else if encoding == BIG5 {
            Self::Big5
        } else if encoding == SHIFT_JIS {
            Self::ShiftJis
        } else if encoding == EUC_JP {
            Self::EucJp
        } else if encoding == ISO_2022_JP {
            Self::Iso2022Jp
        } else if encoding == EUC_KR {
            Self::EucKr
        } else if encoding == UTF_16LE {
            Self::Utf16Le
        } else if encoding == UTF_16BE {
            Self::Utf16Be
        } else if encoding == WINDOWS_1252 {
            Self::Windows1252
        } else if encoding == UTF_8 {
            Self::Utf8
        } else {
            Self::Other(encoding)
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
    pub source_bytes: Vec<u8>,
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
    let encoding = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        TextEncoding::Utf8Bom
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        TextEncoding::Utf16Le
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        TextEncoding::Utf16Be
    } else if std::str::from_utf8(&bytes).is_ok() {
        TextEncoding::Utf8
    } else {
        let mut detector = EncodingDetector::new(Iso2022JpDetection::Allow);
        detector.feed(&bytes, true);
        TextEncoding::from_encoding_rs(detector.guess(None, Utf8Detection::Allow))
    };

    decode_remote_text_with_encoding(&bytes, encoding, revision)
}

pub(crate) fn decode_remote_text_with_encoding(
    bytes: &[u8],
    encoding: TextEncoding,
    revision: RemoteFileRevision,
) -> Result<RemoteTextFile> {
    if bytes.len() as u64 > EDITOR_HARD_LIMIT_BYTES {
        return Err(anyhow!(
            "file too large for in-memory editor ({} bytes, max {} bytes)",
            bytes.len(),
            EDITOR_HARD_LIMIT_BYTES
        ));
    }

    let body = match encoding {
        TextEncoding::Utf8Bom => bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes),
        TextEncoding::Utf16Le => bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes),
        TextEncoding::Utf16Be => bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes),
        _ => bytes,
    };
    let text = if encoding == TextEncoding::Iso8859_1 {
        body.iter().map(|byte| char::from(*byte)).collect()
    } else {
        encoding.encoding_rs().decode(body).0.into_owned()
    };
    let line_ending = if text.contains("\r\n") {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    };

    Ok(RemoteTextFile {
        content: text.replace("\r\n", "\n"),
        source_bytes: bytes.to_vec(),
        revision,
        format: RemoteTextFormat {
            encoding,
            line_ending,
        },
        large_file: bytes.len() as u64 > EDITOR_SOFT_LIMIT_BYTES,
    })
}

pub(crate) fn encode_remote_text(content: &str, format: RemoteTextFormat) -> Result<Vec<u8>> {
    let normalized = content.replace("\r\n", "\n");
    let body = match format.line_ending {
        LineEnding::Lf => normalized,
        LineEnding::Crlf => normalized.replace('\n', "\r\n"),
    };
    let bytes = match format.encoding {
        TextEncoding::Utf8 => body.into_bytes(),
        TextEncoding::Utf8Bom => {
            let mut bytes = Vec::with_capacity(body.len() + 3);
            bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            bytes.extend_from_slice(body.as_bytes());
            bytes
        }
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            let little_endian = format.encoding == TextEncoding::Utf16Le;
            let mut bytes = Vec::with_capacity(body.len() * 2 + 2);
            bytes.extend_from_slice(if little_endian {
                &[0xFF, 0xFE]
            } else {
                &[0xFE, 0xFF]
            });
            for unit in body.encode_utf16() {
                let encoded = if little_endian {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                };
                bytes.extend_from_slice(&encoded);
            }
            bytes
        }
        TextEncoding::Iso8859_1 => {
            let mut bytes = Vec::with_capacity(body.len());
            for character in body.chars() {
                let value = u32::from(character);
                if value > u32::from(u8::MAX) {
                    return Err(anyhow!(
                        t!(
                            "editor_encoding_unrepresentable",
                            encoding = format.encoding.label()
                        )
                        .to_string()
                    ));
                }
                bytes.push(value as u8);
            }
            bytes
        }
        encoding => {
            let (bytes, _, had_errors) = encoding.encoding_rs().encode(&body);
            if had_errors {
                return Err(anyhow!(
                    t!(
                        "editor_encoding_unrepresentable",
                        encoding = encoding.label()
                    )
                    .to_string()
                ));
            }
            bytes.into_owned()
        }
    };
    Ok(bytes)
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

    let bytes = encode_remote_text(&save.content, save.format)?;
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
        assert_eq!(
            encode_remote_text(&file.content, file.format).unwrap(),
            source
        );
    }

    #[test]
    fn detects_and_round_trips_utf16_bom() {
        let source = vec![
            0xFF, 0xFE, b'f', 0, b'i', 0, b'r', 0, b's', 0, b't', 0, b'\r', 0, b'\n', 0,
        ];
        let file = decode_remote_text(source.clone(), None, None).unwrap();

        assert_eq!(file.content, "first\n");
        assert_eq!(file.format.encoding, TextEncoding::Utf16Le);
        assert_eq!(file.format.line_ending, LineEnding::Crlf);
        assert_eq!(
            encode_remote_text(&file.content, file.format).unwrap(),
            source
        );
    }

    #[test]
    fn detects_gbk_and_redecodes_with_selected_encoding() {
        let (bytes, _, had_errors) = encoding_rs::GBK.encode("简体中文");
        assert!(!had_errors);
        let bytes = bytes.into_owned();

        let detected = decode_remote_text(bytes.clone(), None, None).unwrap();
        assert_eq!(detected.content, "简体中文");
        assert!(matches!(
            detected.format.encoding,
            TextEncoding::Gbk | TextEncoding::Gb18030
        ));

        let reopened = super::decode_remote_text_with_encoding(
            &bytes,
            TextEncoding::Gbk,
            detected.revision.clone(),
        )
        .unwrap();
        assert_eq!(reopened.content, "简体中文");
        assert_eq!(reopened.format.encoding, TextEncoding::Gbk);
    }

    #[test]
    fn selectable_encodings_round_trip_representable_text() {
        let cases = [
            (TextEncoding::Utf8, "plain 文本"),
            (TextEncoding::Utf8Bom, "plain 文本"),
            (TextEncoding::Utf16Le, "plain 文本"),
            (TextEncoding::Utf16Be, "plain 文本"),
            (TextEncoding::Gb18030, "简体中文"),
            (TextEncoding::Gbk, "简体中文"),
            (TextEncoding::Big5, "繁體中文"),
            (TextEncoding::ShiftJis, "日本語"),
            (TextEncoding::EucJp, "日本語"),
            (TextEncoding::Iso2022Jp, "日本語"),
            (TextEncoding::EucKr, "한국어"),
            (TextEncoding::Windows1252, "Western café"),
            (TextEncoding::Iso8859_1, "Western café"),
        ];

        for (encoding, text) in cases {
            let format = super::RemoteTextFormat {
                encoding,
                line_ending: LineEnding::Lf,
            };
            let bytes = encode_remote_text(text, format).unwrap();
            let revision = RemoteFileRevision::from_bytes(&bytes, None, None);
            let decoded =
                super::decode_remote_text_with_encoding(&bytes, encoding, revision).unwrap();
            assert_eq!(decoded.content, text, "failed for {}", encoding.label());
        }
    }

    #[test]
    fn rejects_saving_unrepresentable_characters_in_a_legacy_encoding() {
        let result = encode_remote_text(
            "中文",
            super::RemoteTextFormat {
                encoding: TextEncoding::Windows1252,
                line_ending: LineEnding::Lf,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn legacy_non_utf8_content_is_decoded_instead_of_rejected() {
        let file = decode_remote_text(vec![b'c', b'a', b'f', 0xE9], None, None).unwrap();
        assert_eq!(file.content, "café");
        assert_eq!(file.format.encoding, TextEncoding::Windows1252);
    }

    #[test]
    fn preserves_an_automatically_detected_encoding_outside_the_quick_pick_list() {
        let source = "Это тест кодировки символов.";
        let (bytes, _, had_errors) = encoding_rs::WINDOWS_1251.encode(source);
        assert!(!had_errors);

        let file = decode_remote_text(bytes.into_owned(), None, None).unwrap();
        assert_eq!(file.content, source);
        assert_eq!(file.format.encoding.label(), "windows-1251");
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
