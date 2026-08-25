use std::{future::Future, pin::Pin};

use anyhow::{Context, Result};
use russh_sftp::{client::SftpSession, protocol::FileAttributes};

use super::{PermissionApplyTarget, RemoteEntry, join_remote};

pub(super) fn recursive_delete<'a>(
    sftp: &'a SftpSession,
    path: String,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        match sftp.read_dir(&path).await {
            Ok(entries) => {
                for entry in entries {
                    let name = entry.file_name();
                    if name == "." || name == ".." {
                        continue;
                    }
                    let child_path = join_remote(&path, &name);
                    if is_directory(entry.metadata().permissions) {
                        recursive_delete(sftp, child_path).await?;
                    } else {
                        sftp.remove_file(&child_path)
                            .await
                            .with_context(|| format!("Failed to delete file {child_path}"))?;
                    }
                }
                sftp.remove_dir(&path)
                    .await
                    .with_context(|| format!("Failed to delete dir {path}"))?;
            }
            Err(_) => {
                sftp.remove_file(&path)
                    .await
                    .with_context(|| format!("Failed to delete {path}"))?;
            }
        }
        Ok(())
    })
}

pub(super) async fn set_path_permissions(sftp: &SftpSession, path: &str, mode: u32) -> Result<()> {
    let mut attributes = FileAttributes::empty();
    attributes.permissions = Some(mode);
    sftp.set_metadata(path, attributes)
        .await
        .with_context(|| format!("chmod {mode:o} {path}"))
}

pub(super) fn set_permissions_recursive<'a>(
    sftp: &'a SftpSession,
    path: String,
    mode: u32,
    apply_to: PermissionApplyTarget,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let metadata = sftp
            .metadata(&path)
            .await
            .with_context(|| format!("metadata {path}"))?;
        let is_dir = is_directory(metadata.permissions);
        if should_apply_permissions(is_dir, apply_to) {
            set_path_permissions(sftp, &path, mode).await?;
        }
        if !is_dir {
            return Ok(());
        }

        for entry in sftp
            .read_dir(&path)
            .await
            .with_context(|| format!("read_dir {path}"))?
        {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            set_permissions_recursive(sftp, join_remote(&path, &name), mode, apply_to).await?;
        }
        Ok(())
    })
}

pub(super) async fn list_dir(sftp: &SftpSession, path: &str) -> Result<Vec<RemoteEntry>> {
    let raw = sftp
        .read_dir(path)
        .await
        .with_context(|| format!("read_dir {path} failed"))?;

    let mut entries = raw
        .into_iter()
        .filter(|entry| {
            let name = entry.file_name();
            name != "." && name != ".."
        })
        .map(|entry| {
            let name = entry.file_name().to_string();
            let full_path = join_remote(path, &name);
            let metadata = entry.metadata();
            let permissions = metadata.permissions.unwrap_or(0);
            RemoteEntry {
                name,
                full_path,
                is_dir: is_directory(Some(permissions)),
                size: metadata.size.unwrap_or(0),
                modified: metadata.mtime.unwrap_or(0),
                permissions,
            }
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| match (left.is_dir, right.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    });
    Ok(entries)
}

pub(super) async fn create_remote_dir_all(sftp: &SftpSession, remote_dir: &str) -> Result<()> {
    if remote_dir.is_empty() || remote_dir == "/" {
        return Ok(());
    }

    let mut current = String::from("/");
    for segment in remote_dir.split('/').filter(|segment| !segment.is_empty()) {
        current = join_remote(&current, segment);
        if sftp.metadata(&current).await.is_ok() {
            continue;
        }
        sftp.create_dir(&current)
            .await
            .with_context(|| format!("create remote directory {current}"))?;
    }
    Ok(())
}

fn is_directory(permissions: Option<u32>) -> bool {
    permissions
        .map(|mode| (mode & 0o170_000) == 0o040_000)
        .unwrap_or(false)
}

fn should_apply_permissions(is_dir: bool, target: PermissionApplyTarget) -> bool {
    match target {
        PermissionApplyTarget::FilesAndDirectories => true,
        PermissionApplyTarget::FilesOnly => !is_dir,
        PermissionApplyTarget::DirectoriesOnly => is_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::{is_directory, should_apply_permissions};
    use crate::sftp::PermissionApplyTarget;

    #[test]
    fn file_type_detection_uses_posix_mode_bits() {
        assert!(is_directory(Some(0o040755)));
        assert!(!is_directory(Some(0o100644)));
        assert!(!is_directory(None));
    }

    #[test]
    fn recursive_permission_target_selects_expected_entry_types() {
        assert!(should_apply_permissions(
            true,
            PermissionApplyTarget::FilesAndDirectories
        ));
        assert!(should_apply_permissions(
            false,
            PermissionApplyTarget::FilesOnly
        ));
        assert!(!should_apply_permissions(
            true,
            PermissionApplyTarget::FilesOnly
        ));
        assert!(should_apply_permissions(
            true,
            PermissionApplyTarget::DirectoriesOnly
        ));
        assert!(!should_apply_permissions(
            false,
            PermissionApplyTarget::DirectoriesOnly
        ));
    }
}
