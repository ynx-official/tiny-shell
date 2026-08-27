use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt};

pub const BUILTIN_HIGHLIGHT_PACK_VERSION: u32 = 4;
pub const HIGHLIGHT_RULE_EXPORT_VERSION: u32 = 1;
pub const MAX_EXPORTED_HIGHLIGHT_RULES: usize = 128;
pub const MAX_EXPORTED_PATTERN_CHARACTERS: usize = 512;

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "kebab-case")]
pub enum HighlightRulePack {
    #[default]
    Core,
    Network,
    Web,
    Development,
    Git,
    Containers,
    Database,
    Security,
    Custom,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum HighlightRuleCategory {
    #[default]
    General,
    LogLevel,
    Protocol,
    Address,
    Identifier,
    Timestamp,
    Path,
    StructuredLog,
    StackTrace,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum HighlightEntityKind {
    Url,
    Email,
    FilePath,
    IpAddress,
    MacAddress,
    Uuid,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", content = "id", rename_all = "kebab-case")]
pub enum HighlightRuleScope {
    #[default]
    Global,
    Group(String),
    Session(String),
}

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
    pub capture_group: Option<usize>,
    #[serde(default)]
    pub pack: HighlightRulePack,
    #[serde(default)]
    pub category: HighlightRuleCategory,
    #[serde(default)]
    pub scope: HighlightRuleScope,
    #[serde(default)]
    pub entity: Option<HighlightEntityKind>,
    #[serde(default)]
    pub style: HighlightRuleStyle,
}

impl HighlightRule {
    pub fn custom(
        id: impl Into<String>,
        name: impl Into<String>,
        pattern: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            enabled: true,
            pattern: pattern.into(),
            match_kind: HighlightMatchKind::Literal,
            case_sensitive: false,
            whole_word: true,
            target: HighlightTarget::Match,
            capture_group: None,
            pack: HighlightRulePack::Custom,
            category: HighlightRuleCategory::General,
            scope: HighlightRuleScope::Global,
            entity: None,
            style: HighlightRuleStyle::default(),
        }
    }
}

impl HighlightRuleScope {
    pub fn specificity(&self) -> usize {
        match self {
            Self::Global => 0,
            Self::Group(group) => 1 + group.split('/').filter(|part| !part.is_empty()).count(),
            Self::Session(_) => usize::MAX,
        }
    }

