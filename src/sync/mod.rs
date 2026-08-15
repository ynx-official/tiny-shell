mod backend;
mod http;
mod merge;
mod model;
pub mod protocol;
mod reconcile;
mod secrets;
pub mod state;

use std::fmt;

use anyhow::{Context, Result, anyhow};
use futures::StreamExt as _;
use hmac::{Hmac, Mac};
use reqwest::{StatusCode, header};
use sha2::{Digest, Sha256};

#[cfg(test)]
use crate::crypto;
use crate::session::config::{ManagedKey, QuickCommandCategory, Session};
use backend::for_credentials;
use http::{http_client, send_with_retry};

#[cfg(test)]
pub use model::SyncPayload;
pub use protocol::{V3SyncPayload, parse_payload, serialize_payload};

pub use merge::{
    MergeLocal, MergedConfig, merge_payload_for_upload_with_deleted, merge_payload_with_deleted,
    merge_public_payload_with_deleted,
};
pub use model::PrivacyPasswordStatus;
pub use reconcile::{
    ConflictResolution, SyncConflict, SyncEntityKind, ThreeWayMerge, reconcile_three_way,
};

const SYNC_FILE_NAME: &str = "tiny-shell-sync.json";
const MAX_SYNC_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct SyncCredentials {
    pub backend: SyncBackendCredentials,
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

impl SyncBackendCredentials {
    pub fn kind(&self) -> SyncBackendKind {
        match self {
            Self::WebDav { .. } => SyncBackendKind::WebDav,
            Self::S3 { .. } => SyncBackendKind::S3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncBackendKind {
    WebDav,
    S3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncErrorCategory {
    EndpointRequired,
    EndpointInvalid,
    AuthenticationFailed,
    NotFound,
    Conflict,
    RemoteMissing,
    Other,
}

#[derive(Clone)]
pub struct SyncFailure {
    pub backend: Option<SyncBackendKind>,
    pub category: SyncErrorCategory,
    pub detail: String,
}

impl SyncFailure {
    fn new(
        backend: Option<SyncBackendKind>,
        category: SyncErrorCategory,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            backend,
            category,
            detail: detail.into(),
        }
    }

    pub fn other(backend: Option<SyncBackendKind>, error: impl fmt::Display) -> Self {
        Self::new(backend, SyncErrorCategory::Other, error.to_string())
    }
}

impl fmt::Debug for SyncFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyncFailure")
            .field("backend", &self.backend)
            .field("category", &self.category)
            .field("detail", &"<redacted>")
            .finish()
    }
}

pub type SyncOperationResult<T> = std::result::Result<T, SyncFailure>;

#[derive(Clone)]
pub struct UploadPreflightReady {
    pub credentials: SyncCredentials,
    pub privacy_password: String,
    pub include_secrets: bool,
    pub merged: Option<MergedConfig>,
    pub remote_payload: Option<protocol::V3SyncPayload>,
    pub etag: Option<String>,
}

#[derive(Clone)]
pub struct PendingSyncConflicts {
    pub credentials: SyncCredentials,
    pub privacy_password: String,
    pub include_secrets: bool,
    pub three_way: ThreeWayMerge,
    pub remote_payload: protocol::V3SyncPayload,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadBlockReason {
    PasswordRequired,
    PasswordMismatch,
    UnavailableSecrets {
        session_secret_count: u32,
        managed_key_secret_count: u32,
    },
}

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum SyncResult {
    Uploaded {
        target: state::SyncTargetKey,
        payload: protocol::V3SyncPayload,
        etag: Option<String>,
        privacy_password: Option<String>,
        merged: Option<MergedConfig>,
    },
    UploadPreflightReady(UploadPreflightReady),
    ReconciliationConflicts(PendingSyncConflicts),
    UploadPreflightBlocked {
        credentials: SyncCredentials,
        reason: UploadBlockReason,
    },
    Downloaded {
        credentials: SyncCredentials,
        target: state::SyncTargetKey,
        payload: protocol::V3SyncPayload,
        password_status: PrivacyPasswordStatus,
        sessions: Vec<Session>,
        deleted_sessions: Vec<crate::session::config::DeletedSession>,
        connection_groups: Vec<String>,
        deleted_connection_groups: Vec<crate::session::config::DeletedConnectionGroup>,
        managed_keys: Vec<ManagedKey>,
        quick_command_categories: Vec<QuickCommandCategory>,
        etag: Option<String>,
        /// 本次下载成功解密的敏感字段数。
        decrypted_count: u32,
        /// 因密码缺失、错误或密文损坏而未能解密的敏感字段数。
        unavailable_secret_count: u32,
    },
    /// 本地强行重置隐私密码成功，需把新密码硬件绑定落盘。
    PrivacyPasswordReset {
        target: state::SyncTargetKey,
        payload: protocol::V3SyncPayload,
        new_password: String,
        etag: Option<String>,
    },
    PrivacyPasswordChecked {
        password: String,
        status: PrivacyPasswordStatus,
    },
    PrivacyPasswordInitializationReady {
        credentials: SyncCredentials,
        password: String,
    },
    ConnectionVerified,
    Failed(SyncFailure),
}

impl fmt::Debug for SyncResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uploaded {
                etag,
                privacy_password,
                ..
            } => formatter
                .debug_struct("Uploaded")
                .field("etag", etag)
                .field(
                    "privacy_password",
                    &privacy_password.as_ref().map(|_| "<redacted>"),
                )
                .finish(),
            Self::UploadPreflightReady(plan) => formatter
                .debug_struct("UploadPreflightReady")
                .field("include_secrets", &plan.include_secrets)
                .field("has_remote_config", &plan.merged.is_some())
                .field("has_remote_payload", &plan.remote_payload.is_some())
                .field("etag", &plan.etag)
                .finish(),
            Self::ReconciliationConflicts(pending) => formatter
                .debug_struct("ReconciliationConflicts")
                .field("conflict_count", &pending.three_way.conflicts.len())
                .field("etag", &pending.etag)
                .finish(),
            Self::UploadPreflightBlocked { reason, .. } => formatter
                .debug_struct("UploadPreflightBlocked")
                .field("reason", reason)
                .finish(),
            Self::Downloaded {
                password_status,
                sessions,
                connection_groups,
                managed_keys,
                quick_command_categories,
                etag,
                decrypted_count,
                unavailable_secret_count,
                ..
            } => formatter
                .debug_struct("Downloaded")
                .field("password_status", password_status)
                .field("session_count", &sessions.len())
                .field("connection_group_count", &connection_groups.len())
                .field("managed_key_count", &managed_keys.len())
                .field(
                    "quick_command_category_count",
                    &quick_command_categories.len(),
                )
                .field("etag", etag)
                .field("decrypted_count", decrypted_count)
                .field("unavailable_secret_count", unavailable_secret_count)
                .finish(),
            Self::PrivacyPasswordReset { .. } => formatter
                .debug_struct("PrivacyPasswordReset")
                .field("new_password", &"<redacted>")
                .finish(),
            Self::PrivacyPasswordChecked { status, .. } => formatter
                .debug_struct("PrivacyPasswordChecked")
                .field("password", &"<redacted>")
                .field("status", status)
                .finish(),
            Self::PrivacyPasswordInitializationReady { .. } => formatter
                .debug_struct("PrivacyPasswordInitializationReady")
                .field("password", &"<redacted>")
                .finish(),
            Self::ConnectionVerified => formatter.write_str("ConnectionVerified"),
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

#[derive(Debug, PartialEq, Eq)]
enum UploadCondition<'a> {
    IfMatch(&'a str),
    IfNoneMatch,
    None,
}

impl UploadMode {
    /// 便捷构造：使用本地已记录的 etag 做条件上传。
    pub fn conditional(expected_etag: Option<String>) -> Self {
        UploadMode::Conditional { expected_etag }
    }

    fn condition(&self) -> UploadCondition<'_> {
        match self {
            Self::Conditional {
                expected_etag: Some(etag),
            } => UploadCondition::IfMatch(etag),
            Self::Conditional {
                expected_etag: None,
            } => UploadCondition::IfNoneMatch,
            Self::Force => UploadCondition::None,
        }
    }
}

pub async fn upload(
    credentials: SyncCredentials,
    payload: V3SyncPayload,
    mode: UploadMode,
) -> SyncOperationResult<Option<String>> {
    validate_credentials(&credentials)?;
    let backend = credentials.backend.kind();
    let body = serialize_payload(&payload)
        .map_err(|error| SyncFailure::other(Some(backend), format!("{error:#}")))?;
    for_credentials(&credentials.backend)
        .upload(&credentials.backend, body, mode)
        .await
}

pub(super) async fn upload_webdav(
    endpoint: &str,
    username: &str,
    password: &str,
    body: Vec<u8>,
    mode: UploadMode,
) -> SyncOperationResult<Option<String>> {
    let client = http_client(Some(SyncBackendKind::WebDav))?;
    let mut request = client
        .put(webdav_sync_url(endpoint)?)
        .basic_auth(username, Some(password))
        .header(header::CONTENT_TYPE, "application/json")
        .body(body);
    request = match mode.condition() {
        UploadCondition::IfMatch(etag) => request.header(header::IF_MATCH, etag),
        UploadCondition::IfNoneMatch => {
            // An uninitialized client may only create a new remote file. This keeps
            // it from silently replacing configuration uploaded by another device.
            request.header(header::IF_NONE_MATCH, "*")
        }
        UploadCondition::None => request,
    };
    let response = send_with_retry(request, SyncBackendKind::WebDav, "send WebDAV upload").await?;
    let status = response.status();
    if is_conflict_status(status) {
        return Err(SyncFailure::new(
            Some(SyncBackendKind::WebDav),
            SyncErrorCategory::Conflict,
            "remote configuration changed; download it before uploading",
        ));
    }
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(SyncFailure::new(
            Some(SyncBackendKind::WebDav),
            SyncErrorCategory::AuthenticationFailed,
            format!("WebDAV upload failed: HTTP {status}"),
        ));
    }
    if status == StatusCode::NOT_FOUND {
        return Err(SyncFailure::new(
            Some(SyncBackendKind::WebDav),
            SyncErrorCategory::NotFound,
            format!("WebDAV upload failed: HTTP {status}"),
        ));
    }
    if !status.is_success() {
        return Err(SyncFailure::other(
            Some(SyncBackendKind::WebDav),
            format!("WebDAV upload failed: HTTP {status}"),
        ));
    }
    Ok(response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string))
}

