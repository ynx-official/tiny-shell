use anyhow::{Context, ensure};
use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteMode {
    #[default]
    Auto,
    Github,
    GhProxy,
    GhFast,
    Custom,
}

impl RouteMode {
    pub const BUILTIN: [Self; 4] = [Self::Auto, Self::Github, Self::GhProxy, Self::GhFast];

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Auto => "update_route_auto",
            Self::Github => "update_route_github",
            Self::GhProxy => "update_route_gh_proxy",
            Self::GhFast => "update_route_gh_fast",
            Self::Custom => "update_route_custom",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RoutePreferences {
    pub mode: RouteMode,
    pub custom_prefixes: Vec<String>,
    pub selected_custom_prefix: Option<String>,
}

impl<'de> Deserialize<'de> for RoutePreferences {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct Stored {
            mode: RouteMode,
            custom_prefix: String,
            custom_prefixes: Option<Vec<String>>,
            selected_custom_prefix: Option<String>,
        }
        let stored = Stored::deserialize(deserializer)?;
        let legacy = stored.custom_prefixes.is_none();
        let raw = stored
            .custom_prefixes
            .unwrap_or_else(|| vec![stored.custom_prefix.clone()]);
        let mut custom_prefixes = Vec::new();
        for value in raw {
            if value.trim().is_empty() {
                continue;
            }
            // Keep malformed historical values visible so users can remove them.
            let value = normalize_custom(&value).unwrap_or(value);
            if !custom_prefixes.contains(&value) {
                custom_prefixes.push(value);
            }
        }
        let selected = stored
            .selected_custom_prefix
            .or_else(|| {
                (legacy && stored.mode == RouteMode::Custom).then_some(stored.custom_prefix)
            })
            .map(|value| normalize_custom(&value).unwrap_or(value));
        let selected_custom_prefix = selected.filter(|value| custom_prefixes.contains(value));
        let mode = if stored.mode == RouteMode::Custom && selected_custom_prefix.is_none() {
            RouteMode::Auto
        } else {
            stored.mode
        };
        Ok(Self {
            mode,
            custom_prefixes,
            selected_custom_prefix,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePreferenceError {
    Invalid,
    Duplicate,
    Missing,
}

impl RoutePreferences {
    pub fn with_added_custom(&self, value: &str) -> Result<Self, RoutePreferenceError> {
        let prefix = normalize_custom(value).map_err(|_| RoutePreferenceError::Invalid)?;
        if self.custom_prefixes.contains(&prefix)
            || [
                "https://github.com/",
                "https://gh-proxy.com/",
                "https://ghfast.top/",
            ]
            .contains(&prefix.as_str())
        {
            return Err(RoutePreferenceError::Duplicate);
        }
        let mut next = self.clone();
        next.custom_prefixes.push(prefix);
        Ok(next)
    }

    pub fn with_selected_custom(&self, prefix: &str) -> Result<Self, RoutePreferenceError> {
        if !self.custom_prefixes.iter().any(|value| value == prefix) {
            return Err(RoutePreferenceError::Missing);
        }
        normalize_custom(prefix).map_err(|_| RoutePreferenceError::Invalid)?;
        let mut next = self.clone();
        next.mode = RouteMode::Custom;
        next.selected_custom_prefix = Some(prefix.into());
        Ok(next)
    }

    pub fn without_custom(&self, prefix: &str) -> Self {
        let mut next = self.clone();
        next.custom_prefixes.retain(|value| value != prefix);
        if next.selected_custom_prefix.as_deref() == Some(prefix) {
            next.selected_custom_prefix = None;
            if next.mode == RouteMode::Custom {
                next.mode = RouteMode::Auto;
            }
        }
        next
    }
}

pub(crate) fn normalize_custom(value: &str) -> anyhow::Result<String> {
    let mut url = Url::parse(value.trim()).context("invalid proxy prefix")?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "invalid HTTPS proxy prefix"
    );
    let host = url.host_str().context("missing proxy host")?;
    ensure!(
        host != "localhost" && !host.ends_with(".localhost") && !host.ends_with(".local"),
        "local proxy host is not allowed"
    );
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        let local = match ip {
            std::net::IpAddr::V4(ip) => {
                ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || ip.is_broadcast()
            }
            std::net::IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
                    || ip.is_multicast()
                    || ip.to_ipv4_mapped().is_some()
            }
        };
        ensure!(!local, "local proxy address is not allowed");
    }
    let path = url.path().to_owned();
    ensure!(
        !path.contains("http:") && !path.contains("https:") && !path.contains('%'),
        "enter a proxy prefix, not a target URL"
    );
    if !path.ends_with('/') {
        url.set_path(&format!("{path}/"));
    }
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_default_to_auto_and_preferences_round_trip() {
        let legacy: RoutePreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy, RoutePreferences::default());
        let custom = RoutePreferences::default()
            .with_added_custom("https://example.com/")
            .unwrap()
            .with_selected_custom("https://example.com/")
            .unwrap();
        let restored: RoutePreferences =
            serde_json::from_str(&serde_json::to_string(&custom).unwrap()).unwrap();
        assert_eq!(restored, custom);
    }

