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
    foreground: &str,
    bold: bool,
) -> HighlightRule {
    HighlightRule {
        id: id.to_string(),
        name: name.to_string(),
        enabled: true,
        pattern: pattern.to_string(),
        match_kind: HighlightMatchKind::Regex,
        case_sensitive: false,
        whole_word: true,
        target: HighlightTarget::Match,
        style: HighlightRuleStyle {
            foreground: Some(foreground.to_string()),
            background: None,
            bold,
            underline: false,
        },
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
            "PANIC|FATAL|CRITICAL|EMERGENCY|OOM",
            "#FF3232",
            true,
        ),
        recommended_rule(
            "builtin-error",
            "Error",
            "ERROR|ERR|FAILED|FAILURE|EXCEPTION|TRACEBACK",
            "#E06060",
            false,
        ),
        recommended_rule(
            "builtin-warning",
            "Warning",
            "WARNING|WARN|DEPRECATED",
            "#E8C97A",
            false,
        ),
        recommended_rule(
            "builtin-success",
            "Success",
            "SUCCESS|SUCCEEDED|PASSED|COMPLETED",
            "#7EC699",
            false,
        ),
        recommended_rule("builtin-info", "Info", "INFO|NOTICE", "#6CB4EE", false),
        recommended_rule("builtin-debug", "Debug", "DEBUG|TRACE", "#828C9B", false),
        recommended_rule(
            "builtin-http-errors",
            "HTTP errors",
            "[45][0-9]{2}",
            "#E8A87C",
            false,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn recommended_rules_are_high_signal_and_have_stable_unique_ids() {
        let rules = default_highlight_rules();

        assert_eq!(rules.len(), 7);
        assert!(rules.iter().all(|rule| rule.enabled));
        assert!(rules.iter().all(|rule| rule.whole_word));
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
            ]
        );
        assert!(
            rules
                .iter()
                .all(|rule| rule.match_kind == HighlightMatchKind::Regex)
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
}
