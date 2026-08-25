use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use directories::BaseDirs;
use russh::{
    Disconnect,
    client::{self},
    keys::{PrivateKey, decode_secret_key, load_secret_key},
};
use rust_i18n::t;

use crate::session::{
    config::{AuthMethod, ConfigStore, Session},
    ssh_keys::{
        authenticate_with_default_keys, normalize_inline_private_key, private_keys_with_algs,
        session_has_explicit_key,
    },
};

use super::SftpClientHandler;

pub(super) async fn connect_and_authenticate(
    session: &Session,
    proxy_config: &ConfigStore,
) -> Result<Arc<russh::client::Handle<SftpClientHandler>>> {
    const CONNECTION_TIMEOUT: Duration = Duration::from_secs(60);

    tokio::time::timeout(CONNECTION_TIMEOUT, async move {
        if session.requires_credential_prompt() {
            return Err(anyhow!(t!("session_credentials_required").to_string()));
        }

        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(600)),
            keepalive_interval: Some(Duration::from_secs(3)),
            keepalive_max: 2,
            ..Default::default()
        });
        let addr = format!("{}:{}", session.host, session.port);
        let handler = SftpClientHandler::new(&session.host, session.port)?;
        let stream = crate::session::config::connect_proxy(session, proxy_config).await?;
        let mut handle = client::connect_stream(config, stream, handler)
            .await
            .with_context(|| format!("connect {addr} failed"))?;

        let authed = match session.auth {
            AuthMethod::Password => handle
                .authenticate_password(&session.user, &session.password)
                .await
                .context("password authentication failed")?,
            AuthMethod::Key => {
                if session_has_explicit_key(session) {
                    authenticate_explicit_key(&mut handle, session, "public key").await?
                } else {
                    let success = authenticate_default_keys(&mut handle, session).await?;
                    if !success {
                        return Err(anyhow!(
                            "public key authentication failed for {}@{}:{} - no valid default key found in ~/.ssh/",
                            session.user,
                            session.host,
                            session.port
                        ));
                    }
                    success
                }
            }
            AuthMethod::KeyPending => {
                return Err(anyhow!(t!("session_credentials_required").to_string()));
            }
            AuthMethod::Config => {
                if !session.private_key_path.trim().is_empty() {
                    authenticate_explicit_key(&mut handle, session, "ssh-config key").await?
                } else {
                    let success = authenticate_default_keys(&mut handle, session).await?;
                    if !success {
                        return Err(anyhow!(
                            "ssh-config authentication failed for {}@{}:{} - no valid default key found",
                            session.user,
                            session.host,
                            session.port
                        ));
                    }
                    success
                }
            }
        };

        if !authed {
            let _ = handle
                .disconnect(Disconnect::ByApplication, "auth failed", "")
                .await;
            return Err(anyhow!(
                "authentication failed: server rejected {} authentication for {}@{}:{}",
                match session.auth {
                    AuthMethod::Password => "password",
                    AuthMethod::Key | AuthMethod::KeyPending => "public key",
                    AuthMethod::Config => "ssh-config",
                },
                session.user,
                session.host,
                session.port
            ));
        }

        Ok(Arc::new(handle))
    })
    .await
    .context("connection timed out")?
}

async fn authenticate_explicit_key(
    handle: &mut russh::client::Handle<SftpClientHandler>,
    session: &Session,
    label: &str,
) -> Result<bool> {
    let keypair = load_session_private_key(session)?;
    let keys = private_keys_with_algs(keypair).context("invalid private key")?;
    for key in keys {
        match handle.authenticate_publickey(&session.user, key).await {
            Ok(true) => return Ok(true),
            Ok(false) => {
                tracing::debug!("[sftp] public key auth failed with algorithm, trying next");
            }
            Err(error) => {
                tracing::debug!(%error, "[sftp] public key auth error, trying next");
            }
        }
    }
    Err(anyhow!(
        "{label} authentication failed for {}@{}:{}",
        session.user,
        session.host,
        session.port
    ))
}

async fn authenticate_default_keys(
    handle: &mut russh::client::Handle<SftpClientHandler>,
    session: &Session,
) -> Result<bool> {
    let passphrase = session.passphrase.trim();
    let passphrase = (!passphrase.is_empty()).then_some(passphrase);
    authenticate_with_default_keys(handle, &session.user, passphrase).await
}

fn load_session_private_key(session: &Session) -> Result<PrivateKey> {
    let inline_key = normalize_inline_private_key(&session.private_key_inline);
    let key_path = expand_key_path(session.private_key_path.trim());
    let passphrase = session.passphrase.trim();
    let passphrase = (!passphrase.is_empty()).then_some(passphrase);
    let has_inline = !inline_key.is_empty();
    let has_path = key_path.is_some();

    if !has_inline && !has_path {
        return Err(anyhow!("private key content or path is required"));
    }

    let mut errors = Vec::new();
    if has_inline {
        match decode_secret_key(&inline_key, passphrase) {
            Ok(key) => return Ok(key),
            Err(error) => errors.push(format!("decode private key content: {error}")),
        }
    }
    if let Some(path) = key_path {
        match load_secret_key(path.as_path(), passphrase) {
            Ok(key) => return Ok(key),
            Err(error) => errors.push(format!("load key {}: {error}", path.display())),
        }
    }
    Err(anyhow!(errors.join("; ")))
}

fn expand_key_path(value: &str) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    if value == "~" {
        return BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return BaseDirs::new().map(|dirs| dirs.home_dir().join(rest));
    }
    Some(PathBuf::from(value))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use directories::BaseDirs;

    use super::expand_key_path;

    #[test]
    fn key_path_expansion_preserves_absolute_and_relative_paths() {
        assert_eq!(expand_key_path(""), None);
        assert_eq!(
            expand_key_path("/tmp/id_ed25519"),
            Some(PathBuf::from("/tmp/id_ed25519"))
        );
        assert_eq!(
            expand_key_path("keys/id_ed25519"),
            Some(PathBuf::from("keys/id_ed25519"))
        );
    }

    #[test]
    fn tilde_key_paths_expand_under_the_home_directory() {
        let Some(home) = BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) else {
            return;
        };
        assert_eq!(expand_key_path("~"), Some(home.clone()));
        assert_eq!(
            expand_key_path("~/.ssh/id_ed25519"),
            Some(home.join(".ssh/id_ed25519"))
        );
    }
}