    #[test]
    fn legacy_single_route_is_migrated_without_losing_selection() {
        let prefs: RoutePreferences = serde_json::from_str(
            r#"{"mode":"custom","custom_prefix":"https://EXAMPLE.com/proxy"}"#,
        )
        .unwrap();
        assert_eq!(prefs.custom_prefixes, ["https://example.com/proxy/"]);
        assert_eq!(
            prefs.selected_custom_prefix.as_deref(),
            Some("https://example.com/proxy/")
        );
        assert_eq!(prefs.mode, RouteMode::Custom);
        let saved = serde_json::to_value(&prefs).unwrap();
        assert!(saved.get("custom_prefix").is_none());
    }

    #[test]
    fn adding_routes_keeps_existing_entries_and_rejects_normalized_duplicates() {
        let prefs = RoutePreferences::default()
            .with_added_custom("https://example.com/a")
            .unwrap();
        let prefs = prefs.with_added_custom("https://example.com/b/").unwrap();
        assert_eq!(prefs.custom_prefixes.len(), 2);
        assert_eq!(prefs.mode, RouteMode::Auto);
        assert_eq!(
            prefs.with_added_custom(" https://EXAMPLE.com:443/a/ "),
            Err(RoutePreferenceError::Duplicate)
        );
        assert_eq!(
            prefs.with_added_custom("https://ghfast.top"),
            Err(RoutePreferenceError::Duplicate)
        );
        assert_eq!(
            prefs.with_added_custom("http://example.com"),
            Err(RoutePreferenceError::Invalid)
        );
    }

    #[test]
    fn removing_one_route_preserves_others_and_only_resets_the_active_custom_selection() {
        let prefs = RoutePreferences::default()
            .with_added_custom("https://example.com/a/")
            .unwrap()
            .with_added_custom("https://example.com/b/")
            .unwrap()
            .with_selected_custom("https://example.com/b/")
            .unwrap();
        let remaining = prefs.without_custom("https://example.com/a/");
        assert_eq!(remaining.custom_prefixes, ["https://example.com/b/"]);
        assert_eq!(remaining.mode, RouteMode::Custom);
        let remaining = prefs.without_custom("https://example.com/b/");
        assert_eq!(remaining.custom_prefixes, ["https://example.com/a/"]);
        assert_eq!(remaining.mode, RouteMode::Auto);
        assert!(remaining.selected_custom_prefix.is_none());
        assert_eq!(
            remaining.without_custom("https://missing.example/"),
            remaining
        );
    }

    #[test]
    fn explicit_empty_list_cannot_restore_a_removed_legacy_route() {
        let prefs: RoutePreferences = serde_json::from_str(r#"{"mode":"custom","custom_prefix":"https://example.com/","custom_prefixes":[],"selected_custom_prefix":"https://example.com/"}"#).unwrap();
        assert!(prefs.custom_prefixes.is_empty());
        assert_eq!(prefs.mode, RouteMode::Auto);
    }
}