pub async fn download(
    credentials: SyncCredentials,
    _privacy_password: &str,
) -> SyncOperationResult<(V3SyncPayload, Option<String>)> {
    validate_credentials(&credentials)?;
    let backend = credentials.backend.kind();
    let (body, etag) = for_credentials(&credentials.backend)
        .download(&credentials.backend)
        .await?;
    let payload = parse_payload(&body)
        .map_err(|error| SyncFailure::other(Some(backend), format!("{error:#}")))?;
    Ok((payload, etag))
}

pub(super) async fn download_webdav(
    endpoint: &str,
    username: &str,
    password: &str,
) -> SyncOperationResult<(Vec<u8>, Option<String>)> {
    let response = http_client(Some(SyncBackendKind::WebDav))?
        .get(webdav_sync_url(endpoint)?)
        .basic_auth(username, Some(password));
    let response =
        send_with_retry(response, SyncBackendKind::WebDav, "send WebDAV download").await?;
    let status = response.status();
    if status == StatusCode::NOT_FOUND {
        return Err(SyncFailure::new(
            Some(SyncBackendKind::WebDav),
            SyncErrorCategory::RemoteMissing,
            "no remote configuration exists yet",
        ));
    }
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(SyncFailure::new(
            Some(SyncBackendKind::WebDav),
            SyncErrorCategory::AuthenticationFailed,
            format!("WebDAV download failed: HTTP {status}"),
        ));
    }
    if !status.is_success() {
        return Err(SyncFailure::other(
            Some(SyncBackendKind::WebDav),
            format!("WebDAV download failed: HTTP {status}"),
        ));
    }
    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = read_response_body_limited(response, SyncBackendKind::WebDav).await?;
    Ok((body, etag))
}

