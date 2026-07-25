use std::fmt;

use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use hmac::{Hmac, Mac};
use rand::{RngCore, rngs::OsRng};
use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::crypto;
use crate::session::config::{ManagedKey, Session};

const SYNC_FILE_NAME: &str = "tiny-shell-sync.json";
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPayload {
    pub schema_version: u32,
    pub revision: String,
    pub updated_at: String,
    pub device_id: String,
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub managed_keys: Vec<ManagedKey>,
}

impl SyncPayload {
    pub fn new(device_id: String, sessions: Vec<Session>, managed_keys: Vec<ManagedKey>) -> Self {
        Self {
            schema_version: FORMAT_VERSION,
            revision: Uuid::new_v4().to_string(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            device_id,
            sessions,
            managed_keys,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedEnvelope {
    format_version: u32,
    kdf: String,
    cipher: String,
    salt: String,
    nonce: String,
    payload: String,
}

#[derive(Clone)]
pub struct SyncCredentials {
    pub backend: SyncBackendCredentials,
    pub encryption_password: String,
}

#[derive(Clone)]
pub enum SyncBackendCredentials {
    WebDav {
        endpoint: String,
        username: String,
        password: String,
    },
    S3 {
        endpoint: String,
        region: String,
        bucket: String,
        object_key: String,
        access_key: String,
        secret_key: String,
        session_token: String,
    },
}

#[derive(Clone)]
pub enum SyncResult {
    Uploaded {
        etag: Option<String>,
    },
    Downloaded {
        sessions: Vec<Session>,
        managed_keys: Vec<ManagedKey>,
        etag: Option<String>,
        /// 本次下载实际解密覆盖的字段数（远端密文被解密的次数）。
        decrypted_count: u32,
    },
    /// 本地强行重置隐私密码成功，需把新密码硬件绑定落盘。
    PrivacyPasswordReset {
        new_password: String,
    },
    Failed(String),
}

impl fmt::Debug for SyncResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uploaded { etag } => formatter
                .debug_struct("Uploaded")
                .field("etag", etag)
                .finish(),
            Self::Downloaded {
                sessions,
                managed_keys,
                etag,
                decrypted_count,
            } => formatter
                .debug_struct("Downloaded")
                .field("session_count", &sessions.len())
                .field("managed_key_count", &managed_keys.len())
                .field("etag", etag)
                .field("decrypted_count", decrypted_count)
                .finish(),
            Self::PrivacyPasswordReset { .. } => formatter
                .debug_struct("PrivacyPasswordReset")
                .field("new_password", &"<redacted>")
                .finish(),
            Self::Failed(_) => formatter.write_str("Failed(<redacted>)"),
        }
    }
}

/// 上传时的并发控制模式。
pub enum UploadMode {
    /// 基于已知 etag 的条件上传：
    /// - `Some(etag)` → `If-Match: <etag>`，远端未变才覆盖
    /// - `None` → `If-None-Match: *`，仅当远端不存在时创建
    Conditional { expected_etag: Option<String> },
    /// 强制覆盖远端，忽略当前 etag。
    /// 用于"重置隐私密码"等需要无条件替换远端密文的场景。
    Force,
}

impl UploadMode {
    /// 便捷构造：使用本地已记录的 etag 做条件上传。
    pub fn conditional(expected_etag: Option<String>) -> Self {
        UploadMode::Conditional { expected_etag }
    }
}

pub async fn upload(
    credentials: SyncCredentials,
    payload: SyncPayload,
    mode: UploadMode,
) -> Result<Option<String>> {
    validate_credentials(&credentials)?;
    let body = encrypt_payload(&payload, &credentials.encryption_password)?;
    match credentials.backend {
        SyncBackendCredentials::WebDav {
            endpoint,
            username,
            password,
        } => upload_webdav(&endpoint, &username, &password, body, mode).await,
        SyncBackendCredentials::S3 {
            endpoint,
            region,
            bucket,
            object_key,
            access_key,
            secret_key,
            session_token,
        } => {
            let config = S3Config {
                endpoint,
                region,
                bucket,
                object_key,
                access_key,
                secret_key,
                session_token,
            };
            upload_s3(&config, body, mode).await
        }
    }
}

async fn upload_webdav(
    endpoint: &str,
    username: &str,
    password: &str,
    body: Vec<u8>,
    mode: UploadMode,
) -> Result<Option<String>> {
    let client = Client::new();
    let mut request = client
        .put(sync_url(endpoint))
        .basic_auth(username, Some(password))
        .header(header::CONTENT_TYPE, "application/json")
        .body(body);
    request = match mode {
        UploadMode::Conditional {
            expected_etag: Some(etag),
        } => request.header(header::IF_MATCH, etag),
        UploadMode::Conditional {
            expected_etag: None,
        } => {
            // An uninitialized client may only create a new remote file. This keeps
            // it from silently replacing configuration uploaded by another device.
            request.header(header::IF_NONE_MATCH, "*")
        }
        UploadMode::Force => {
            // 不带条件头，WebDAV PUT 默认覆盖已存在资源。
            request
        }
    };
    let response = request.send().await.context("send WebDAV upload")?;
    if response.status() == StatusCode::PRECONDITION_FAILED
        || response.status() == StatusCode::CONFLICT
    {
        return Err(anyhow!(
            "remote configuration changed; download it before uploading"
        ));
    }
    if !response.status().is_success() {
        return Err(anyhow!("WebDAV upload failed: HTTP {}", response.status()));
    }
    Ok(response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string))
}

