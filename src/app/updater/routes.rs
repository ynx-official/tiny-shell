//! A route transports release bytes; it never decides which bytes are trusted.
use std::time::{Duration, Instant};

use anyhow::{Context, ensure};
use futures::future::join_all;
use reqwest::{Client, StatusCode, Url};

use super::{
    release_source::{self, ReleaseMetadata},
    signing,
};
pub(crate) use crate::session::update_preferences::{
    RouteMode, RoutePreferences, normalize_custom,
};

const MANIFEST_URL: &str =
    "https://github.com/ynx-official/tiny-shell/releases/latest/download/update-manifest.json";
const RELEASE_ROOT: &str = "https://github.com/ynx-official/tiny-shell/releases/";
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const METADATA_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Route {
    pub mode: RouteMode,
    pub prefix: String,
}

impl Route {
    pub fn github() -> Self {
        Self::builtin(RouteMode::Github)
    }

    fn builtin(mode: RouteMode) -> Self {
        Self {
            mode,
            prefix: match mode {
                RouteMode::GhProxy => "https://gh-proxy.com/",
                RouteMode::GhFast => "https://ghfast.top/",
                _ => "",
            }
            .into(),
        }
    }

    pub fn label(&self) -> String {
        if self.mode == RouteMode::Custom {
            self.prefix
                .strip_prefix("https://")
                .unwrap_or(&self.prefix)
                .trim_end_matches('/')
                .to_string()
        } else {
            rust_i18n::t!(self.mode.label_key()).to_string()
        }
    }

    pub fn url(&self, canonical: &str) -> anyhow::Result<String> {
        ensure!(
            canonical.starts_with(RELEASE_ROOT)
                && !canonical.contains(['?', '#', '\\'])
                && !canonical.contains("/../"),
            "invalid canonical update URL"
        );
        if self.mode == RouteMode::Github {
            return Ok(canonical.into());
        }
        Ok(format!("{}{canonical}", normalize_custom(&self.prefix)?))
    }

    fn allows_redirect(&self, url: &Url) -> bool {
        let official = url.port_or_known_default() == Some(443)
            && [
                "github.com",
                "api.github.com",
                "release-assets.githubusercontent.com",
                "objects.githubusercontent.com",
                "github-releases.githubusercontent.com",
                "raw.githubusercontent.com",
            ]
            .contains(&url.host_str().unwrap_or_default());
        let same_origin =
            Url::parse(&self.prefix).is_ok_and(|prefix| prefix.origin() == url.origin());
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && (official || same_origin)
    }