pub async fn verify_webdav_connection(
    endpoint: &str,
    username: &str,
    password: &str,
) -> SyncOperationResult<()> {
    let verify_url = webdav_verification_url(endpoint)?;
    let response = http_client(Some(SyncBackendKind::WebDav))?
        .request(
            reqwest::Method::from_bytes(b"PROPFIND")
                .map_err(|error| SyncFailure::other(Some(SyncBackendKind::WebDav), error))?,
            verify_url,
        )
        .basic_auth(username, Some(password))
        .header("Depth", "0");
    let response = send_with_retry(
        response,
        SyncBackendKind::WebDav,
        "send WebDAV connection check",
    )
    .await?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(SyncFailure::new(
            Some(SyncBackendKind::WebDav),
            SyncErrorCategory::AuthenticationFailed,
            format!("WebDAV connection check failed: HTTP {status}"),
        ));
    }
    if status == StatusCode::NOT_FOUND {
        return Err(SyncFailure::new(
            Some(SyncBackendKind::WebDav),
            SyncErrorCategory::NotFound,
            format!("WebDAV connection check failed: HTTP {status}"),
        ));
    }
    if !status.is_success() {
        return Err(SyncFailure::other(
            Some(SyncBackendKind::WebDav),
            format!("WebDAV connection check failed: HTTP {status}"),
        ));
    }
    Ok(())
}

