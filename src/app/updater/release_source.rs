use anyhow::Context;
use reqwest::{Client, StatusCode, header::HeaderMap};
use semver::Version;
use serde::Deserialize;

const REPO_OWNER: &str = "ynx-official";
const REPO_NAME: &str = "tiny-shell";
const MANIFEST_SCHEMA_VERSION: u32 = 1;
const MANIFEST_URL: &str =
    "https://github.com/ynx-official/tiny-shell/releases/latest/download/update-manifest.json";
const API_URL: &str = "https://api.github.com/repos/ynx-official/tiny-shell/releases/latest";
const RELEASES_LATEST_URL: &str = "https://github.com/ynx-official/tiny-shell/releases/latest";

#[derive(Debug, Clone)]
pub(super) struct ReleaseMetadata {
    pub version: String,
    pub notes: String,
    pub assets: Vec<ReleaseAssetMetadata>,
}

#[derive(Debug, Clone)]
pub(super) struct ReleaseAssetMetadata {
    pub name: String,
    pub download_url: String,
    pub size: u64,
    pub digest: String,
}

#[derive(Debug)]
enum SourceAttempt<T> {
    Found(T),
    Missing,
    Unavailable(anyhow::Error),
}

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    schema_version: u32,
    version: String,
    #[serde(default)]
    notes_url: Option<String>,
    assets: Vec<ManifestAsset>,
}