pub async fn download(credentials: SyncCredentials) -> Result<(SyncPayload, Option<String>)> {
    validate_credentials(&credentials)?;
    let encryption_password = credentials.encryption_password;
    let (body, etag) = match credentials.backend {
        SyncBackendCredentials::WebDav {
            endpoint,
            username,
            password,
        } => download_webdav(&endpoint, &username, &password).await?,
        SyncBackendCredentials::S3 {
            endpoint,
            region,
            bucket,
            object_key,
            access_key,
            secret_key,
            session_token,
        } => {
            let config = S3Config {
                endpoint,
                region,
                bucket,
                object_key,
                access_key,
                secret_key,
                session_token,
            };
            download_s3(&config).await?
        }
    };
    let payload = decrypt_payload(&body, &encryption_password)?;
    Ok((payload, etag))
}

async fn download_webdav(
    endpoint: &str,
    username: &str,
    password: &str,
) -> Result<(Vec<u8>, Option<String>)> {
    let response = Client::new()
        .get(sync_url(endpoint))
        .basic_auth(username, Some(password))
        .send()
        .await
        .context("send WebDAV download")?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(anyhow!("no remote configuration exists yet"));
    }
    if !response.status().is_success() {
        return Err(anyhow!(
            "WebDAV download failed: HTTP {}",
            response.status()
        ));
    }
    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response
        .bytes()
        .await
        .context("read WebDAV response")?
        .to_vec();
    Ok((body, etag))
}

fn validate_credentials(credentials: &SyncCredentials) -> Result<()> {
    if credentials.encryption_password.len() < 8 {
        return Err(anyhow!(
            "encryption password must contain at least 8 characters"
        ));
    }
    match &credentials.backend {
        SyncBackendCredentials::WebDav { endpoint, .. } if endpoint.trim().is_empty() => {
            Err(anyhow!("WebDAV endpoint is required"))
        }
        SyncBackendCredentials::S3 {
            region,
            bucket,
            access_key,
            secret_key,
            ..
        } if region.trim().is_empty()
            || bucket.trim().is_empty()
            || access_key.trim().is_empty()
            || secret_key.is_empty() =>
        {
            Err(anyhow!(
                "S3 region, bucket, access key and secret key are required"
            ))
        }
        _ => Ok(()),
    }
}

struct S3Config {
    endpoint: String,
    region: String,
    bucket: String,
    object_key: String,
    access_key: String,
    secret_key: String,
    session_token: String,
}