pub fn validate_credentials(credentials: &SyncCredentials) -> SyncOperationResult<()> {
    match &credentials.backend {
        SyncBackendCredentials::WebDav { endpoint, .. } => {
            validate_webdav_endpoint(endpoint).map(|_| ())
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
            Err(SyncFailure::other(
                Some(SyncBackendKind::S3),
                "S3 region, bucket, access key and secret key are required",
            ))
        }
        SyncBackendCredentials::S3 { .. } => Ok(()),
    }
}

fn validate_webdav_endpoint(endpoint: &str) -> SyncOperationResult<reqwest::Url> {
    if endpoint.trim().is_empty() {
        return Err(SyncFailure::new(
            Some(SyncBackendKind::WebDav),
            SyncErrorCategory::EndpointRequired,
            "WebDAV directory URL is required",
        ));
    }
    reqwest::Url::parse(endpoint.trim()).map_err(|error| {
        SyncFailure::new(
            Some(SyncBackendKind::WebDav),
            SyncErrorCategory::EndpointInvalid,
            format!("parse WebDAV directory URL: {error}"),
        )
    })
}

fn webdav_collection_url(endpoint: &str) -> SyncOperationResult<reqwest::Url> {
    let mut url = validate_webdav_endpoint(endpoint)?;
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn webdav_sync_url(endpoint: &str) -> SyncOperationResult<reqwest::Url> {
    let mut url = webdav_collection_url(endpoint)?;
    let path = format!("{}{SYNC_FILE_NAME}", url.path());
    url.set_path(&path);
    Ok(url)
}

fn webdav_verification_url(endpoint: &str) -> SyncOperationResult<reqwest::Url> {
    webdav_collection_url(endpoint)
}

pub(super) struct S3Config {
    pub(super) endpoint: String,
    pub(super) region: String,
    pub(super) bucket: String,
    pub(super) object_key: String,
    pub(super) access_key: String,
    pub(super) secret_key: String,
    pub(super) session_token: String,
}

pub(super) async fn upload_s3(
    config: &S3Config,
    body: Vec<u8>,
    mode: UploadMode,
) -> SyncOperationResult<Option<String>> {
    let url = s3_url(config)
        .map_err(|error| SyncFailure::other(Some(SyncBackendKind::S3), format!("{error:#}")))?;
    let mut headers = signed_s3_headers("PUT", &url, &body, config)
        .map_err(|error| SyncFailure::other(Some(SyncBackendKind::S3), format!("{error:#}")))?;
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    match mode.condition() {
        UploadCondition::IfMatch(etag) => {
            let etag = header_value(etag, "S3 ETag").map_err(|error| {
                SyncFailure::other(Some(SyncBackendKind::S3), format!("{error:#}"))
            })?;
            headers.insert(header::IF_MATCH, etag);
        }
        UploadCondition::IfNoneMatch => {
            headers.insert(header::IF_NONE_MATCH, header::HeaderValue::from_static("*"));
        }
        UploadCondition::None => {}
    }
    let response = http_client(Some(SyncBackendKind::S3))?
        .put(url)
        .headers(headers)
        .body(body);
    let response = send_with_retry(response, SyncBackendKind::S3, "send S3 upload").await?;
    let status = response.status();
    if is_conflict_status(status) {
        return Err(SyncFailure::new(
            Some(SyncBackendKind::S3),
            SyncErrorCategory::Conflict,
            "remote configuration changed; download it before uploading",
        ));
    }
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        return Err(SyncFailure::other(
            Some(SyncBackendKind::S3),
            format!("S3 upload failed: HTTP {status}: {detail}"),
        ));
    }
    Ok(response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string))
}

