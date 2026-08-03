use chrono::{DateTime, TimeZone, Utc};

pub fn format_mtime(ts: u32) -> String {
    let dt: DateTime<Utc> = Utc
        .timestamp_opt(ts as i64, 0)
        .single()
        .unwrap_or_else(Utc::now);
    dt.format("%Y-%m-%d %H:%M").to_string()
}

pub(crate) fn parent_dir(path: &str) -> Option<String> {
    if path == "/" || path.is_empty() {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    if let Some(idx) = trimmed.rfind('/') {
        if idx == 0 {
            Some("/".to_string())
        } else {
            Some(trimmed[..idx].to_string())
        }
    } else {
        Some("/".to_string())
    }
}

pub(crate) fn join_remote(parent: &str, child: &str) -> String {
    if parent == "/" {
        format!("/{child}")
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), child)
    }
}

pub(crate) fn base_name(path: &str) -> String {
    let sep = |c: char| c == '/' || c == '\\';
    path.trim_end_matches(sep)
        .rsplit(sep)
        .next()
        .unwrap_or(path)
        .to_string()
}

pub(crate) fn strip_archive_suffix(name: &str) -> &str {
    for suffix in [".tar.gz", ".tgz", ".zip", ".tar"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            return stripped;
        }
    }
    name
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub(crate) fn remote_parent(path: &str) -> String {
    if path == "/" {
        "/".to_string()
    } else {
        path.rsplit_once('/')
            .map(|(parent, _)| {
                if parent.is_empty() {
                    "/".to_string()
                } else {
                    parent.to_string()
                }
            })
            .unwrap_or_else(|| "/".to_string())
    }
}

pub(crate) fn resolve_remote_path(path: &str, home: &str) -> String {
    if path == "~" {
        home.to_string()
    } else if let Some(rest) = path.strip_prefix("~/") {
        join_remote(home, rest)
    } else {
        path.to_string()
    }
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