    pub fn applies_to(&self, session_id: Option<&str>, group: Option<&str>) -> bool {
        match self {
            Self::Global => true,
            Self::Session(expected) => session_id == Some(expected.as_str()),
            Self::Group(expected) => group.is_some_and(|actual| {
                actual == expected
                    || actual
                        .strip_prefix(expected)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            }),
        }
    }
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
    let (pack, category, entity) = builtin_rule_metadata(id);
    HighlightRule {
        id: id.to_string(),
        name: name.to_string(),
        enabled: true,
        pattern: pattern.to_string(),
        match_kind: HighlightMatchKind::Regex,
        case_sensitive: false,
        whole_word,
        target: HighlightTarget::Match,
        capture_group: None,
        pack,
        category,
        scope: HighlightRuleScope::Global,
        entity,
        style,
    }
}

pub fn builtin_rule_metadata(
    id: &str,
) -> (
    HighlightRulePack,
    HighlightRuleCategory,
    Option<HighlightEntityKind>,
) {
    match id {
        "builtin-critical" | "builtin-error" | "builtin-warning" | "builtin-success"
        | "builtin-info" | "builtin-debug" => (
            HighlightRulePack::Core,
            HighlightRuleCategory::LogLevel,
            None,
        ),
        "builtin-http-errors" | "builtin-http-method" => (
            HighlightRulePack::Web,
            HighlightRuleCategory::Protocol,
            None,
        ),
        "builtin-url" => (
            HighlightRulePack::Web,
            HighlightRuleCategory::Identifier,
            Some(HighlightEntityKind::Url),
        ),
        "builtin-email" => (
            HighlightRulePack::Web,
            HighlightRuleCategory::Identifier,
            Some(HighlightEntityKind::Email),
        ),
        "builtin-ipv4" | "builtin-ipv6" => (
            HighlightRulePack::Network,
            HighlightRuleCategory::Address,
            Some(HighlightEntityKind::IpAddress),
        ),
        "builtin-mac" => (
            HighlightRulePack::Network,
            HighlightRuleCategory::Address,
            Some(HighlightEntityKind::MacAddress),
        ),
        "builtin-uuid" => (
            HighlightRulePack::Core,
            HighlightRuleCategory::Identifier,
            Some(HighlightEntityKind::Uuid),
        ),
        "builtin-timestamp" => (
            HighlightRulePack::Core,
            HighlightRuleCategory::Timestamp,
            None,
        ),
        "builtin-file-path" => (
            HighlightRulePack::Development,
            HighlightRuleCategory::Path,
            Some(HighlightEntityKind::FilePath),
        ),
        "builtin-structured-error" | "builtin-structured-warning" => (
            HighlightRulePack::Core,
            HighlightRuleCategory::StructuredLog,
            None,
        ),
        "builtin-source-location" | "builtin-stack-frame" => (
            HighlightRulePack::Development,
            HighlightRuleCategory::StackTrace,
            None,
        ),
        "builtin-git-conflict" | "builtin-git-ref" => {
            (HighlightRulePack::Git, HighlightRuleCategory::General, None)
        }
        "builtin-container-failure" | "builtin-container-lifecycle" => (
            HighlightRulePack::Containers,
            HighlightRuleCategory::General,
            None,
        ),
        "builtin-database-error" | "builtin-database-transaction" => (
            HighlightRulePack::Database,
            HighlightRuleCategory::General,
            None,
        ),
        "builtin-security-alert" | "builtin-security-advisory" => (
            HighlightRulePack::Security,
            HighlightRuleCategory::General,
            None,
        ),
        _ => (
            HighlightRulePack::Custom,
            HighlightRuleCategory::General,
            None,
        ),
    }
}

pub fn default_enabled_highlight_packs() -> Vec<HighlightRulePack> {
    vec![
        HighlightRulePack::Core,
        HighlightRulePack::Network,
        HighlightRulePack::Web,
        HighlightRulePack::Development,
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HighlightRuleBundle {
    pub format_version: u32,
    pub builtin_pack_version: u32,
    pub enabled_packs: Vec<HighlightRulePack>,
    pub rules: Vec<HighlightRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HighlightRuleBundleError {
    InvalidJson(String),
    UnsupportedVersion(u32),
    DuplicateRuleId(String),
    EmptyRuleId,
    TooManyRules { max: usize },
    PatternTooLong { rule_id: String, max: usize },
}

impl fmt::Display for HighlightRuleBundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(error) => write!(formatter, "invalid highlight rule bundle: {error}"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "unsupported highlight rule bundle version {version}"
                )
            }
            Self::DuplicateRuleId(id) => write!(formatter, "duplicate highlight rule id `{id}`"),
            Self::EmptyRuleId => formatter.write_str("highlight rule ids cannot be empty"),
            Self::TooManyRules { max } => {
                write!(
                    formatter,
                    "highlight rule bundle exceeds the {max}-rule limit"
                )
            }
            Self::PatternTooLong { rule_id, max } => write!(
                formatter,
                "highlight rule `{rule_id}` exceeds the {max}-character pattern limit"
            ),
        }
    }
}

impl HighlightRuleBundle {
    pub fn new(enabled_packs: Vec<HighlightRulePack>, rules: Vec<HighlightRule>) -> Self {
        Self {
            format_version: HIGHLIGHT_RULE_EXPORT_VERSION,
            builtin_pack_version: BUILTIN_HIGHLIGHT_PACK_VERSION,
            enabled_packs,
            rules,
        }
    }

    pub fn to_pretty_json(&self) -> Result<String, HighlightRuleBundleError> {
        serde_json::to_string_pretty(self)
            .map_err(|error| HighlightRuleBundleError::InvalidJson(error.to_string()))
    }