pub(super) async fn download_s3(
    config: &S3Config,
) -> SyncOperationResult<(Vec<u8>, Option<String>)> {
    let url = s3_url(config)
        .map_err(|error| SyncFailure::other(Some(SyncBackendKind::S3), format!("{error:#}")))?;
    let headers = signed_s3_headers("GET", &url, &[], config)
        .map_err(|error| SyncFailure::other(Some(SyncBackendKind::S3), format!("{error:#}")))?;
    let response = http_client(Some(SyncBackendKind::S3))?
        .get(url)
        .headers(headers);
    let response = send_with_retry(response, SyncBackendKind::S3, "send S3 download").await?;
    let status = response.status();
    if status == StatusCode::NOT_FOUND {
        return Err(SyncFailure::new(
            Some(SyncBackendKind::S3),
            SyncErrorCategory::RemoteMissing,
            "no remote configuration exists yet",
        ));
    }
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        return Err(SyncFailure::other(
            Some(SyncBackendKind::S3),
            format!("S3 download failed: HTTP {status}: {detail}"),
        ));
    }
    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = read_response_body_limited(response, SyncBackendKind::S3).await?;
    Ok((body, etag))
}

async fn read_response_body_limited(
    response: reqwest::Response,
    backend: SyncBackendKind,
) -> SyncOperationResult<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SYNC_PAYLOAD_BYTES as u64)
    {
        return Err(SyncFailure::other(
            Some(backend),
            format!(
                "synchronized configuration exceeds the {} MiB limit",
                MAX_SYNC_PAYLOAD_BYTES / (1024 * 1024)
            ),
        ));
    }

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            SyncFailure::other(Some(backend), format!("read sync response: {error}"))
        })?;
        if extend_body_with_limit(&mut body, &chunk).is_err() {
            return Err(SyncFailure::other(
                Some(backend),
                format!(
                    "synchronized configuration exceeds the {} MiB limit",
                    MAX_SYNC_PAYLOAD_BYTES / (1024 * 1024)
                ),
            ));
        }
    }
    Ok(body)
}

fn extend_body_with_limit(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), ()> {
    if body.len().saturating_add(chunk.len()) > MAX_SYNC_PAYLOAD_BYTES {
        return Err(());
    }
    body.extend_from_slice(chunk);
    Ok(())
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

fn is_conflict_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::PRECONDITION_FAILED | StatusCode::CONFLICT
    )
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

