use std::{fmt, sync::OnceLock, time::Duration};

use anyhow::{Context, Result};

use crate::session::{config::ConfigStore, session_types::Session};

pub trait ProxyStream:
    tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static
{
}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static> ProxyStream
    for T
{
}

#[derive(Clone)]
pub struct EnvProxy {
    pub proxy_type: String,
    pub host: String,
    pub port: Option<u16>,
    pub user: String,
    pub pass: String,
}

impl fmt::Debug for EnvProxy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvProxy")
            .field("proxy_type", &self.proxy_type)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("pass", &"<redacted>")
            .finish()
    }
}

pub static ENV_PROXY: OnceLock<Option<EnvProxy>> = OnceLock::new();

fn configured_env_proxy() -> Option<&'static EnvProxy> {
    ENV_PROXY.get().and_then(Option::as_ref)
}

async fn read_http_connect_response(stream: &mut tokio::net::TcpStream) -> Result<()> {
    const MAX_RESPONSE_BYTES: usize = 16 * 1024;
    let mut response = Vec::with_capacity(256);
    let mut byte = [0_u8; 1];
    while response.len() < MAX_RESPONSE_BYTES {
        let read = tokio::io::AsyncReadExt::read(stream, &mut byte).await?;
        if read == 0 {
            break;
        }
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            return parse_http_connect_status(&response);
        }
    }

    Err(anyhow::anyhow!(
        "HTTP proxy returned an incomplete or oversized CONNECT response"
    ))
}

fn valid_http_version(version: &str) -> bool {
    let Some(version) = version.strip_prefix("HTTP/") else {
        return false;
    };
    let Some((major, minor)) = version.split_once('.') else {
        return false;
    };
    !major.is_empty()
        && major.bytes().all(|byte| byte.is_ascii_digit())
        && !minor.is_empty()
        && minor.bytes().all(|byte| byte.is_ascii_digit())
}

fn parse_http_connect_status(response: &[u8]) -> Result<()> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("HTTP proxy returned an incomplete CONNECT response"))?;
    let status_line_end = response[..header_end + 2]
        .windows(2)
        .position(|window| window == b"\r\n")
        .ok_or_else(|| anyhow::anyhow!("HTTP proxy returned an invalid CONNECT status line"))?;
    let status_line = std::str::from_utf8(&response[..status_line_end])
        .context("HTTP proxy returned a non-UTF-8 CONNECT status line")?;
    let mut fields = status_line.split_ascii_whitespace();
    let version = fields.next().unwrap_or_default();
    let status = fields
        .next()
        .ok_or_else(|| anyhow::anyhow!("HTTP proxy returned an invalid CONNECT status line"))?
        .parse::<u16>()
        .context("HTTP proxy returned an invalid CONNECT status code")?;
    if !valid_http_version(version) || !(200..300).contains(&status) {
        return Err(anyhow::anyhow!(
            "HTTP proxy CONNECT failed with status {status}"
        ));
    }
    Ok(())
}