async fn upload_s3(config: &S3Config, body: Vec<u8>, mode: UploadMode) -> Result<Option<String>> {
    let url = s3_url(config)?;
    let mut headers = signed_s3_headers("PUT", &url, &body, config)?;
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    match mode {
        UploadMode::Conditional {
            expected_etag: Some(etag),
        } => {
            headers.insert(header::IF_MATCH, header_value(&etag, "S3 ETag")?);
        }
        UploadMode::Conditional {
            expected_etag: None,
        } => {
            headers.insert(header::IF_NONE_MATCH, header::HeaderValue::from_static("*"));
        }
        UploadMode::Force => {
            // S3 PUT 本身就是覆盖语义，无需额外条件头。
        }
    }
    let response = Client::new()
        .put(url)
        .headers(headers)
        .body(body)
        .send()
        .await
        .context("send S3 upload")?;
    if response.status() == StatusCode::PRECONDITION_FAILED
        || response.status() == StatusCode::CONFLICT
    {
        return Err(anyhow!(
            "remote configuration changed; download it before uploading"
        ));
    }
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(anyhow!("S3 upload failed: HTTP {status}: {detail}"));
    }
    Ok(response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string))
}

async fn download_s3(config: &S3Config) -> Result<(Vec<u8>, Option<String>)> {
    let url = s3_url(config)?;
    let headers = signed_s3_headers("GET", &url, &[], config)?;
    let response = Client::new()
        .get(url)
        .headers(headers)
        .send()
        .await
        .context("send S3 download")?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(anyhow!("no remote configuration exists yet"));
    }
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(anyhow!("S3 download failed: HTTP {status}: {detail}"));
    }
    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response.bytes().await.context("read S3 response")?.to_vec();
    Ok((body, etag))
}

fn s3_url(config: &S3Config) -> Result<reqwest::Url> {
    let endpoint = if config.endpoint.trim().is_empty() {
        format!("https://s3.{}.amazonaws.com", config.region.trim())
    } else {
        config.endpoint.trim().trim_end_matches('/').to_string()
    };
    let key = if config.object_key.trim().is_empty() {
        SYNC_FILE_NAME
    } else {
        config.object_key.trim().trim_start_matches('/')
    };
    let url = format!(
        "{}/{}/{}",
        endpoint,
        aws_uri_encode(config.bucket.trim(), true),
        aws_uri_encode(key, false)
    );
    reqwest::Url::parse(&url).context("parse S3 object URL")
}

fn signed_s3_headers(
    method: &str,
    url: &reqwest::Url,
    body: &[u8],
    config: &S3Config,
) -> Result<header::HeaderMap> {
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("S3 endpoint has no host"))?;
    let host = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    let payload_hash = hex_sha256(body);
    let mut canonical_headers =
        format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
    let mut signed_headers = "host;x-amz-content-sha256;x-amz-date".to_string();
    if !config.session_token.is_empty() {
        canonical_headers.push_str(&format!(
            "x-amz-security-token:{}\n",
            config.session_token.trim()
        ));
        signed_headers.push_str(";x-amz-security-token");
    }
    let canonical_request = format!(
        "{method}\n{}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        url.path()
    );
    let scope = format!("{date}/{}/s3/aws4_request", config.region.trim());
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex_sha256(canonical_request.as_bytes())
    );
    let date_key = hmac_sha256(
        format!("AWS4{}", config.secret_key).as_bytes(),
        date.as_bytes(),
    )?;
    let region_key = hmac_sha256(&date_key, config.region.trim().as_bytes())?;
    let service_key = hmac_sha256(&region_key, b"s3")?;
    let signing_key = hmac_sha256(&service_key, b"aws4_request")?;
    let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes())?);
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        config.access_key.trim()
    );
    let mut headers = header::HeaderMap::new();
    headers.insert("x-amz-date", header_value(&amz_date, "S3 date")?);
    headers.insert(
        "x-amz-content-sha256",
        header_value(&payload_hash, "S3 payload hash")?,
    );
    headers.insert(
        header::AUTHORIZATION,
        header_value(&authorization, "S3 authorization")?,
    );
    if !config.session_token.is_empty() {
        headers.insert(
            "x-amz-security-token",
            header_value(config.session_token.trim(), "S3 session token")?,
        );
    }
    Ok(headers)
}

fn header_value(value: &str, name: &str) -> Result<header::HeaderValue> {
    header::HeaderValue::from_str(value).with_context(|| format!("invalid {name}"))
}