/// 对上传到同步后端的快照做敏感字段处理。
///
/// - `include_secrets=false`：把所有敏感字段清空，远端只保留连接骨架。
///   下载时这些空字段会被 `merge` 保留为本地原值。
/// - `include_secrets=true`：用隐私密码对每个敏感字段做字段级加密，
///   密文带 `v1:` 前缀，下载时由 `merge` 解密还原。
#[cfg(test)]
pub struct SecretScrubber;

#[cfg(test)]
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
    /// 远端对象提供基础信息；敏感字段则按状态独立处理：空字段保留本地值，
    /// 可解密密文覆盖本地值，无法解密的密文保留本地值或在新对象中置空。
    pub fn merge(
        local_sessions: &[Session],
        remote_sessions: Vec<Session>,
        local_keys: &[ManagedKey],
        remote_keys: Vec<ManagedKey>,
        privacy_password: &str,
    ) -> MergedSecrets {
        let mut decrypted_count = 0;
        let mut unavailable_secret_count = 0;

        let mut sessions: Vec<Session> = remote_sessions
            .into_iter()
            .map(|mut remote| {
                let local = local_sessions
                    .iter()
                    .find(|local| sessions_match(local, &remote));
                merge_session_secrets(
                    &mut remote,
                    local,
                    privacy_password,
                    &mut decrypted_count,
                    &mut unavailable_secret_count,
                );
                remote
            })
            .collect();
        let local_only_sessions: Vec<_> = local_sessions
            .iter()
            .filter(|local| !sessions.iter().any(|remote| sessions_match(local, remote)))
            .cloned()
            .collect();
        sessions.extend(local_only_sessions);

        let mut managed_keys: Vec<ManagedKey> = remote_keys
            .into_iter()
            .map(|mut remote| {
                let local = local_keys
                    .iter()
                    .find(|local| local.fingerprint == remote.fingerprint);
                merge_key_secrets(
                    &mut remote,
                    local,
                    privacy_password,
                    &mut decrypted_count,
                    &mut unavailable_secret_count,
                );
                remote
            })
            .collect();
        let local_only_keys: Vec<_> = local_keys
            .iter()
            .filter(|local| {
                !managed_keys
                    .iter()
                    .any(|remote| remote.fingerprint == local.fingerprint)
            })
            .cloned()
            .collect();
        managed_keys.extend(local_only_keys);

        MergedSecrets {
            sessions,
            managed_keys,
            decrypted_count,
            unavailable_secret_count,
        }
    }
}

#[cfg(test)]
pub struct MergedSecrets {
    pub sessions: Vec<Session>,
    pub managed_keys: Vec<ManagedKey>,
    pub decrypted_count: u32,
    pub unavailable_secret_count: u32,
}

#[cfg(test)]
fn sessions_match(a: &Session, b: &Session) -> bool {
    a.host == b.host && a.port == b.port && a.user == b.user
}

#[cfg(test)]
fn merge_session_secrets(
    remote: &mut Session,
    local: Option<&Session>,
    privacy_password: &str,
    decrypted_count: &mut u32,
    unavailable_secret_count: &mut u32,
) {
    remote.password = merge_secret_field(
        local.map_or("", |value| value.password.as_str()),
        &remote.password,
        privacy_password,
        decrypted_count,
        unavailable_secret_count,
    );
    remote.passphrase = merge_secret_field(
        local.map_or("", |value| value.passphrase.as_str()),
        &remote.passphrase,
        privacy_password,
        decrypted_count,
        unavailable_secret_count,
    );
    remote.private_key_inline = merge_secret_field(
        local.map_or("", |value| value.private_key_inline.as_str()),
        &remote.private_key_inline,
        privacy_password,
        decrypted_count,
        unavailable_secret_count,
    );
    remote.proxy_password = merge_secret_field(
        local.map_or("", |value| value.proxy_password.as_str()),
        &remote.proxy_password,
        privacy_password,
        decrypted_count,
        unavailable_secret_count,
    );
}

