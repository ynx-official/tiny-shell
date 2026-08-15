use std::{fs, path::Path};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::session::config::{ConfigStore, TempTaskDirectory};

use super::base_name;

pub(super) enum SftpTempDirectory {
    Managed(TempTaskDirectory),
    Fallback(std::path::PathBuf),
}

impl SftpTempDirectory {
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Managed(directory) => directory.path(),
            Self::Fallback(path) => path,
        }
    }
}

impl Drop for SftpTempDirectory {
    fn drop(&mut self) {
        let Self::Fallback(path) = self else {
            return;
        };
        if let Err(error) = fs::remove_dir_all(&*path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!(path = %path.display(), %error, "failed to clean up SFTP temporary directory");
        }
    }
}

pub(super) fn allocate_sftp_temp_directory(purpose: &str) -> Result<SftpTempDirectory> {
    if let Some(workspace) = ConfigStore::temp_workspace() {
        match workspace.allocate(purpose) {
            Ok(directory) => return Ok(SftpTempDirectory::Managed(directory)),
            Err(error) => {
                tracing::warn!(%error, purpose, "falling back to the operating-system temporary directory");
            }
        }
    }

    let path = std::env::temp_dir().join("tiny-shell").join(format!(
        "{}-{}",
        sanitize_component(purpose, "task"),
        Uuid::new_v4()
    ));
    fs::create_dir_all(&path)
        .with_context(|| format!("create SFTP temporary directory {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("secure SFTP temporary directory {}", path.display()))?;
    }
    Ok(SftpTempDirectory::Fallback(path))
}

pub(super) fn safe_local_edit_name(remote_path: &str) -> String {
    sanitize_component(&base_name(remote_path), "remote-file")
}

fn sanitize_component(value: &str, fallback: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() || matches!(sanitized.as_str(), "." | "..") {
        fallback.to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::{safe_local_edit_name, sanitize_component};

    #[test]
    fn local_edit_name_keeps_only_a_safe_basename() {
        assert_eq!(
            safe_local_edit_name("/var/tmp/report 1.txt"),
            "report_1.txt"
        );
        assert_eq!(safe_local_edit_name("/tmp/.."), "remote-file");
        assert_eq!(safe_local_edit_name("/tmp/配置.yml"), "__.yml");
    }

    #[test]
    fn temporary_directory_component_cannot_inject_path_separators() {
        assert_eq!(
            sanitize_component("pack/../../escape", "task"),
            "pack_.._.._escape"
        );
        assert_eq!(sanitize_component("", "task"), "task");
    }
}