    pub fn from_json(json: &str) -> Result<Self, HighlightRuleBundleError> {
        let mut bundle: Self = serde_json::from_str(json)
            .map_err(|error| HighlightRuleBundleError::InvalidJson(error.to_string()))?;
        if bundle.format_version != HIGHLIGHT_RULE_EXPORT_VERSION {
            return Err(HighlightRuleBundleError::UnsupportedVersion(
                bundle.format_version,
            ));
        }
        if bundle.rules.len() > MAX_EXPORTED_HIGHLIGHT_RULES {
            return Err(HighlightRuleBundleError::TooManyRules {
                max: MAX_EXPORTED_HIGHLIGHT_RULES,
            });
        }
        let mut ids = HashSet::with_capacity(bundle.rules.len());
        for rule in &bundle.rules {
            if rule.id.trim().is_empty() {
                return Err(HighlightRuleBundleError::EmptyRuleId);
            }
            if !ids.insert(rule.id.as_str()) {
                return Err(HighlightRuleBundleError::DuplicateRuleId(rule.id.clone()));
            }
            if rule.pattern.chars().count() > MAX_EXPORTED_PATTERN_CHARACTERS {
                return Err(HighlightRuleBundleError::PatternTooLong {
                    rule_id: rule.id.clone(),
                    max: MAX_EXPORTED_PATTERN_CHARACTERS,
                });
            }
        }
        bundle
            .enabled_packs
            .retain(|pack| *pack != HighlightRulePack::Custom);
        bundle.enabled_packs.sort_unstable();
        bundle.enabled_packs.dedup();
        Ok(bundle)
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
    let mut rules = vec![
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
    ];
    let structured = [
        (
            "builtin-structured-error",
            "Structured error level",
            r#"(?:"(?:level|severity)"\s*:\s*"|(?:^|\s)(?:level|severity)=)(fatal|critical|error|err|failed)"#,
            recommended_style("#E85D68", None, true, false),
        ),
        (
            "builtin-structured-warning",
            "Structured warning level",
            r#"(?:"(?:level|severity)"\s*:\s*"|(?:^|\s)(?:level|severity)=)(warning|warn|deprecated)"#,
            recommended_style("#DFAF45", None, true, false),
        ),
    ];
    for (id, name, pattern, style) in structured {
        let mut rule = recommended_rule(id, name, pattern, true, style);
        rule.capture_group = Some(1);
        rules.insert(0, rule);
    }

    let mut source_location = recommended_rule(
        "builtin-source-location",
        "Source location",
        r#"((?:[A-Z]:\\|/|\./|\.\./|~/)[^\s:()]+):[0-9]+(?::[0-9]+)?"#,
        false,
        recommended_style("#4EA1F3", None, false, false),
    );
    source_location.capture_group = Some(1);
    rules.push(source_location);

    let mut stack_frame = recommended_rule(
        "builtin-stack-frame",
        "Stack frame",
        r#"^\s*(?:at\s+.+|File\s+"[^"]+",\s+line\s+[0-9]+.*|Caused by:.+|goroutine\s+[0-9]+.+)$"#,
        false,
        recommended_style("#7C8494", None, false, false),
    );
    stack_frame.target = HighlightTarget::Line;
    rules.push(stack_frame);

    let specialized = [
        (
            "builtin-git-conflict",
            "Git conflict marker",
            HighlightRulePack::Git,
            r#"^(?:<{7}|={7}|>{7}).*$"#,
            HighlightTarget::Line,
            recommended_style("#E85D68", None, true, false),
        ),
        (
            "builtin-git-ref",
            "Git reference",
            HighlightRulePack::Git,
            r#"(?:commit|branch|HEAD(?: detached at)?)\s+(?:[0-9A-F]{7,40}|[A-Z0-9._/-]+)"#,
            HighlightTarget::Match,
            recommended_style("#4EA1F3", None, false, false),
        ),
        (
            "builtin-container-failure",
            "Container failure",
            HighlightRulePack::Containers,
            r#"CrashLoopBackOff|ImagePullBackOff|ErrImagePull|OOMKilled|Evicted|Unhealthy|Exited\s+\([1-9][0-9]*\)"#,
            HighlightTarget::Match,
            recommended_style("#E85D68", None, true, false),
        ),
        (
            "builtin-container-lifecycle",
            "Container lifecycle",
            HighlightRulePack::Containers,
            r#"Pulling|Pulled|Created|Started|Terminating|ContainerCreating"#,
            HighlightTarget::Match,
            recommended_style("#52B788", None, false, false),
        ),
        (
            "builtin-database-error",
            "Database error",
            HighlightRulePack::Database,
            r#"SQLSTATE\[[A-Z0-9]+\]|ORA-[0-9]{5}|DEADLOCK DETECTED|DUPLICATE KEY|CONSTRAINT VIOLATION"#,
            HighlightTarget::Match,
            recommended_style("#E85D68", None, true, false),
        ),
        (
            "builtin-database-transaction",
            "Database transaction",
            HighlightRulePack::Database,
            r#"BEGIN TRANSACTION|COMMIT|ROLLBACK|SAVEPOINT"#,
            HighlightTarget::Match,
            recommended_style("#4EA1F3", None, false, false),
        ),
        (
            "builtin-security-alert",
            "Security alert",
            HighlightRulePack::Security,
            r#"AUTHENTICATION FAILED|ACCESS DENIED|CERTIFICATE (?:EXPIRED|INVALID)|SIGNATURE INVALID|HOST KEY VERIFICATION FAILED"#,
            HighlightTarget::Match,
            recommended_style("#E85D68", None, true, false),
        ),
        (
            "builtin-security-advisory",
            "Security advisory",
            HighlightRulePack::Security,
            r#"EXPOSED SECRET|LEAKED CREDENTIAL|VULNERABILITY|CVE-[0-9]{4}-[0-9]{4,}"#,
            HighlightTarget::Match,
            recommended_style("#DFAF45", None, true, false),
        ),
    ];
    for (id, name, pack, pattern, target, style) in specialized {
        let mut rule = recommended_rule(id, name, pattern, true, style);
        rule.pack = pack;
        rule.target = target;
        rules.push(rule);
    }
    rules
}

fn legacy_default_highlight_rules_v3() -> Vec<HighlightRule> {
    let ids = [
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
    ];
    default_highlight_rules()
        .into_iter()
        .filter(|rule| ids.contains(&rule.id.as_str()))
        .collect()
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

fn same_rule_behavior(left: &[HighlightRule], right: &[HighlightRule]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.id == right.id
                && left.name == right.name
                && left.enabled == right.enabled
                && left.pattern == right.pattern
                && left.match_kind == right.match_kind
                && left.case_sensitive == right.case_sensitive
                && left.whole_word == right.whole_word
                && left.target == right.target
                && left.style == right.style
        })
}

