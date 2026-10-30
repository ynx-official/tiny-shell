use super::*;

fn release(version: &str, digest: &str) -> ReleaseMetadata {
    ReleaseMetadata {
        version: version.into(),
        notes: String::new(),
        assets: vec![super::super::release_source::ReleaseAssetMetadata {
            name: "test.zip".into(),
            download_url: format!(
                "https://github.com/ynx-official/tiny-shell/releases/download/{version}/test.zip"
            ),
            size: 10,
            digest: digest.into(),
        }],
    }
}

fn candidate(mode: RouteMode, version: &str, ms: u64, trusted: bool) -> Candidate {
    Candidate {
        entry: ProbeEntry {
            route: Route::builtin(mode),
            elapsed_ms: Some(ms),
            state: ProbeState::Available,
        },
        release: Some(release(version, "sha256:abcd")),
        trusted,
    }
}

#[test]
fn highest_trusted_version_wins_before_latency() {
    let report = select_candidates(
        vec![
            candidate(RouteMode::Github, "v2.3.0", 900, true),
            candidate(RouteMode::GhProxy, "v2.2.1", 10, true),
            candidate(RouteMode::GhFast, "v2.3.0", 100, true),
        ],
        RouteMode::Auto,
    );
    assert_eq!(report.routes[0].mode, RouteMode::GhFast);
    assert_eq!(report.routes.len(), 2);
    assert_eq!(report.entries[1].state, ProbeState::Stale);
}

#[test]
fn unsigned_proxy_requires_matching_official_assets() {
    let mut fake = candidate(RouteMode::GhFast, "v2.3.0", 1, false);
    fake.release.as_mut().unwrap().assets[0].digest = "sha256:forged".into();
    let report = select_candidates(
        vec![
            candidate(RouteMode::Github, "v2.3.0", 900, true),
            candidate(RouteMode::GhProxy, "v2.3.0", 10, false),
            fake,
        ],
        RouteMode::Auto,
    );
    assert_eq!(report.routes[0].mode, RouteMode::GhProxy);
    assert_eq!(report.entries[2].state, ProbeState::Invalid);
    let report = select_candidates(
        vec![candidate(RouteMode::GhProxy, "v9.0.0", 1, false)],
        RouteMode::Auto,
    );
    assert!(report.release.is_none());
    assert_eq!(report.entries[0].state, ProbeState::Unverified);
}

#[test]
fn fixed_route_does_not_fall_back_to_github() {
    let report = select_candidates(
        vec![
            candidate(RouteMode::Github, "v2.3.0", 900, true),
            candidate(RouteMode::GhProxy, "v2.2.1", 10, false),
        ],
        RouteMode::GhProxy,
    );
    assert!(report.routes.is_empty());
    assert!(report.release.is_none());
}

#[test]
fn automatic_mode_probes_every_custom_route_but_fixed_mode_uses_only_its_selection() {
    let prefs = RoutePreferences::default()
        .with_added_custom("https://example.com/a")
        .unwrap()
        .with_added_custom("https://example.com/b")
        .unwrap();
    assert_eq!(configured_routes(&prefs).unwrap().len(), 5);
    let fixed = prefs
        .with_selected_custom("https://example.com/b/")
        .unwrap();
    let routes = configured_routes(&fixed).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].prefix, "https://example.com/b/");
    assert_eq!(routes[0].label(), "example.com/b");
    let after_remove = fixed.without_custom("https://example.com/b/");
    assert_eq!(configured_routes(&after_remove).unwrap().len(), 4);
}

#[test]
fn verified_signature_route_can_work_without_github() {
    let report = select_candidates(
        vec![candidate(RouteMode::GhProxy, "v2.3.0", 10, true)],
        RouteMode::GhProxy,
    );
    assert_eq!(report.routes.len(), 1);
    assert_eq!(report.release.unwrap().version, "v2.3.0");
}

#[test]
fn integrity_and_local_errors_are_not_retryable() {
    for message in [
        "SHA-256 mismatch",
        "size mismatch",
        "failed to write payload",
        "update download cancelled",
    ] {
        assert!(!retryable_download_error(&anyhow::anyhow!(message)));
    }
}

#[tokio::test]
async fn download_http_errors_only_retry_availability_failures() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    for (status, retry) in [
        (404, true),
        (403, true),
        (429, true),
        (500, true),
        (503, true),
        (400, false),
        (401, false),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let response = client
            .get(format!("http://{address}/asset"))
            .send()
            .await
            .unwrap();
        let error = response.error_for_status().unwrap_err();
        assert_eq!(retryable_download_error(&anyhow::Error::new(error)), retry);
        server.await.unwrap();
    }
}

#[test]
fn route_configuration_and_urls_preserve_the_trust_boundary() {
    let prefs = RoutePreferences::default();
    assert_eq!(configured_routes(&prefs).unwrap().len(), 3);
    let prefs = RoutePreferences {
        mode: RouteMode::Github,
        custom_prefixes: vec!["invalid leftover".into()],
        ..Default::default()
    };
    assert_eq!(configured_routes(&prefs).unwrap(), vec![Route::github()]);
    for invalid in [
        "http://example.com",
        "https://u:p@example.com",
        "https://example.com/?key=x",
        "https://example.com/#x",
        "https://localhost",
        "https://127.0.0.1",
        "https://[::1]",
    ] {
        assert!(normalize_custom(invalid).is_err(), "{invalid}");
    }
    assert_eq!(
        normalize_custom(" https://example.com/proxy ").unwrap(),
        "https://example.com/proxy/"
    );
    let route = Route::builtin(RouteMode::GhProxy);
    assert!(
        route
            .url("https://github.com/other/repo/releases/latest/download/a")
            .is_err()
    );
    assert_eq!(
        route.url(MANIFEST_URL).unwrap(),
        format!("https://gh-proxy.com/{MANIFEST_URL}")
    );
}

#[test]
fn redirects_cannot_leave_the_route_or_github_allowlist() {
    let route = Route::builtin(RouteMode::GhProxy);
    for invalid in [
        "http://gh-proxy.com/a",
        "https://evil.example/a",
        "https://github.com:444/a",
        "https://user@github.com/a",
    ] {
        assert!(!route.allows_redirect(&Url::parse(invalid).unwrap()));
    }
    assert!(route.allows_redirect(
        &Url::parse("https://release-assets.githubusercontent.com/a?token=cdn").unwrap()
    ));
}