fn hmac_sha256(key: &[u8], value: &[u8]) -> Result<Vec<u8>> {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(key).map_err(|_| anyhow!("initialize S3 signer"))?;
    mac.update(value);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn hex_sha256(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

fn aws_uri_encode(value: &str, encode_slash: bool) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (!encode_slash && byte == b'/')
        {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn sync_url(endpoint: &str) -> String {
    let endpoint = endpoint.trim();
    if endpoint.ends_with('/') {
        format!("{endpoint}{SYNC_FILE_NAME}")
    } else if endpoint.ends_with(".json") {
        endpoint.to_string()
    } else {
        format!("{endpoint}/{SYNC_FILE_NAME}")
    }
}

fn encrypt_payload(payload: &SyncPayload, password: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let key = crypto::derive_key(password, &salt)?;
    let plaintext = serde_json::to_vec(payload).context("serialize sync payload")?;
    let ciphertext = XChaCha20Poly1305::new((&key).into())
        .encrypt(XNonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| anyhow!("encrypt sync payload"))?;
    serde_json::to_vec_pretty(&EncryptedEnvelope {
        format_version: FORMAT_VERSION,
        kdf: "argon2id".to_string(),
        cipher: "xchacha20poly1305".to_string(),
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        payload: STANDARD.encode(ciphertext),
    })
    .context("serialize encrypted sync envelope")
}

fn decrypt_payload(raw: &[u8], password: &str) -> Result<SyncPayload> {
    let envelope: EncryptedEnvelope =
        serde_json::from_slice(raw).context("parse encrypted sync envelope")?;
    if envelope.format_version != FORMAT_VERSION
        || envelope.kdf != "argon2id"
        || envelope.cipher != "xchacha20poly1305"
    {
        return Err(anyhow!("unsupported remote sync format"));
    }
    let salt = STANDARD.decode(envelope.salt).context("decode sync salt")?;
    let nonce = STANDARD
        .decode(envelope.nonce)
        .context("decode sync nonce")?;
    if nonce.len() != 24 {
        return Err(anyhow!("invalid sync nonce"));
    }
    let ciphertext = STANDARD
        .decode(envelope.payload)
        .context("decode encrypted sync payload")?;
    let key = crypto::derive_key(password, &salt)?;
    let plaintext = XChaCha20Poly1305::new((&key).into())
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| anyhow!("cannot decrypt remote configuration; check the password"))?;
    let payload: SyncPayload =
        serde_json::from_slice(&plaintext).context("parse decrypted sync payload")?;
    if payload.schema_version != FORMAT_VERSION {
        return Err(anyhow!("unsupported synchronized configuration version"));
    }
    Ok(payload)
}

/// 对上传到同步后端的快照做敏感字段处理。
///
/// - `include_secrets=false`：把所有敏感字段清空，远端只保留连接骨架。
///   下载时这些空字段会被 `merge` 保留为本地原值。
/// - `include_secrets=true`：用隐私密码对每个敏感字段做字段级加密，
///   密文带 `v1:` 前缀，下载时由 `merge` 解密还原。
pub struct SecretScrubber;

impl SecretScrubber {
    /// 处理一份 sessions + managed_keys 副本，返回脱敏或加密后的快照。
    pub fn scrub(
        mut sessions: Vec<Session>,
        mut managed_keys: Vec<ManagedKey>,
        include_secrets: bool,
        privacy_password: &str,
    ) -> Result<(Vec<Session>, Vec<ManagedKey>)> {
        if !include_secrets {
            for s in &mut sessions {
                s.password.clear();
                s.passphrase.clear();
                s.private_key_inline.clear();
                s.proxy_password.clear();
            }
            for k in &mut managed_keys {
                k.inline_content.clear();
                k.passphrase.clear();
            }
            return Ok((sessions, managed_keys));
        }

        for s in &mut sessions {
            s.password = crypto::encrypt_field(&s.password, privacy_password)?;
            s.passphrase = crypto::encrypt_field(&s.passphrase, privacy_password)?;
            s.private_key_inline = crypto::encrypt_field(&s.private_key_inline, privacy_password)?;
            s.proxy_password = crypto::encrypt_field(&s.proxy_password, privacy_password)?;
        }
        for k in &mut managed_keys {
            k.inline_content = crypto::encrypt_field(&k.inline_content, privacy_password)?;
            k.passphrase = crypto::encrypt_field(&k.passphrase, privacy_password)?;
        }
        Ok((sessions, managed_keys))
    }

    /// 把远端下载的 sessions/managed_keys 与本地合并。
    ///
    /// 合并规则（按字段）：
    /// - 远端字段为空串 → 保留本地原值（远端未勾选同步密码时）
    /// - 远端字段为密文（`v1:` 前缀）→ 用隐私密码解密后覆盖本地
    /// - 远端字段为明文 → 直接覆盖本地（兼容旧版未加密 payload）
    ///
    /// session 按 `host+port+user` 匹配，managed_keys 按 `fingerprint` 匹配。
    /// 仅本地存在的 session/key 保留不动。
    pub fn merge(
        local_sessions: &[Session],
        remote_sessions: Vec<Session>,
        local_keys: &[ManagedKey],
        remote_keys: Vec<ManagedKey>,
        privacy_password: &str,
    ) -> Result<MergedSecrets> {
        let mut merged: Vec<Session> = Vec::with_capacity(local_sessions.len());
        let mut decrypted_count: u32 = 0;

        // 本地 session 先入队（保留顺序），远端匹配项覆盖字段
        for mut local in local_sessions.iter().cloned() {
            if let Some(remote) = find_remote_session(&remote_sessions, &local) {
                merge_session_fields(&mut local, remote, privacy_password, &mut decrypted_count)?;
            }
            merged.push(local);
        }
        // 远端新增的 session（本地无匹配）追加到末尾
        for remote in remote_sessions {
            if !merged.iter().any(|s| sessions_match(s, &remote)) {
                merged.push(remote);
            }
        }

        let mut merged_keys: Vec<ManagedKey> = Vec::with_capacity(local_keys.len());
        for mut local in local_keys.iter().cloned() {
            if let Some(remote) = remote_keys
                .iter()
                .find(|r| r.fingerprint == local.fingerprint)
            {
                merge_key_fields(&mut local, remote, privacy_password, &mut decrypted_count)?;
            }
            merged_keys.push(local);
        }
        for remote in remote_keys {
            if !merged_keys
                .iter()
                .any(|k| k.fingerprint == remote.fingerprint)
            {
                merged_keys.push(remote);
            }
        }

        Ok(MergedSecrets {
            sessions: merged,
            managed_keys: merged_keys,
            decrypted_count,
        })
    }
}

/// 合并结果。
pub struct MergedSecrets {
    pub sessions: Vec<Session>,
    pub managed_keys: Vec<ManagedKey>,
    /// 本次实际解密覆盖的字段数（不含保留本地的空字段）。
    pub decrypted_count: u32,
}

fn sessions_match(a: &Session, b: &Session) -> bool {
    a.host == b.host && a.port == b.port && a.user == b.user
}

fn find_remote_session<'a>(remote: &'a [Session], local: &Session) -> Option<&'a Session> {
    remote.iter().find(|r| sessions_match(r, local))
}