/// Upgrades the built-in catalog while preserving user-authored behavior.
/// Metadata added by newer versions is inferred only for known built-in IDs.
pub fn upgrade_builtin_highlight_rules(rules: &mut Vec<HighlightRule>, version: &mut u32) {
    if *version >= BUILTIN_HIGHLIGHT_PACK_VERSION {
        return;
    }

    if same_rule_behavior(rules, &legacy_default_highlight_rules_v1())
        || same_rule_behavior(rules, &legacy_default_highlight_rules_v2())
        || same_rule_behavior(rules, &legacy_default_highlight_rules_v3())
    {
        *rules = default_highlight_rules();
    } else {
        for rule in rules.iter_mut() {
            if rule.id.starts_with("builtin-") {
                let (pack, category, entity) = builtin_rule_metadata(&rule.id);
                rule.pack = pack;
                rule.category = category;
                rule.entity = entity;
            } else if rule.pack == HighlightRulePack::Core
                && rule.category == HighlightRuleCategory::General
                && rule.entity.is_none()
            {
                rule.pack = HighlightRulePack::Custom;
            }
        }
    }
    *version = BUILTIN_HIGHLIGHT_PACK_VERSION;
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn recommended_rules_are_high_signal_and_have_stable_unique_ids() {
        let rules = default_highlight_rules();

        assert_eq!(rules.len(), 28);
        assert!(rules.iter().all(|rule| rule.enabled));
        assert!(
            rules
                .iter()
                .filter(|rule| {
                    !matches!(
                        rule.id.as_str(),
                        "builtin-url" | "builtin-source-location" | "builtin-stack-frame"
                    )
                })
                .all(|rule| rule.whole_word)
        );
        assert_eq!(
            rules
                .iter()
                .filter(|rule| rule.target == HighlightTarget::Line)
                .map(|rule| rule.id.as_str())
                .collect::<Vec<_>>(),
            ["builtin-stack-frame", "builtin-git-conflict"]
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
                "builtin-structured-warning",
                "builtin-structured-error",
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
                "builtin-source-location",
                "builtin-stack-frame",
                "builtin-git-conflict",
                "builtin-git-ref",
                "builtin-container-failure",
                "builtin-container-lifecycle",
                "builtin-database-error",
                "builtin-database-transaction",
                "builtin-security-alert",
                "builtin-security-advisory",
            ]
        );
        assert!(
            rules
                .iter()
                .all(|rule| rule.match_kind == HighlightMatchKind::Regex)
        );
        assert!(
            rules
                .iter()
                .filter(|rule| {
                    matches!(
                        rule.id.as_str(),
                        "builtin-critical" | "builtin-error" | "builtin-warning"
                    )
                })
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
            capture_group: Some(1),
            pack: HighlightRulePack::Custom,
            category: HighlightRuleCategory::StructuredLog,
            scope: HighlightRuleScope::Group("production/api".to_string()),
            entity: None,
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
        assert_eq!(json["capture_group"], 1);
        assert_eq!(json["scope"]["kind"], "group");
        assert_eq!(json["style"]["background"], "#445566");
        assert_eq!(serde_json::from_value::<HighlightRule>(json).unwrap(), rule);
    }

    #[test]
    fn legacy_rule_json_defaults_new_metadata_without_changing_behavior() {
        let rule: HighlightRule =
            serde_json::from_str(r#"{"id":"legacy","name":"Legacy","pattern":"ERROR"}"#).unwrap();
        assert!(rule.enabled);
        assert_eq!(rule.capture_group, None);
        assert_eq!(rule.pack, HighlightRulePack::Core);
        assert_eq!(rule.category, HighlightRuleCategory::General);
        assert_eq!(rule.scope, HighlightRuleScope::Global);
        assert_eq!(rule.entity, None);
    }

    #[test]
    fn untouched_original_defaults_upgrade_but_user_changes_are_preserved() {
        let mut untouched = legacy_default_highlight_rules_v1();
        let mut version = 0;
        upgrade_builtin_highlight_rules(&mut untouched, &mut version);
        assert_eq!(untouched, default_highlight_rules());
        assert_eq!(version, BUILTIN_HIGHLIGHT_PACK_VERSION);

        let mut previous_defaults = legacy_default_highlight_rules_v2();
        let mut version = 0;
        upgrade_builtin_highlight_rules(&mut previous_defaults, &mut version);
        assert_eq!(previous_defaults, default_highlight_rules());

        let mut previous_defaults = legacy_default_highlight_rules_v3();
        let mut version = 3;
        upgrade_builtin_highlight_rules(&mut previous_defaults, &mut version);
        assert_eq!(previous_defaults, default_highlight_rules());

        let mut customized_previous_defaults = legacy_default_highlight_rules_v2();
        customized_previous_defaults[1].pattern.push_str("|CUSTOM");
        let expected = customized_previous_defaults.clone();
        let mut version = 0;
        upgrade_builtin_highlight_rules(&mut customized_previous_defaults, &mut version);
        assert!(same_rule_behavior(&customized_previous_defaults, &expected));

        let mut customized = legacy_default_highlight_rules_v1();
        customized[1].style.foreground = Some("#123456".to_string());
        let expected = customized.clone();
        let mut version = 0;
        upgrade_builtin_highlight_rules(&mut customized, &mut version);
        assert!(same_rule_behavior(&customized, &expected));

        let mut empty = Vec::new();
        let mut version = 0;
        upgrade_builtin_highlight_rules(&mut empty, &mut version);
        assert!(empty.is_empty());
    }

    #[test]
    fn scopes_match_global_group_descendants_and_exact_sessions() {
        assert!(HighlightRuleScope::Global.applies_to(None, None));
        assert!(HighlightRuleScope::Group("prod".into()).applies_to(None, Some("prod/api")));
        assert!(!HighlightRuleScope::Group("prod".into()).applies_to(None, Some("production")));
        assert!(HighlightRuleScope::Session("s-1".into()).applies_to(Some("s-1"), None));
        assert!(!HighlightRuleScope::Session("s-1".into()).applies_to(Some("s-2"), None));
    }

    #[test]
    fn rule_bundle_round_trips_and_rejects_duplicate_ids() {
        let bundle =
            HighlightRuleBundle::new(default_enabled_highlight_packs(), default_highlight_rules());
        let json = bundle.to_pretty_json().unwrap();
        assert_eq!(HighlightRuleBundle::from_json(&json).unwrap(), bundle);

        let mut duplicated = bundle;
        duplicated.rules.push(duplicated.rules[0].clone());
        let json = duplicated.to_pretty_json().unwrap();
        assert!(matches!(
            HighlightRuleBundle::from_json(&json),
            Err(HighlightRuleBundleError::DuplicateRuleId(_))
        ));

        let oversized = HighlightRuleBundle::new(
            Vec::new(),
            (0..=MAX_EXPORTED_HIGHLIGHT_RULES)
                .map(|index| HighlightRule::custom(format!("rule-{index}"), "Rule", "value"))
                .collect(),
        );
        assert!(matches!(
            HighlightRuleBundle::from_json(&oversized.to_pretty_json().unwrap()),
            Err(HighlightRuleBundleError::TooManyRules { max: 128 })
        ));
    }
}