#[cfg(test)]
fn merge_key_secrets(
    remote: &mut ManagedKey,
    local: Option<&ManagedKey>,
    privacy_password: &str,
    decrypted_count: &mut u32,
    unavailable_secret_count: &mut u32,
) {
    remote.inline_content = merge_secret_field(
        local.map_or("", |value| value.inline_content.as_str()),
        &remote.inline_content,
        privacy_password,
        decrypted_count,
        unavailable_secret_count,
    );
    remote.passphrase = merge_secret_field(
        local.map_or("", |value| value.passphrase.as_str()),
        &remote.passphrase,
        privacy_password,
        decrypted_count,
        unavailable_secret_count,
    );
}

#[cfg(test)]
fn merge_secret_field(
    local: &str,
    remote: &str,
    privacy_password: &str,
    decrypted_count: &mut u32,
    unavailable_secret_count: &mut u32,
) -> String {
    if remote.is_empty() {
        return local.to_string();
    }
    if !crypto::is_sealed_field(remote) {
        return remote.to_string();
    }
    match crypto::decrypt_field(remote, privacy_password) {
        Ok(plaintext) => {
            *decrypted_count += 1;
            plaintext
        }
        Err(_) => {
            *unavailable_secret_count += 1;
            local.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{SyncPayload, protocol::V3SyncPayload};
    use uuid::Uuid;

    #[test]
    fn payload_is_plain_json_with_readable_basic_information() {
        let sessions = vec![sample_session("example.test", "alice", "secret")];
        let (sessions, managed_keys) =
            SecretScrubber::scrub(sessions, Vec::new(), true, "privacy-password").unwrap();
        let payload = SyncPayload::new(
            "test-device".into(),
            sessions,
            Vec::new(),
            managed_keys,
            Vec::new(),
            true,
            "privacy-password",
        )
        .unwrap();

        let payload: V3SyncPayload = payload.into();
        let serialized = serialize_payload(&payload).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(json["device_id"], "test-device");
        assert_eq!(json["sessions"][0]["value"]["host"], "example.test");
        assert_eq!(json["sessions"][0]["value"]["user"], "alice");
        assert!(
            json["sessions"][0]["value"]["password"]["value"]
                .as_str()
                .is_some_and(crypto::is_sealed_field)
        );
        assert_eq!(
            parse_payload(&serialized).unwrap().revision,
            payload.revision
        );
    }

    #[test]
    fn backend_credentials_do_not_require_a_sync_file_password() {
        let credentials = SyncCredentials {
            backend: SyncBackendCredentials::WebDav {
                endpoint: "https://example.test/dav/".to_string(),
                username: "user".to_string(),
                password: "password".to_string(),
            },
        };
        assert!(validate_credentials(&credentials).is_ok());
    }

    #[test]
    fn upload_mode_maps_to_safe_conditional_headers() {
        let etag = UploadMode::conditional(Some("etag-1".into()));
        let create = UploadMode::conditional(None);
        let force = UploadMode::Force;

        assert_eq!(etag.condition(), UploadCondition::IfMatch("etag-1"));
        assert_eq!(create.condition(), UploadCondition::IfNoneMatch);
        assert_eq!(force.condition(), UploadCondition::None);
    }

    #[test]
    fn precondition_and_conflict_responses_never_count_as_success() {
        assert!(is_conflict_status(StatusCode::PRECONDITION_FAILED));
        assert!(is_conflict_status(StatusCode::CONFLICT));
        assert!(!is_conflict_status(StatusCode::OK));
    }

    #[test]
    fn webdav_sync_url_appends_the_project_file_name() {
        assert_eq!(
            webdav_sync_url("https://example.test/dav/")
                .unwrap()
                .as_str(),
            "https://example.test/dav/tiny-shell-sync.json"
        );
        assert_eq!(
            webdav_sync_url("https://example.test/dav")
                .unwrap()
                .as_str(),
            "https://example.test/dav/tiny-shell-sync.json"
        );
    }

    #[test]
    fn webdav_endpoint_requires_a_valid_directory_url() {
        assert!(validate_webdav_endpoint("").is_err());
        assert!(validate_webdav_endpoint("not a url").is_err());
        assert!(validate_webdav_endpoint("https://example.test/dav/").is_ok());
        assert!(validate_webdav_endpoint("https://example.test/dav").is_ok());
    }

    #[test]
    fn webdav_verification_uses_the_collection_url() {
        assert_eq!(
            webdav_verification_url("https://example.test/dav/")
                .unwrap()
                .as_str(),
            "https://example.test/dav/"
        );
        assert_eq!(
            webdav_verification_url("https://example.test/dav")
                .unwrap()
                .as_str(),
            "https://example.test/dav/"
        );
    }

    #[test]
    fn sync_response_body_limit_rejects_oversized_payloads() {
        let mut body = vec![0; MAX_SYNC_PAYLOAD_BYTES - 2];
        assert!(extend_body_with_limit(&mut body, &[1, 2]).is_ok());
        assert!(extend_body_with_limit(&mut body, &[3]).is_err());
        assert_eq!(body.len(), MAX_SYNC_PAYLOAD_BYTES);
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
            connection_type: crate::session::config::ConnectionType::Ssh,
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
        let merged = SecretScrubber::merge(&local, remote, &[], Vec::new(), "privacy-pw");
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
        let merged = SecretScrubber::merge(&local, remote, &[], Vec::new(), "pw");
        assert_eq!(merged.sessions[0].password, "remote-pw");
        assert!(merged.decrypted_count >= 1);
    }

    #[test]
    fn wrong_password_still_recovers_remote_basic_information() {
        let mut remote_sessions = vec![sample_session("recover.example", "alice", "remote-pw")];
        remote_sessions[0].name = "Recovered session".to_string();
        let (remote_sessions, _) =
            SecretScrubber::scrub(remote_sessions, Vec::new(), true, "correct-password").unwrap();

        let merged =
            SecretScrubber::merge(&[], remote_sessions, &[], Vec::new(), "forgotten-password");

        assert_eq!(merged.sessions.len(), 1);
        assert_eq!(merged.sessions[0].name, "Recovered session");
        assert_eq!(merged.sessions[0].host, "recover.example");
        assert_eq!(merged.sessions[0].user, "alice");
        assert!(merged.sessions[0].password.is_empty());
        assert!(merged.sessions[0].private_key_inline.is_empty());
        assert!(merged.unavailable_secret_count > 0);
    }

    #[test]
    fn merge_overwrites_when_remote_plaintext() {
        // 兼容旧版 payload：远端字段为明文时直接覆盖
        let local = vec![sample_session("h1", "u1", "old-pw")];
        let remote = vec![sample_session("h1", "u1", "legacy-pw")];
        let merged = SecretScrubber::merge(&local, remote, &[], Vec::new(), "privacy-pw");
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
        let merged = SecretScrubber::merge(&local, remote, &[], Vec::new(), "privacy-pw");
        assert_eq!(merged.sessions.len(), 2);
        assert!(merged.sessions.iter().any(|s| s.host == "h2"));
    }

    #[test]
    fn merge_keeps_local_only_sessions() {
        let local = vec![sample_session("h1", "u1", "pw1")];
        let remote: Vec<Session> = Vec::new();
        let merged = SecretScrubber::merge(&local, remote, &[], Vec::new(), "privacy-pw");
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
            SecretScrubber::merge(&[], Vec::new(), &local_keys, remote_keys_scrubbed, "pw");
        assert_eq!(merged.managed_keys.len(), 1);
        assert_eq!(merged.managed_keys[0].inline_content, "remote-content");
        assert_eq!(merged.managed_keys[0].passphrase, "remote-pass");
    }
}
