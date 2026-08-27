use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum HighlightMatchKind {
    #[default]
    Literal,
    Regex,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum HighlightTarget {
    #[default]
    Match,
    Line,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct HighlightRuleStyle {
    #[serde(default)]
    pub foreground: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub underline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct HighlightRule {
    pub id: String,
    pub name: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub pattern: String,
    #[serde(default)]
    pub match_kind: HighlightMatchKind,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub target: HighlightTarget,
    #[serde(default)]
    pub style: HighlightRuleStyle,
}

fn enabled_by_default() -> bool {
    true
}

fn recommended_rule(
    id: &str,
    name: &str,
    pattern: &str,
    whole_word: bool,
    style: HighlightRuleStyle,
) -> HighlightRule {
    HighlightRule {
        id: id.to_string(),
        name: name.to_string(),
        enabled: true,
        pattern: pattern.to_string(),
        match_kind: HighlightMatchKind::Regex,
        case_sensitive: false,
        whole_word,
        target: HighlightTarget::Match,
        style,
    }
}

fn recommended_style(
    foreground: &str,
    background: Option<&str>,
    bold: bool,
    underline: bool,
) -> HighlightRuleStyle {
    HighlightRuleStyle {
        foreground: Some(foreground.to_string()),
        background: background.map(str::to_string),
        bold,
        underline,
    }
}

/// Returns the conservative built-in rules used for new and legacy configurations.
///
/// These rules intentionally avoid short, ambiguous terms such as `OK`, `UP`,
/// and `PASS`, which otherwise create frequent false positives in terminal output.
pub fn default_highlight_rules() -> Vec<HighlightRule> {
    vec![
        recommended_rule(
            "builtin-critical",
            "Critical",
            "PANIC|FATAL|CRITICAL|EMERGENCY|OOM|SEGFAULT|ASSERTION FAILED|ABORTED|DEADLOCK|CORRUPTION",
            true,
            recommended_style("#FF5F56", Some("#FF323233"), true, false),
        ),
        recommended_rule(
            "builtin-error",
            "Error",
            "ERROR|ERR|FAILED|FAILURE|EXCEPTION|TRACEBACK|DENIED|REFUSED|TIMEOUT|TIMED OUT|UNREACHABLE|DOWN|STOPPED|INACTIVE|INVALID|UNAUTHORIZED|FORBIDDEN|NOT FOUND|NOT_FOUND|PERMISSION DENIED|CONNECTION RESET|BROKEN PIPE|NO SUCH FILE|KILLED|TERMINATED|ROLLBACK|ROLLED BACK|CANCELLED|CANCELED",
            true,
            recommended_style("#E85D68", Some("#E0606026"), true, false),
        ),
        recommended_rule(
            "builtin-warning",
            "Warning",
            "WARNING|WARN|DEPRECATED|RETRY|RETRYING|THROTTLED|DEGRADED|CAUTION|SKIPPED|UNSUPPORTED|OBSOLETE|UNAVAILABLE|PENDING|BACKOFF",
            true,
            recommended_style("#DFAF45", Some("#E8C97A1F"), true, false),
        ),
        recommended_rule(
            "builtin-success",
            "Success",
            "SUCCESS|SUCCEEDED|PASSED|COMPLETED|READY|HEALTHY|RUNNING|ACTIVE|DONE|ONLINE|CONNECTED|ENABLED|INSTALLED|DEPLOYED",
            true,
            recommended_style("#52B788", None, true, false),
        ),
        recommended_rule(
            "builtin-info",
            "Info",
            "INFO|NOTICE|STARTING|STARTED|RESTARTING|STOPPING|SHUTTING DOWN|EXITED",
            true,
            recommended_style("#4EA1F3", None, false, false),
        ),
        recommended_rule(
            "builtin-debug",
            "Debug",
            "DEBUG|TRACE|VERBOSE",
            true,
            recommended_style("#7C8494", None, false, false),
        ),
        recommended_rule(
            "builtin-http-errors",
            "HTTP errors",
            "HTTP(?:/[0-9.]+)?[ \\t]+[45][0-9]{2}|STATUS(?:=|:)[ \\t]*[45][0-9]{2}",
            true,
            recommended_style("#E8874A", Some("#E8A87C1F"), true, false),
        ),
        recommended_rule(
            "builtin-http-method",
            "HTTP method",
            "GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS|CONNECT",
            true,
            recommended_style("#4EA1F3", None, false, false),
        ),
        recommended_rule(
            "builtin-url",
            "Web URL",
            r#"https?://[^\s<>"']*[^\s<>"'.,;:!?)}\]]"#,
            false,
            recommended_style("#4EA1F3", None, false, true),
        ),
        recommended_rule(
            "builtin-email",
            "Email address",
            r"[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,63}",
            true,
            recommended_style("#4EA1F3", None, false, false),
        ),
        recommended_rule(
            "builtin-ipv4",
            "IPv4 address",
            "(?:(?:25[0-5]|2[0-4][0-9]|1?[0-9]{1,2})\\.){3}(?:25[0-5]|2[0-4][0-9]|1?[0-9]{1,2})",
            true,
            recommended_style("#5BC0EB", None, false, false),
        ),
        recommended_rule(
            "builtin-mac",
            "MAC address",
            "(?:[0-9A-F]{2}[:-]){5}[0-9A-F]{2}",
            true,
            recommended_style("#5BC0EB", None, false, false),
        ),
        recommended_rule(
            "builtin-ipv6",
            "IPv6 address",
            concat!(
                "(?:",
                "(?:[0-9A-F]{1,4}:){7}[0-9A-F]{1,4}|",
                "(?:[0-9A-F]{1,4}:){1,6}:[0-9A-F]{1,4}|",
                "(?:[0-9A-F]{1,4}:){1,5}(?::[0-9A-F]{1,4}){1,2}|",
                "(?:[0-9A-F]{1,4}:){1,4}(?::[0-9A-F]{1,4}){1,3}|",
                "(?:[0-9A-F]{1,4}:){1,3}(?::[0-9A-F]{1,4}){1,4}|",
                "(?:[0-9A-F]{1,4}:){1,2}(?::[0-9A-F]{1,4}){1,5}|",
                "[0-9A-F]{1,4}:(?:(?::[0-9A-F]{1,4}){1,6})|",
                "(?:[0-9A-F]{1,4}:){1,7}:|",
                ":(?:(?::[0-9A-F]{1,4}){1,7}|:)",
                ")"
            ),
            true,
            recommended_style("#5BC0EB", None, false, false),
        ),
        recommended_rule(
            "builtin-uuid",
            "UUID",
            "[0-9A-F]{8}-[0-9A-F]{4}-[1-8][0-9A-F]{3}-[89AB][0-9A-F]{3}-[0-9A-F]{12}",
            true,
            recommended_style("#7C8494", None, false, false),
        ),
        recommended_rule(
            "builtin-timestamp",
            "ISO timestamp",
            "[0-9]{4}-[0-9]{2}-[0-9]{2}[T ][0-2][0-9]:[0-5][0-9]:[0-5][0-9](?:\\.[0-9]+)?(?:Z|[+-][0-9]{2}:?[0-9]{2})?",
            true,
            recommended_style("#7C8494", None, false, false),
        ),
        recommended_rule(
            "builtin-file-path",
            "File path",
            r#"[A-Z]:\\(?:[^\s\\/:*?"<>|]+\\)*[^\s\\/:*?"<>|]+|(?:~|\.\.?)?/(?:[A-Z0-9._~-]+/)*[A-Z0-9._~-]+"#,
            true,
            recommended_style("#4EA1F3", None, false, false),
        ),
    ]
}

fn legacy_default_highlight_rules_v1() -> Vec<HighlightRule> {
    let mut rules = default_highlight_rules();
    rules.truncate(7);
    let legacy = [
        ("PANIC|FATAL|CRITICAL|EMERGENCY|OOM", "#FF3232", true),
        (
            "ERROR|ERR|FAILED|FAILURE|EXCEPTION|TRACEBACK",
            "#E06060",
            false,
        ),
        ("WARNING|WARN|DEPRECATED", "#E8C97A", false),
        ("SUCCESS|SUCCEEDED|PASSED|COMPLETED", "#7EC699", false),
        ("INFO|NOTICE", "#6CB4EE", false),
        ("DEBUG|TRACE", "#828C9B", false),
        ("[45][0-9]{2}", "#E8A87C", false),
    ];
    for (rule, (pattern, foreground, bold)) in rules.iter_mut().zip(legacy) {
        rule.pattern = pattern.to_string();
        rule.style = recommended_style(foreground, None, bold, false);
    }
    rules
}

fn legacy_default_highlight_rules_v2() -> Vec<HighlightRule> {
    let legacy_ids = [
        "builtin-critical",
        "builtin-error",
        "builtin-warning",
        "builtin-success",
        "builtin-info",
        "builtin-debug",
        "builtin-http-errors",
        "builtin-url",
        "builtin-ipv4",
    ];
    let mut rules = default_highlight_rules()
        .into_iter()
        .filter(|rule| legacy_ids.contains(&rule.id.as_str()))
        .collect::<Vec<_>>();
    let legacy_patterns = [
        "PANIC|FATAL|CRITICAL|EMERGENCY|OOM|SEGFAULT|ASSERTION FAILED",
        "ERROR|ERR|FAILED|FAILURE|EXCEPTION|TRACEBACK|DENIED|REFUSED|TIMEOUT|TIMED OUT|UNREACHABLE|DOWN|STOPPED|INACTIVE",
        "WARNING|WARN|DEPRECATED|RETRY|RETRYING|THROTTLED|DEGRADED",
        "SUCCESS|SUCCEEDED|PASSED|COMPLETED|READY|HEALTHY|RUNNING|ACTIVE",
        "INFO|NOTICE",
        "DEBUG|TRACE",
        "HTTP(?:/[0-9.]+)?[ \\t]+[45][0-9]{2}|STATUS(?:=|:)[ \\t]*[45][0-9]{2}",
        r#"https?://[^\s<>"']*[^\s<>"'.,;:!?)}\]]"#,
        "(?:(?:25[0-5]|2[0-4][0-9]|1?[0-9]{1,2})\\.){3}(?:25[0-5]|2[0-4][0-9]|1?[0-9]{1,2})",
    ];
    for (rule, pattern) in rules.iter_mut().zip(legacy_patterns) {
        rule.pattern = pattern.to_string();
    }
    rules
}

/// Upgrade only the untouched original built-in set. Any user edit, deletion,
/// reordering, or custom rule keeps the saved list byte-for-byte intact.
pub fn upgrade_builtin_highlight_rules(rules: &mut Vec<HighlightRule>) {
    if *rules == legacy_default_highlight_rules_v1()
        || *rules == legacy_default_highlight_rules_v2()
    {
        *rules = default_highlight_rules();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn recommended_rules_are_high_signal_and_have_stable_unique_ids() {
        let rules = default_highlight_rules();

        assert_eq!(rules.len(), 16);
        assert!(rules.iter().all(|rule| rule.enabled));
        assert!(
            rules
                .iter()
                .filter(|rule| rule.id != "builtin-url")
                .all(|rule| rule.whole_word)
        );
        assert!(
            rules
                .iter()
                .all(|rule| rule.target == HighlightTarget::Match)
        );
        assert_eq!(
            rules
                .iter()
                .map(|rule| rule.id.as_str())
                .collect::<HashSet<_>>()
                .len(),
            rules.len()
        );
        assert_eq!(
            rules
                .iter()
                .map(|rule| rule.id.as_str())
                .collect::<Vec<_>>(),
            [
                "builtin-critical",
                "builtin-error",
                "builtin-warning",
                "builtin-success",
                "builtin-info",
                "builtin-debug",
                "builtin-http-errors",
                "builtin-http-method",
                "builtin-url",
                "builtin-email",
                "builtin-ipv4",
                "builtin-mac",
                "builtin-ipv6",
                "builtin-uuid",
                "builtin-timestamp",
                "builtin-file-path",
            ]
        );
        assert!(
            rules
                .iter()
                .all(|rule| rule.match_kind == HighlightMatchKind::Regex)
        );
        assert!(rules.iter().take(4).all(|rule| rule.style.bold));
        assert!(
            rules
                .iter()
                .take(3)
                .all(|rule| rule.style.background.is_some())
        );
        assert!(
            rules
                .iter()
                .find(|rule| rule.id == "builtin-url")
                .is_some_and(|rule| rule.style.underline && !rule.whole_word)
        );
    }

    #[test]
    fn rule_serialization_uses_readable_enum_values_and_preserves_style() {
        let rule = HighlightRule {
            id: "custom-timeout".to_string(),
            name: "Timeout".to_string(),
            enabled: false,
            pattern: "timed out".to_string(),
            match_kind: HighlightMatchKind::Literal,
            case_sensitive: true,
            whole_word: false,
            target: HighlightTarget::Line,
            style: HighlightRuleStyle {
                foreground: Some("#112233".to_string()),
                background: Some("#445566".to_string()),
                bold: true,
                underline: true,
            },
        };

        let json = serde_json::to_value(&rule).unwrap();
        assert_eq!(json["match_kind"], "literal");
        assert_eq!(json["target"], "line");
        assert_eq!(json["style"]["background"], "#445566");
        assert_eq!(serde_json::from_value::<HighlightRule>(json).unwrap(), rule);
    }

    #[test]
    fn untouched_original_defaults_upgrade_but_user_changes_are_preserved() {
        let mut untouched = legacy_default_highlight_rules_v1();
        upgrade_builtin_highlight_rules(&mut untouched);
        assert_eq!(untouched, default_highlight_rules());

        let mut previous_defaults = legacy_default_highlight_rules_v2();
        upgrade_builtin_highlight_rules(&mut previous_defaults);
        assert_eq!(previous_defaults, default_highlight_rules());

        let mut customized_previous_defaults = legacy_default_highlight_rules_v2();
        customized_previous_defaults[1].pattern.push_str("|CUSTOM");
        let expected = customized_previous_defaults.clone();
        upgrade_builtin_highlight_rules(&mut customized_previous_defaults);
        assert_eq!(customized_previous_defaults, expected);

        let mut customized = legacy_default_highlight_rules_v1();
        customized[1].style.foreground = Some("#123456".to_string());
        let expected = customized.clone();
        upgrade_builtin_highlight_rules(&mut customized);
        assert_eq!(customized, expected);

        let mut empty = Vec::new();
        upgrade_builtin_highlight_rules(&mut empty);
        assert!(empty.is_empty());
    }
}