    pub fn client(&self) -> anyhow::Result<Client> {
        let route = self.clone();
        Client::builder()
            .user_agent(format!("tiny-shell/{}", super::CURRENT_VERSION))
            .connect_timeout(super::HTTP_CONNECT_TIMEOUT)
            .read_timeout(super::HTTP_READ_TIMEOUT)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 6 || !route.allows_redirect(attempt.url()) {
                    attempt.error("update redirect is not allowed")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .context("failed to build update client")
    }
}

pub(crate) fn configured_routes(prefs: &RoutePreferences) -> anyhow::Result<Vec<Route>> {
    let mut routes = match prefs.mode {
        RouteMode::Auto => vec![
            Route::github(),
            Route::builtin(RouteMode::GhProxy),
            Route::builtin(RouteMode::GhFast),
        ],
        RouteMode::Custom => Vec::new(),
        mode => vec![Route::builtin(mode)],
    };
    let prefixes: Vec<_> = match prefs.mode {
        RouteMode::Auto => prefs.custom_prefixes.iter().collect(),
        RouteMode::Custom => vec![
            prefs
                .selected_custom_prefix
                .as_ref()
                .filter(|prefix| prefs.custom_prefixes.contains(prefix))
                .context("select a saved custom update route")?,
        ],
        _ => Vec::new(),
    };
    for prefix in prefixes {
        let prefix = normalize_custom(prefix)?;
        if !routes.iter().any(|route| route.prefix == prefix) {
            routes.push(Route {
                mode: RouteMode::Custom,
                prefix,
            });
        }
    }
    Ok(routes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeState {
    Available,
    Timeout,
    Failed,
    Invalid,
    Unverified,
    Stale,
}

impl ProbeState {
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Available => "update_route_available",
            Self::Timeout => "update_route_timeout",
            Self::Failed => "update_route_failed",
            Self::Invalid => "update_route_invalid",
            Self::Unverified => "update_route_unverified",
            Self::Stale => "update_route_stale",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProbeEntry {
    pub route: Route,
    pub elapsed_ms: Option<u64>,
    pub state: ProbeState,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProbeReport {
    pub entries: Vec<ProbeEntry>,
    pub routes: Vec<Route>,
    pub(super) release: Option<ReleaseMetadata>,
}

struct Candidate {
    entry: ProbeEntry,
    release: Option<ReleaseMetadata>,
    trusted: bool,
}

async fn response_bytes(mut response: reqwest::Response, limit: usize) -> anyhow::Result<Vec<u8>> {
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= limit as u64),
        "update metadata is too large"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= limit,
            "update metadata is too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn metadata_read_error(error: anyhow::Error) -> ProbeState {
    if error.downcast_ref::<reqwest::Error>().is_some() {
        ProbeState::Failed
    } else {
        ProbeState::Invalid
    }
}

async fn probe(route: Route) -> Candidate {
    let started = Instant::now();
    let result = tokio::time::timeout(PROBE_TIMEOUT, async {
        let client = route.client().map_err(|_| ProbeState::Failed)?;
        let url = route.url(MANIFEST_URL).map_err(|_| ProbeState::Invalid)?;
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|_| ProbeState::Failed)?;
        let response = response
            .error_for_status()
            .map_err(|_| ProbeState::Failed)?;
        let bytes = response_bytes(response, METADATA_LIMIT)
            .await
            .map_err(metadata_read_error)?;
        let release =
            release_source::parse_manifest_bytes(&bytes).map_err(|_| ProbeState::Invalid)?;
        let mut trusted = route.mode == RouteMode::Github;
        if let Some(key) = signing::PUBLIC_KEY.filter(|key| !key.trim().is_empty()) {
            let signature_url = format!(
                "{RELEASE_ROOT}download/{}/update-manifest.json.sig",
                release.version
            );
            let response = client
                .get(route.url(&signature_url).map_err(|_| ProbeState::Invalid)?)
                .send()
                .await
                .map_err(|_| ProbeState::Failed)?;
            if response.status() != StatusCode::NOT_FOUND {
                let response = response
                    .error_for_status()
                    .map_err(|_| ProbeState::Failed)?;
                let signature = response_bytes(response, 1024)
                    .await
                    .map_err(metadata_read_error)?;
                signing::verify(&bytes, &signature, key).map_err(|_| ProbeState::Invalid)?;
                trusted = true;
            }
        }
        Ok::<_, ProbeState>((release, trusted))
    })
    .await;
    let elapsed_ms = Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
    let (release, trusted, state) = match result {
        Ok(Ok((release, trusted))) => (Some(release), trusted, ProbeState::Available),
        Ok(Err(state)) => (None, false, state),
        Err(_) => (None, false, ProbeState::Timeout),
    };
    Candidate {
        entry: ProbeEntry {
            route,
            elapsed_ms,
            state,
        },
        release,
        trusted,
    }
}

fn same_assets(candidate: &ReleaseMetadata, official: &ReleaseMetadata) -> bool {
    candidate.version == official.version
        && !candidate.assets.is_empty()
        && candidate.assets.iter().all(|asset| {
            official.assets.iter().any(|other| {
                asset.name == other.name
                    && asset.download_url == other.download_url
                    && asset.digest.eq_ignore_ascii_case(&other.digest)
                    && (other.size == 0 || asset.size == other.size)
            })
        })
}

fn select_candidates(mut candidates: Vec<Candidate>, mode: RouteMode) -> ProbeReport {
    let official = candidates
        .iter()
        .find(|c| c.entry.route.mode == RouteMode::Github && c.trusted)
        .and_then(|c| c.release.clone());
    for candidate in &mut candidates {
        if !candidate.trusted
            && let Some(release) = &candidate.release
        {
            if let Some(official) = &official {
                if same_assets(release, official) {
                    candidate.trusted = true;
                    // An unsigned proxy cannot supply authoritative release notes.
                    candidate.release = Some(official.clone());
                } else {
                    candidate.entry.state =
                        if super::is_newer_version(&release.version, &official.version)
                            .unwrap_or(false)
                        {
                            ProbeState::Stale
                        } else {
                            ProbeState::Invalid
                        };
                }
            } else {
                candidate.entry.state = ProbeState::Unverified;
            }
        }
    }
    let newest = candidates
        .iter()
        .filter(|c| c.trusted)
        .filter_map(|c| c.release.as_ref())
        .filter_map(|r| super::parse_version(&r.version).ok())
        .max();
    // Stable order resolves equal timings deterministically (GitHub comes first).
    let mut indices: Vec<_> = (0..candidates.len()).collect();
    indices.sort_by_key(|i| candidates[*i].entry.elapsed_ms.unwrap_or(u64::MAX));
    let mut routes = Vec::new();
    let mut selected: Option<ReleaseMetadata> = None;
    for i in indices {
        let candidate = &mut candidates[i];
        if !candidate.trusted {
            continue;
        }
        let Some(release) = &candidate.release else {
            continue;
        };
        if super::parse_version(&release.version).ok() != newest {
            candidate.entry.state = ProbeState::Stale;
            continue;
        }
        if mode != RouteMode::Auto && candidate.entry.route.mode != mode {
            continue;
        }
        if let Some(selected) = &selected {
            if !same_assets(release, selected) || !same_assets(selected, release) {
                candidate.entry.state = ProbeState::Invalid;
                continue;
            }
        } else {
            selected = Some(release.clone());
        }
        routes.push(candidate.entry.route.clone());
    }
    ProbeReport {
        entries: candidates
            .into_iter()
            .filter(|c| mode == RouteMode::Auto || c.entry.route.mode == mode)
            .map(|c| c.entry)
            .collect(),
        routes,
        release: selected,
    }
}

pub(crate) async fn probe_routes(prefs: &RoutePreferences) -> anyhow::Result<ProbeReport> {
    let mut routes = configured_routes(prefs)?;
    // Even a fixed unsigned mirror needs GitHub corroboration; it is not a download fallback.
    if !routes.iter().any(|route| route.mode == RouteMode::Github) {
        routes.push(Route::github());
    }
    let mut candidates = join_all(routes.into_iter().map(probe)).await;
    let needs_fallback = !candidates
        .iter()
        .any(|candidate| candidate.trusted && candidate.release.is_some());
    if needs_fallback
        && let Some(direct) = candidates
            .iter_mut()
            .find(|c| c.entry.route.mode == RouteMode::Github)
        && matches!(direct.entry.state, ProbeState::Failed | ProbeState::Timeout)
    {
        let started = Instant::now();
        let client = Route::github().client()?;
        if let Ok(Ok(release)) = tokio::time::timeout(
            Duration::from_secs(10),
            release_source::fetch_latest_release(&client),
        )
        .await
        {
            direct.release = Some(release);
            direct.trusted = true;
            direct.entry.state = ProbeState::Available;
            direct.entry.elapsed_ms = Some(
                direct
                    .entry
                    .elapsed_ms
                    .unwrap_or(0)
                    .saturating_add(started.elapsed().as_millis() as u64),
            );
        }
    }
    let mut report = select_candidates(candidates, prefs.mode);
    if let Some(release) = &mut report.release
        && release.notes.is_empty()
    {
        // Old manifests do not authenticate notes. Only retrieve them from GitHub.
        let client = Route::github().client()?;
        let url = format!(
            "{RELEASE_ROOT}download/{}/release-notes.txt",
            release.version
        );
        if let Ok(notes) = tokio::time::timeout(
            PROBE_TIMEOUT,
            release_source::fetch_optional_notes(&client, &url),
        )
        .await
        {
            release.notes = notes;
        }
    }
    Ok(report)
}

/// Integrity, redirect and local preparation errors must not be hidden by retry.
pub(super) fn retryable_download_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<reqwest::Error>())
        .any(|error| {
            !error.is_redirect()
                && (error.is_timeout()
                    || error.is_connect()
                    || error.is_body()
                    || error.status().is_some_and(|status| {
                        status == StatusCode::NOT_FOUND
                            || status == StatusCode::FORBIDDEN
                            || status == StatusCode::TOO_MANY_REQUESTS
                            || status.is_server_error()
                    }))
        })
}

#[cfg(test)]
mod tests;
