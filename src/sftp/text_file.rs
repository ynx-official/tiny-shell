use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};

pub const EDITOR_SOFT_LIMIT_BYTES: u64 = 1024 * 1024;
pub const EDITOR_HARD_LIMIT_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFileRevision {
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
pub enum TextEncoding {
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
pub enum LineEnding {
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
pub struct RemoteTextFormat {
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
}

#[derive(Debug, Clone)]
pub struct RemoteTextFile {
    pub content: String,
    pub revision: RemoteFileRevision,
    pub format: RemoteTextFormat,
    pub large_file: bool,
}

#[derive(Debug, Clone)]
pub struct RemoteTextSave {
    pub remote_path: String,
    pub content: String,
    pub expected_revision: RemoteFileRevision,
    pub format: RemoteTextFormat,
    pub force: bool,
}

pub fn decode_remote_text(
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

pub fn encode_remote_text(content: &str, format: RemoteTextFormat) -> Vec<u8> {
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