#[derive(Debug, Deserialize)]
struct ManifestAsset {
    name: String,
    url: String,
    size: u64,
    digest: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

pub(super) async fn fetch_latest_release(client: &Client) -> anyhow::Result<ReleaseMetadata> {
    match fetch_manifest(client).await? {
        SourceAttempt::Found(release) => return Ok(release),
        SourceAttempt::Missing => {
            tracing::info!(
                "latest release does not provide update-manifest.json; using GitHub API"
            );
        }
        SourceAttempt::Unavailable(error) => {
            tracing::warn!("update manifest unavailable; using GitHub API: {error:#}");
        }
    }

    match fetch_github_api(client).await? {
        SourceAttempt::Found(release) => Ok(release),
        SourceAttempt::Unavailable(error) => {
            tracing::warn!("GitHub release API unavailable; using HTML fallback: {error:#}");
            fetch_github_html(client).await.with_context(|| {
                format!("GitHub API failed ({error:#}); HTML fallback also failed")
            })
        }
        SourceAttempt::Missing => anyhow::bail!("no release has been published yet"),
    }
}

async fn fetch_manifest(client: &Client) -> anyhow::Result<SourceAttempt<ReleaseMetadata>> {
    let response = match client.get(MANIFEST_URL).send().await {
        Ok(response) => response,
        Err(error) => {
            return Ok(SourceAttempt::Unavailable(
                anyhow::Error::new(error).context("failed to fetch update manifest"),
            ));
        }
    };

    let status = response.status();
    if status == StatusCode::NOT_FOUND {
        return Ok(SourceAttempt::Missing);
    }
    if is_availability_status(status) {
        return Ok(SourceAttempt::Unavailable(anyhow::anyhow!(
            "update manifest returned {status}"
        )));
    }
    if !status.is_success() {
        anyhow::bail!("update manifest returned {status}");
    }

    let manifest: UpdateManifest = response
        .json()
        .await
        .context("failed to parse update manifest JSON")?;
    let notes_url = manifest.notes_url.clone();
    let mut release = manifest_into_release(manifest)?;
    if let Some(notes_url) = notes_url {
        validate_https_url(&notes_url).context("update manifest contains an invalid notes URL")?;
        release.notes = fetch_optional_notes(client, &notes_url).await;
    }
    Ok(SourceAttempt::Found(release))
}

fn manifest_into_release(manifest: UpdateManifest) -> anyhow::Result<ReleaseMetadata> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported update manifest schema version {}",
            manifest.schema_version
        );
    }
    validate_version(&manifest.version)?;
    if manifest.assets.is_empty() {
        anyhow::bail!("update manifest does not contain release assets");
    }

    let version = manifest.version.clone();
    let assets = manifest
        .assets
        .into_iter()
        .map(|asset| {
            validate_asset(&version, &asset.name, &asset.url, &asset.digest)?;
            Ok(ReleaseAssetMetadata {
                name: asset.name,
                download_url: asset.url,
                size: asset.size,
                digest: asset.digest,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(ReleaseMetadata {
        version: manifest.version,
        notes: String::new(),
        assets,
    })
}

async fn fetch_github_api(client: &Client) -> anyhow::Result<SourceAttempt<ReleaseMetadata>> {
    let response = match client
        .get(API_URL)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return Ok(SourceAttempt::Unavailable(
                anyhow::Error::new(error).context("failed to fetch latest GitHub release"),
            ));
        }
    };

    let status = response.status();
    if status == StatusCode::NOT_FOUND {
        return Ok(SourceAttempt::Missing);
    }
    if !status.is_success() {
        let headers = response.headers().clone();
        let body = response.text().await.unwrap_or_default();
        let error = github_status_error(status, &headers, &body);
        if is_availability_status(status) {
            return Ok(SourceAttempt::Unavailable(error));
        }
        return Err(error);
    }

    let release: GitHubRelease = response
        .json()
        .await
        .context("failed to parse GitHub release JSON")?;
    let version = release.tag_name.clone();
    validate_version(&version)?;
    let assets = release
        .assets
        .into_iter()
        .map(|asset| {
            let digest = asset.digest.with_context(|| {
                format!(
                    "release asset '{}' does not provide a SHA-256 digest",
                    asset.name
                )
            })?;
            validate_asset(&version, &asset.name, &asset.browser_download_url, &digest)?;
            Ok(ReleaseAssetMetadata {
                name: asset.name,
                download_url: asset.browser_download_url,
                size: asset.size,
                digest,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(SourceAttempt::Found(ReleaseMetadata {
        version,
        notes: release.body,
        assets,
    }))
}

async fn fetch_github_html(client: &Client) -> anyhow::Result<ReleaseMetadata> {
    let latest_response = client
        .get(RELEASES_LATEST_URL)
        .send()
        .await
        .context("failed to resolve latest GitHub release page")?
        .error_for_status()
        .context("latest GitHub release page returned an error status")?;
    let version = version_from_release_url(latest_response.url().as_str())?;

    let expanded_assets_url =
        format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/expanded_assets/{version}");
    let html = client
        .get(&expanded_assets_url)
        .send()
        .await
        .context("failed to fetch GitHub release assets page")?
        .error_for_status()
        .context("GitHub release assets page returned an error status")?
        .text()
        .await
        .context("failed to read GitHub release assets page")?;
    let assets = parse_expanded_assets(&html, &version)?;

    let notes_url = format!(
        "https://raw.githubusercontent.com/{REPO_OWNER}/{REPO_NAME}/{version}/docs/upgrade/{version}/README.md"
    );
    let notes = fetch_optional_notes(client, &notes_url).await;

    Ok(ReleaseMetadata {
        version,
        notes,
        assets,
    })
}

async fn fetch_optional_notes(client: &Client, url: &str) -> String {
    let result = async {
        client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
    }
    .await;

    match result {
        Ok(notes) => notes,
        Err(error) => {
            tracing::warn!("failed to fetch fallback release notes: {error}");
            String::new()
        }
    }
}

fn version_from_release_url(url: &str) -> anyhow::Result<String> {
    let parsed = reqwest::Url::parse(url).context("latest release redirect URL is invalid")?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("github.com") {
        anyhow::bail!("latest release redirected to an unexpected host");
    }

    let expected_prefix = format!("/{REPO_OWNER}/{REPO_NAME}/releases/tag/");
    let version = parsed
        .path()
        .strip_prefix(&expected_prefix)
        .filter(|value| !value.is_empty() && !value.contains('/'))
        .context("latest release redirect does not contain a valid tag")?;
    validate_version(version)?;
    Ok(version.to_string())
}

fn parse_expanded_assets(html: &str, version: &str) -> anyhow::Result<Vec<ReleaseAssetMetadata>> {
    let release_path = format!("/{REPO_OWNER}/{REPO_NAME}/releases/download/{version}/");
    let assets = html
        .split("<li")
        .filter_map(|item| parse_asset_item(item, version, &release_path).transpose())
        .collect::<anyhow::Result<Vec<_>>>()?;

    if assets.is_empty() {
        anyhow::bail!("GitHub release assets page did not contain downloadable assets");
    }
    Ok(assets)
}

fn parse_asset_item(
    item: &str,
    version: &str,
    release_path: &str,
) -> anyhow::Result<Option<ReleaseAssetMetadata>> {
    let Some(path_start) = item.find(release_path) else {
        return Ok(None);
    };
    let path = &item[path_start..];
    let path_end = path.find('"').context("release asset link is malformed")?;
    let path = &path[..path_end];
    let name = path
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .context("release asset link has no file name")?;

    let digest_start = item
        .find("sha256:")
        .context("release asset does not provide a SHA-256 digest")?;
    let digest_end = digest_start + "sha256:".len() + 64;
    let digest = item
        .get(digest_start..digest_end)
        .context("release asset contains a truncated SHA-256 digest")?;
    let download_url = format!("https://github.com{path}");
    validate_asset(version, name, &download_url, digest)?;

    Ok(Some(ReleaseAssetMetadata {
        name: name.to_string(),
        download_url,
        size: 0,
        digest: digest.to_string(),
    }))
}

fn validate_version(version: &str) -> anyhow::Result<()> {
    let normalized = version
        .trim()
        .strip_prefix(['v', 'V'])
        .unwrap_or(version.trim());
    Version::parse(normalized)
        .with_context(|| format!("release contains invalid version '{version}'"))?;
    Ok(())
}

fn validate_asset(version: &str, name: &str, url: &str, digest: &str) -> anyhow::Result<()> {
    if name.is_empty() || name.contains(['/', '\\']) {
        anyhow::bail!("release asset has an invalid file name");
    }
    let parsed_url = validate_https_url(url)
        .with_context(|| format!("release asset '{name}' contains an invalid URL"))?;
    if parsed_url.host_str() != Some("github.com") {
        anyhow::bail!("release asset '{name}' uses an unexpected download host");
    }
    let expected_path = format!("/{REPO_OWNER}/{REPO_NAME}/releases/download/{version}/{name}");
    if parsed_url.path() != expected_path
        || parsed_url.query().is_some()
        || parsed_url.fragment().is_some()
    {
        anyhow::bail!("release asset '{name}' URL does not match release {version}");
    }
    validate_digest(digest)
        .with_context(|| format!("release asset '{name}' contains an invalid digest"))
}

fn validate_https_url(url: &str) -> anyhow::Result<reqwest::Url> {
    let parsed_url = reqwest::Url::parse(url).context("URL is invalid")?;
    if parsed_url.scheme() != "https" {
        anyhow::bail!("URL does not use HTTPS");
    }
    Ok(parsed_url)
}

fn validate_digest(digest: &str) -> anyhow::Result<()> {
    let value = digest
        .strip_prefix("sha256:")
        .context("digest is not SHA-256")?;
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("digest is not 64 hexadecimal characters");
    }
    Ok(())
}

fn is_availability_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    ) || status.is_server_error()
}

fn github_status_error(status: StatusCode, headers: &HeaderMap, body: &str) -> anyhow::Error {
    let reset = headers
        .get("x-ratelimit-reset")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .and_then(chrono::DateTime::from_timestamp_secs)
        .map(|value| value.to_rfc3339());
    let body = body.trim();
    let body = if body.chars().count() > 512 {
        format!("{}…", body.chars().take(512).collect::<String>())
    } else {
        body.to_string()
    };

    match (reset, body.is_empty()) {
        (Some(reset), false) => anyhow::anyhow!(
            "GitHub releases/latest returned {status}; rate limit resets at {reset}; response: {body}"
        ),
        (Some(reset), true) => anyhow::anyhow!(
            "GitHub releases/latest returned {status}; rate limit resets at {reset}"
        ),
        (None, false) => {
            anyhow::anyhow!("GitHub releases/latest returned {status}; response: {body}")
        }
        (None, true) => anyhow::anyhow!("GitHub releases/latest returned {status}"),
    }
}

#[cfg(test)]
mod tests {
    use reqwest::{StatusCode, header::HeaderMap};

    use super::{
        ManifestAsset, UpdateManifest, github_status_error, is_availability_status,
        manifest_into_release, parse_expanded_assets, version_from_release_url,
    };

    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn parses_valid_update_manifest() {
        let release = manifest_into_release(UpdateManifest {
            schema_version: 1,
            version: "v1.1.9".to_string(),
            notes_url: None,
            assets: vec![ManifestAsset {
                name: "tiny-shell-v1.1.9-windows-x86_64-portable.zip".to_string(),
                url: "https://github.com/ynx-official/tiny-shell/releases/download/v1.1.9/tiny-shell-v1.1.9-windows-x86_64-portable.zip".to_string(),
                size: 42,
                digest: DIGEST.to_string(),
            }],
        })
        .unwrap();

        assert_eq!(release.version, "v1.1.9");
        assert_eq!(release.assets[0].size, 42);
        assert_eq!(release.assets[0].digest, DIGEST);
    }

    #[test]
    fn rejects_manifest_with_invalid_digest() {
        let error = manifest_into_release(UpdateManifest {
            schema_version: 1,
            version: "v1.1.9".to_string(),
            notes_url: None,
            assets: vec![ManifestAsset {
                name: "tiny-shell.zip".to_string(),
                url: "https://github.com/ynx-official/tiny-shell/releases/download/v1.1.9/tiny-shell.zip".to_string(),
                size: 42,
                digest: "sha256:bad".to_string(),
            }],
        })
        .unwrap_err();

        assert!(error.to_string().contains("invalid digest"));
    }

    #[test]
    fn rejects_manifest_asset_from_different_release() {
        let error = manifest_into_release(UpdateManifest {
            schema_version: 1,
            version: "v1.1.9".to_string(),
            notes_url: None,
            assets: vec![ManifestAsset {
                name: "tiny-shell.zip".to_string(),
                url: "https://github.com/ynx-official/tiny-shell/releases/download/v1.1.8/tiny-shell.zip".to_string(),
                size: 42,
                digest: DIGEST.to_string(),
            }],
        })
        .unwrap_err();

        assert!(error.to_string().contains("does not match release v1.1.9"));
    }

    #[test]
    fn classifies_only_availability_http_errors_for_fallback() {
        assert!(is_availability_status(StatusCode::FORBIDDEN));
        assert!(is_availability_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_availability_status(StatusCode::BAD_GATEWAY));
        assert!(!is_availability_status(StatusCode::NOT_FOUND));
        assert!(!is_availability_status(StatusCode::UNPROCESSABLE_ENTITY));
    }

    #[test]
    fn rate_limit_error_contains_reset_time_and_response() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-reset", "1785376992".parse().unwrap());
        let error = github_status_error(
            StatusCode::FORBIDDEN,
            &headers,
            r#"{"message":"API rate limit exceeded"}"#,
        );
        let message = error.to_string();

        assert!(message.contains("403 Forbidden"));
        assert!(message.contains("2026-07-30T02:03:12+00:00"));
        assert!(message.contains("API rate limit exceeded"));
    }

    #[test]
    fn extracts_tag_from_latest_release_redirect() {
        assert_eq!(
            version_from_release_url(
                "https://github.com/ynx-official/tiny-shell/releases/tag/v1.1.8"
            )
            .unwrap(),
            "v1.1.8"
        );
    }

    #[test]
    fn rejects_latest_release_redirect_to_other_host() {
        let error = version_from_release_url(
            "https://example.com/ynx-official/tiny-shell/releases/tag/v1.1.8",
        )
        .unwrap_err();
        assert!(error.to_string().contains("unexpected host"));
    }

    #[test]
    fn parses_digest_from_same_release_asset_item() {
        let html = format!(
            r#"<ul>
<li><a href="/ynx-official/tiny-shell/releases/download/v1.1.8/tiny-shell-v1.1.8-windows-x86_64-portable.zip">portable</a><span>{DIGEST}</span></li>
<li><a href="/ynx-official/tiny-shell/releases/download/v1.1.8/tiny-shell-v1.1.8-windows-x86_64-setup.exe">setup</a><span>sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff</span></li>
</ul>"#
        );
        let assets = parse_expanded_assets(&html, "v1.1.8").unwrap();

        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].digest, DIGEST);
        assert_eq!(assets[0].size, 0);
        assert!(assets[0].download_url.ends_with("-portable.zip"));
    }

    #[test]
    fn rejects_asset_item_without_its_own_digest() {
        let html = format!(
            r#"<ul>
<li><a href="/ynx-official/tiny-shell/releases/download/v1.1.8/tiny-shell-v1.1.8-windows-x86_64-portable.zip">portable</a></li>
<li><span>{DIGEST}</span></li>
</ul>"#
        );
        let error = parse_expanded_assets(&html, "v1.1.8").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not provide a SHA-256 digest")
        );
    }
}
