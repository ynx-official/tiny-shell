use regex::Regex;
use serde_json::{Value, json};
use std::sync::OnceLock;

const MAX_STRING: usize = 1024 * 1024;
const MAX_NODES: usize = 4096;
fn patterns() -> &'static Vec<Regex> {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| [
        r"(?s)-----BEGIN[^\r\n]*PRIVATE KEY-----.*?-----END[^\r\n]*PRIVATE KEY-----",
        r"(?i)\b(?:Bearer|Basic)\s+[A-Za-z0-9+/_.=\-]+",
        r"\bsk-[A-Za-z0-9_\-]{12,}",
        r#"(?i)(?:password|passwd|api[_-]?key|secret|access[_-]?token|private[_-]?key)\s*[:=]\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s,;]+)"#,
        r"(?i)https?://[^/\s:@]+:[^/\s@]+@",
        r"(?i)[?&](?:token|key|api_key|secret|password)=[^&\s]+",
    ].into_iter().filter_map(|p| Regex::new(p).ok()).collect())
}
fn secret(key: &str) -> bool {
    let key = key.to_lowercase().replace(['-', '_'], "");
    matches!(
        key.as_str(),
        "password"
            | "passwd"
            | "apikey"
            | "authorization"
            | "secret"
            | "proof"
            | "privatekey"
            | "passphrase"
            | "accesstoken"
            | "refreshtoken"
            | "continuationtoken"
            | "token"
    )
}
pub fn text(value: &str, custom: &[Regex]) -> String {
    let mut value = value.to_owned();
    for pattern in patterns().iter().chain(custom) {
        value = pattern.replace_all(&value, "[REDACTED]").into_owned();
    }
    value
}
pub fn value(input: &Value, custom: &[Regex]) -> Value {
    let mut nodes = 0;
    fn visit(input: &Value, custom: &[Regex], nodes: &mut usize, depth: usize) -> Value {
        *nodes += 1;
        if *nodes > MAX_NODES || depth > 32 {
            return json!({"audit_truncated":true});
        }
        match input {
            Value::Object(map) => Value::Object(
                map.iter()
                    .take(MAX_NODES)
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            if secret(k) {
                                Value::String("[REDACTED]".into())
                            } else {
                                visit(v, custom, nodes, depth + 1)
                            },
                        )
                    })
                    .collect(),
            ),
            Value::Array(array) => Value::Array(
                array
                    .iter()
                    .take(MAX_NODES)
                    .map(|v| visit(v, custom, nodes, depth + 1))
                    .collect(),
            ),
            Value::String(s) => {
                let redacted = text(s, custom);
                if redacted.len() > MAX_STRING {
                    let mut end = MAX_STRING;
                    while !redacted.is_char_boundary(end) {
                        end -= 1;
                    }
                    json!({"text":&redacted[..end],"audit_truncated":true,"original_bytes":s.len()})
                } else {
                    Value::String(redacted)
                }
            }
            other => other.clone(),
        }
    }
    visit(input, custom, &mut nodes, 0)
}

/// Known credential files retain the path and outcome, but never their body or a diff.
pub(super) fn event(event: &mut super::Event, custom: &[Regex]) {
    let path = event.details["arguments"]["path"]
        .as_str()
        .or_else(|| event.details["call"]["arguments"]["path"].as_str())
        .or_else(|| event.details["call"]["path"].as_str());
    if path.is_some_and(sensitive_path) {
        fn hide(value: &mut Value) {
            match value {
                Value::Object(map) => {
                    for (key, item) in map.iter_mut() {
                        if matches!(
                            key.as_str(),
                            "content"
                                | "text"
                                | "stdout"
                                | "stderr"
                                | "diff"
                                | "before"
                                | "after"
                                | "old_content"
                                | "new_content"
                                | "bytes"
                        ) {
                            *item = Value::String("[REDACTED: credential file body]".into());
                        } else {
                            hide(item);
                        }
                    }
                }
                Value::Array(items) => items.iter_mut().for_each(hide),
                _ => {}
            }
        }
        hide(&mut event.details);
        if let Some(map) = event.details.as_object_mut() {
            map.insert("sensitive_file_body_omitted".into(), Value::Bool(true));
        }
    }
    event.details = value(&event.details, custom);
    event.tool = text(&event.tool, &[]);
    for field in [
        &mut event.context.client_name,
        &mut event.context.target_name,
        &mut event.context.purpose,
    ] {
        if let Some(value) = field.as_mut() {
            *value = text(value, custom);
        }
    }
    if let Some(value) = event.context.tool_call_id.as_mut() {
        *value = text(value, &[]);
    }
}
fn sensitive_path(path: &str) -> bool {
    let normalized = path.to_lowercase().replace('\\', "/");
    let basename = normalized.rsplit('/').next().unwrap_or_default();
    matches!(
        basename,
        "id_rsa"
            | "id_ed25519"
            | "id_ecdsa"
            | "id_dsa"
            | "shadow"
            | "gshadow"
            | ".netrc"
            | ".npmrc"
            | ".env"
    ) || basename.starts_with(".env.")
        || normalized.ends_with("/.aws/credentials")
        || normalized.ends_with("/.kube/config")
        || basename.ends_with(".key")
        || basename.ends_with(".pem")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_file_bodies_are_omitted_even_when_they_contain_no_named_secret() {
        let mut record = super::super::Event::new(
            super::super::Context::new(super::super::Source::Ai),
            "execution_result",
            "read_file",
            "succeeded",
            json!({"call":{"arguments":{"path":"/home/test/.aws/credentials"}},"extra":{"result":{"content":"business-credential-without-a-prefix"}},"preview":{"old_content":"original-credential"}}),
        );
        event(&mut record, &[]);
        let output = record.details.to_string();
        assert!(!output.contains("business-credential"));
        assert!(!output.contains("original-credential"));
        assert!(output.contains(".aws/credentials"));
    }
    #[test]
    fn secrets_are_removed_from_both_structured_and_embedded_output() {
        let redacted = value(&json!({"API_Key":"hidden-key","proof":"hidden-proof","command":"PASSWORD=hidden-pass curl -H 'Authorization: Bearer hidden-access-token' https://u:hidden-url-pass@example.test","stdout":"-----BEGIN PRIVATE KEY-----\nhidden-private\n-----END PRIVATE KEY-----"}),&[]).to_string();
        for s in [
            "hidden-key",
            "hidden-proof",
            "hidden-pass",
            "hidden-access",
            "hidden-url-pass",
            "hidden-private",
        ] {
            assert!(!redacted.contains(s));
        }
        assert!(redacted.contains("REDACTED"));
    }
}