pub async fn connect_proxy(
    session: &Session,
    config: &ConfigStore,
) -> Result<Box<dyn ProxyStream>> {
    let target_host = session.host.clone();
    let target_port = session.port;
    let session = session.clone();

    let connect_fut = async move {
        let target_host = &target_host;
        let (proxy_type, proxy_host, proxy_port, proxy_user, proxy_password) = {
            if !session.proxy_type.is_empty() && session.proxy_type != "none" {
                (
                    session.proxy_type.clone(),
                    session.proxy_host.clone(),
                    session.proxy_port,
                    session.proxy_user.clone(),
                    session.proxy_password.clone(),
                )
            } else if config.cache.read_env_proxy
                && let Some(env_p) = configured_env_proxy()
            {
                (
                    env_p.proxy_type.clone(),
                    env_p.host.clone(),
                    env_p.port,
                    env_p.user.clone(),
                    env_p.pass.clone(),
                )
            } else if config.cache.use_proxy {
                (
                    config.cache.global_proxy_type.clone(),
                    config.cache.global_proxy_host.clone(),
                    config.cache.global_proxy_port,
                    config.cache.global_proxy_user.clone(),
                    config.cache.global_proxy_password.clone(),
                )
            } else {
                (
                    "none".to_string(),
                    String::new(),
                    None,
                    String::new(),
                    String::new(),
                )
            }
        };

        if proxy_type != "none" && (proxy_host.is_empty() || proxy_port.is_none()) {
            let addr = format!("{}:{}", target_host, target_port);
            let stream = tokio::net::TcpStream::connect(&addr).await?;
            return Ok(Box::new(stream) as Box<dyn ProxyStream>);
        }

        match proxy_type.as_str() {
            "socks5" | "socks5h" => {
                let proxy_port = proxy_port.unwrap_or(1080);
                let proxy_addr = format!("{}:{}", proxy_host, proxy_port);

                if !proxy_user.is_empty() {
                    let stream = tokio_socks::tcp::Socks5Stream::connect_with_password(
                        proxy_addr.as_str(),
                        (target_host.as_str(), target_port),
                        &proxy_user,
                        &proxy_password,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("SOCKS5 proxy connection failed: {}", e))?;
                    Ok(Box::new(stream) as Box<dyn ProxyStream>)
                } else {
                    let stream = tokio_socks::tcp::Socks5Stream::connect(
                        proxy_addr.as_str(),
                        (target_host.as_str(), target_port),
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("SOCKS5 proxy connection failed: {}", e))?;
                    Ok(Box::new(stream) as Box<dyn ProxyStream>)
                }
            }
            "http" => {
                let proxy_port = proxy_port.unwrap_or(8080);
                let proxy_addr = format!("{}:{}", proxy_host, proxy_port);

                use tokio::io::AsyncWriteExt;
                let mut stream = tokio::net::TcpStream::connect(&proxy_addr)
                    .await
                    .map_err(|e| anyhow::anyhow!("HTTP proxy connection failed: {}", e))?;

                let mut request = format!(
                    "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n",
                    target_host, target_port, target_host, target_port
                );
                if !proxy_user.is_empty() {
                    use base64::Engine as _;
                    let auth = format!("{}:{}", proxy_user, proxy_password);
                    let encoded = base64::engine::general_purpose::STANDARD.encode(auth);
                    request.push_str(&format!("Proxy-Authorization: Basic {}\r\n", encoded));
                }
                request.push_str("\r\n");

                stream.write_all(request.as_bytes()).await?;

                read_http_connect_response(&mut stream).await?;

                Ok(Box::new(stream) as Box<dyn ProxyStream>)
            }
            _ => {
                let addr = format!("{}:{}", target_host, target_port);
                let stream = tokio::net::TcpStream::connect(&addr).await?;
                Ok(Box::new(stream) as Box<dyn ProxyStream>)
            }
        }
    };

    tokio::time::timeout(Duration::from_secs(16), connect_fut)
        .await
        .map_err(|_| anyhow::anyhow!("connection timed out after 16 seconds"))?
}

pub fn active_proxy(
    session: &Session,
    config: &ConfigStore,
) -> Option<(String, String, Option<u16>)> {
    let (proxy_type, proxy_host, proxy_port, _, _) = {
        if !session.proxy_type.is_empty() && session.proxy_type != "none" {
            (
                session.proxy_type.clone(),
                session.proxy_host.clone(),
                session.proxy_port,
                session.proxy_user.clone(),
                session.proxy_password.clone(),
            )
        } else if config.cache.read_env_proxy
            && let Some(env_p) = configured_env_proxy()
        {
            (
                env_p.proxy_type.clone(),
                env_p.host.clone(),
                env_p.port,
                env_p.user.clone(),
                env_p.pass.clone(),
            )
        } else if config.cache.use_proxy {
            (
                config.cache.global_proxy_type.clone(),
                config.cache.global_proxy_host.clone(),
                config.cache.global_proxy_port,
                config.cache.global_proxy_user.clone(),
                config.cache.global_proxy_password.clone(),
            )
        } else {
            (
                "none".to_string(),
                String::new(),
                None,
                String::new(),
                String::new(),
            )
        }
    };

    if proxy_type != "none" && !proxy_host.is_empty() && proxy_port.is_some() {
        Some((proxy_type, proxy_host, proxy_port))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_connect_status_requires_a_success_status_code() {
        assert!(parse_http_connect_status(b"HTTP/1.1 200 Connection Established\r\n\r\n").is_ok());
        assert!(parse_http_connect_status(b"HTTP/1.1 204 No Content\r\n\r\n").is_ok());
        assert!(
            parse_http_connect_status(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                .is_err()
        );
        assert!(parse_http_connect_status(b"HTTP/1.1 500 200 in body\r\n\r\n").is_err());
    }

    #[test]
    fn http_connect_status_rejects_malformed_responses() {
        assert!(parse_http_connect_status(b"200 Connection Established\r\n\r\n").is_err());
        assert!(parse_http_connect_status(b"HTTP/1.1 OK\r\n\r\n").is_err());
        assert!(parse_http_connect_status(b"HTTP/1.1 200 Connection Established").is_err());
        assert!(parse_http_connect_status(b"HTTP/garbage 200 OK\r\n\r\n").is_err());
        assert!(parse_http_connect_status(b"HTTP/ 200 OK\r\n\r\n").is_err());
        assert!(parse_http_connect_status(b"HTTP/1 200 OK\r\n\r\n").is_err());
    }
}