fn merge_session_fields(
    local: &mut Session,
    remote: &Session,
    privacy_password: &str,
    decrypted_count: &mut u32,
) -> Result<()> {
    local.password = merge_field(
        &local.password,
        &remote.password,
        privacy_password,
        decrypted_count,
    )?;
    local.passphrase = merge_field(
        &local.passphrase,
        &remote.passphrase,
        privacy_password,
        decrypted_count,
    )?;
    local.private_key_inline = merge_field(
        &local.private_key_inline,
        &remote.private_key_inline,
        privacy_password,
        decrypted_count,
    )?;
    local.proxy_password = merge_field(
        &local.proxy_password,
        &remote.proxy_password,
        privacy_password,
        decrypted_count,
    )?;
    Ok(())
}

fn merge_key_fields(
    local: &mut ManagedKey,
    remote: &ManagedKey,
    privacy_password: &str,
    decrypted_count: &mut u32,
) -> Result<()> {
    local.inline_content = merge_field(
        &local.inline_content,
        &remote.inline_content,
        privacy_password,
        decrypted_count,
    )?;
    local.passphrase = merge_field(
        &local.passphrase,
        &remote.passphrase,
        privacy_password,
        decrypted_count,
    )?;
    Ok(())
}

/// 单字段合并：空串保留本地，密文解密覆盖，明文直接覆盖。
fn merge_field(
    _local: &str,
    remote: &str,
    privacy_password: &str,
    decrypted_count: &mut u32,
) -> Result<String> {
    if remote.is_empty() {
        // 保留本地原值
        return Ok(_local.to_string());
    }
    if crypto::is_sealed_field(remote) {
        let plaintext = crypto::decrypt_field(remote, privacy_password)?;
        *decrypted_count += 1;
        return Ok(plaintext);
    }
    // 明文（旧版 payload 或未启用字段级加密）
    Ok(remote.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_payload_round_trip() {
        let payload = SyncPayload::new("test-device".into(), Vec::new(), Vec::new());
        let encrypted = encrypt_payload(&payload, "correct horse battery staple").unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("test-device"));
        let decrypted = decrypt_payload(&encrypted, "correct horse battery staple").unwrap();
        assert_eq!(decrypted.revision, payload.revision);
    }

    #[test]
    fn wrong_password_is_rejected() {
        let payload = SyncPayload::new("test-device".into(), Vec::new(), Vec::new());
        let encrypted = encrypt_payload(&payload, "correct horse battery staple").unwrap();
        assert!(decrypt_payload(&encrypted, "incorrect password").is_err());
    }

    #[test]
    fn endpoint_can_be_a_collection_or_file() {
        assert_eq!(
            sync_url("https://example.test/dav/"),
            "https://example.test/dav/tiny-shell-sync.json"
        );
        assert_eq!(
            sync_url("https://example.test/config.json"),
            "https://example.test/config.json"
        );
    }

    #[test]
    fn s3_url_uses_path_style_and_encodes_object_key() {
        let config = S3Config {
            endpoint: "https://s3.example.test".into(),
            region: "us-east-1".into(),
            bucket: "my-bucket".into(),
            object_key: "configs/my file.json".into(),
            access_key: "access".into(),
            secret_key: "secret".into(),
            session_token: String::new(),
        };
        assert_eq!(
            s3_url(&config).unwrap().as_str(),
            "https://s3.example.test/my-bucket/configs/my%20file.json"
        );
    }

    #[test]
    fn aws_uri_encoding_preserves_only_object_key_slashes() {
        assert_eq!(aws_uri_encode("a b/c", false), "a%20b/c");
        assert_eq!(aws_uri_encode("a/b", true), "a%2Fb");
    }

    fn sample_session(host: &str, user: &str, password: &str) -> Session {
        Session {
            id: Uuid::new_v4().to_string(),
            name: format!("{user}@{host}"),
            host: host.to_string(),
            port: 22,
            user: user.to_string(),
            auth: crate::session::config::AuthMethod::Password,
            password: password.to_string(),
            private_key_path: String::new(),
            private_key_inline: "-----BEGIN PRIVATE KEY-----\nxxx\n".to_string(),
            passphrase: "key-pass".to_string(),
            managed_key_id: None,
            last_used: None,
            group: None,
            proxy_type: "none".to_string(),
            proxy_host: String::new(),
            proxy_port: None,
            proxy_user: String::new(),
            proxy_password: "proxy-pw".to_string(),
        }
    }

    fn sample_key(fp: &str, content: &str, pass: &str) -> ManagedKey {
        ManagedKey {
            id: Uuid::new_v4().to_string(),
            name: format!("key-{fp}"),
            key_type: "ed25519".to_string(),
            fingerprint: fp.to_string(),
            inline_content: content.to_string(),
            passphrase: pass.to_string(),
            created_at: 0,
        }
    }

    #[test]
    fn scrub_without_secrets_clears_sensitive_fields() {
        let sessions = vec![sample_session("h1", "u1", "secret-pw")];
        let keys = vec![sample_key("FP1", "keydata", "keypass")];
        let (s, k) = SecretScrubber::scrub(sessions, keys, false, "ignored").unwrap();
        assert_eq!(s[0].password, "");
        assert_eq!(s[0].passphrase, "");
        assert_eq!(s[0].private_key_inline, "");
        assert_eq!(s[0].proxy_password, "");
        assert_eq!(k[0].inline_content, "");
        assert_eq!(k[0].passphrase, "");
    }

    #[test]
    fn scrub_with_secrets_encrypts_all_fields() {
        let sessions = vec![sample_session("h1", "u1", "secret-pw")];
        let keys = vec![sample_key("FP1", "keydata", "keypass")];
        let (s, k) = SecretScrubber::scrub(sessions, keys, true, "privacy-pw").unwrap();
        assert!(crypto::is_sealed_field(&s[0].password));
        assert!(crypto::is_sealed_field(&s[0].passphrase));
        assert!(crypto::is_sealed_field(&s[0].private_key_inline));
        assert!(crypto::is_sealed_field(&s[0].proxy_password));
        assert!(crypto::is_sealed_field(&k[0].inline_content));
        assert!(crypto::is_sealed_field(&k[0].passphrase));
    }

    #[test]
    fn scrub_no_plaintext_leak_in_encrypted_mode() {
        let sessions = vec![sample_session("h1", "u1", "leaked-pw-123")];
        let keys = vec![sample_key("FP1", "leaked-key-data", "leaked-pass")];
        let (s, k) = SecretScrubber::scrub(sessions, keys, true, "privacy-pw").unwrap();
        let blob = serde_json::to_string(&(s, k)).unwrap();
        assert!(!blob.contains("leaked-pw-123"));
        assert!(!blob.contains("leaked-key-data"));
        assert!(!blob.contains("leaked-pass"));
    }

    #[test]
    fn merge_keeps_local_when_remote_empty() {
        let local = vec![sample_session("h1", "u1", "local-pw")];
        let remote = vec![sample_session("h1", "u1", "")];
        let merged = SecretScrubber::merge(&local, remote, &[], Vec::new(), "privacy-pw").unwrap();
        assert_eq!(merged.sessions[0].password, "local-pw");
        assert_eq!(merged.decrypted_count, 0);
    }

    #[test]
    fn merge_decrypts_when_remote_sealed() {
        let local = vec![sample_session("h1", "u1", "old-pw")];
        let mut remote_sessions = vec![sample_session("h1", "u1", "remote-pw")];
        let (remote, _) =
            SecretScrubber::scrub(std::mem::take(&mut remote_sessions), Vec::new(), true, "pw")
                .unwrap();
        let merged = SecretScrubber::merge(&local, remote, &[], Vec::new(), "pw").unwrap();
        assert_eq!(merged.sessions[0].password, "remote-pw");
        assert!(merged.decrypted_count >= 1);
    }

    #[test]
    fn merge_overwrites_when_remote_plaintext() {
        // 兼容旧版 payload：远端字段为明文时直接覆盖
        let local = vec![sample_session("h1", "u1", "old-pw")];
        let remote = vec![sample_session("h1", "u1", "legacy-pw")];
        let merged = SecretScrubber::merge(&local, remote, &[], Vec::new(), "privacy-pw").unwrap();
        assert_eq!(merged.sessions[0].password, "legacy-pw");
        assert_eq!(merged.decrypted_count, 0);
    }

    #[test]
    fn merge_appends_new_remote_sessions() {
        let local = vec![sample_session("h1", "u1", "pw1")];
        let remote = vec![
            sample_session("h1", "u1", "pw1"),
            sample_session("h2", "u2", "pw2"),
        ];
        let merged = SecretScrubber::merge(&local, remote, &[], Vec::new(), "privacy-pw").unwrap();
        assert_eq!(merged.sessions.len(), 2);
        assert!(merged.sessions.iter().any(|s| s.host == "h2"));
    }

    #[test]
    fn merge_keeps_local_only_sessions() {
        let local = vec![sample_session("h1", "u1", "pw1")];
        let remote: Vec<Session> = Vec::new();
        let merged = SecretScrubber::merge(&local, remote, &[], Vec::new(), "privacy-pw").unwrap();
        assert_eq!(merged.sessions.len(), 1);
        assert_eq!(merged.sessions[0].password, "pw1");
    }

    #[test]
    fn merge_managed_keys_by_fingerprint() {
        let local_keys = vec![sample_key("FP1", "local-content", "local-pass")];
        let mut remote_keys = vec![sample_key("FP1", "remote-content", "remote-pass")];
        let (_, remote_keys_scrubbed) =
            SecretScrubber::scrub(Vec::new(), std::mem::take(&mut remote_keys), true, "pw")
                .unwrap();
        let merged =
            SecretScrubber::merge(&[], Vec::new(), &local_keys, remote_keys_scrubbed, "pw")
                .unwrap();
        assert_eq!(merged.managed_keys.len(), 1);
        assert_eq!(merged.managed_keys[0].inline_content, "remote-content");
        assert_eq!(merged.managed_keys[0].passphrase, "remote-pass");
    }
}
