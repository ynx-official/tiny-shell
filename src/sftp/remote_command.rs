use std::time::Duration;

use anyhow::{Context, Result, anyhow};

use super::{SftpClientHandler, base_name, remote_parent, shell_quote};

pub(super) async fn create_remote_archive(
    handle: &russh::client::Handle<SftpClientHandler>,
    remote_dir: &str,
    remote_archive: &str,
) -> Result<()> {
    let command = remote_directory_archive_command(remote_dir, remote_archive);
    exec_remote_command(handle, &command)
        .await
        .with_context(|| {
            format!(
                "archive remote directory {}",
                remote_dir.trim_end_matches('/')
            )
        })
}

pub(super) async fn create_remote_paths_archive(
    handle: &russh::client::Handle<SftpClientHandler>,
    remote_paths: &[String],
    remote_archive: &str,
) -> Result<()> {
    let command = remote_paths_archive_command(remote_paths, remote_archive)?;
    exec_remote_command(handle, &command)
        .await
        .with_context(|| format!("archive {} remote paths", remote_paths.len()))
}

pub(super) async fn remove_remote_path(
    handle: &russh::client::Handle<SftpClientHandler>,
    remote_path: &str,
) -> Result<()> {
    let command = format!("rm -f -- {}", shell_quote(remote_path));
    exec_remote_command(handle, &command)
        .await
        .with_context(|| format!("remove remote temporary file {remote_path}"))
}

fn remote_directory_archive_command(remote_dir: &str, remote_archive: &str) -> String {
    let remote_dir = remote_dir.trim_end_matches('/');
    format!(
        "tar -C {} -czf {} -- {}",
        shell_quote(&remote_parent(remote_dir)),
        shell_quote(remote_archive),
        shell_quote(&base_name(remote_dir)),
    )
}

fn remote_paths_archive_command(remote_paths: &[String], remote_archive: &str) -> Result<String> {
    let first = remote_paths
        .first()
        .context("cannot archive an empty path selection")?;
    let parent = remote_parent(first);
    if remote_paths
        .iter()
        .any(|path| remote_parent(path) != parent)
    {
        return Err(anyhow!(
            "selected paths must share the same parent directory"
        ));
    }
    let names = remote_paths
        .iter()
        .map(|path| shell_quote(&base_name(path)))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(format!(
        "tar -C {} -czf {} -- {}",
        shell_quote(&parent),
        shell_quote(remote_archive),
        names
    ))
}

pub(super) async fn exec_remote_command(
    handle: &russh::client::Handle<SftpClientHandler>,
    command: &str,
) -> Result<()> {
    let mut channel = handle
        .channel_open_session()
        .await
        .context("open remote exec session")?;
    channel
        .exec(true, command)
        .await
        .with_context(|| format!("exec remote command: {command}"))?;

    let mut stderr = Vec::new();
    let mut stdout = Vec::new();
    let mut exit_status = None;
    let result = tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            tokio::task::yield_now().await;
            let Some(message) = channel.wait().await else {
                break;
            };
            match message {
                russh::ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                russh::ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
                russh::ChannelMsg::ExitStatus { exit_status: code } => exit_status = Some(code),
                russh::ChannelMsg::Close => break,
                _ => {}
            }
        }
    })
    .await;

    if result.is_err() {
        return Err(anyhow!("remote command timeout: {command}"));
    }

    match exit_status.unwrap_or(0) {
        0 => Ok(()),
        code => {
            let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&stdout).trim().to_string();
            Err(anyhow!(
                "remote command exited with {code}: {}",
                if !stderr.is_empty() { stderr } else { stdout }
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{remote_directory_archive_command, remote_paths_archive_command};

    #[test]
    fn archive_commands_quote_every_remote_argument() {
        assert_eq!(
            remote_directory_archive_command("/srv/a b/", "/tmp/out file.tar.gz"),
            "tar -C '/srv' -czf '/tmp/out file.tar.gz' -- 'a b'"
        );
        assert!(
            remote_paths_archive_command(
                &["/srv/a b".to_string(), "/srv/c'd".to_string()],
                "/tmp/out.tar.gz"
            )
            .is_ok_and(|command| {
                command == "tar -C '/srv' -czf '/tmp/out.tar.gz' -- 'a b' 'c'\"'\"'d'"
            })
        );
    }

    #[test]
    fn multi_path_archive_rejects_empty_and_mixed_parent_selections() {
        assert!(remote_paths_archive_command(&[], "/tmp/out.tar.gz").is_err());
        assert!(
            remote_paths_archive_command(
                &["/srv/a".to_string(), "/other/b".to_string()],
                "/tmp/out.tar.gz"
            )
            .is_err()
        );
    }
}
