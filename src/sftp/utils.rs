use chrono::{DateTime, TimeZone, Utc};

pub fn format_mtime(ts: u32) -> String {
    let dt: DateTime<Utc> = Utc
        .timestamp_opt(ts as i64, 0)
        .single()
        .unwrap_or_else(Utc::now);
    dt.format("%Y-%m-%d %H:%M").to_string()
}

pub fn format_permissions(mode: u32) -> String {
    let mut result = String::with_capacity(9);
    for (read, write, execute, special, special_exec, special_no_exec) in [
        (0o400, 0o200, 0o100, 0o4000, 's', 'S'),
        (0o040, 0o020, 0o010, 0o2000, 's', 'S'),
        (0o004, 0o002, 0o001, 0o1000, 't', 'T'),
    ] {
        result.push(if mode & read != 0 { 'r' } else { '-' });
        result.push(if mode & write != 0 { 'w' } else { '-' });
        result.push(if mode & special != 0 {
            if mode & execute != 0 {
                special_exec
            } else {
                special_no_exec
            }
        } else if mode & execute != 0 {
            'x'
        } else {
            '-'
        });
    }
    result
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

pub(crate) fn normalize_remote_directory_path(path: String) -> String {
    let normalized = path.trim_end_matches('/');
    if normalized.is_empty() {
        "/".to_string()
    } else if normalized.len() == path.len() {
        path
    } else {
        normalized.to_string()
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

#[cfg(test)]
mod tests {
    use super::{format_permissions, normalize_remote_directory_path};

    #[test]
    fn formats_posix_permissions_with_special_bits() {
        assert_eq!(format_permissions(0o100644), "rw-r--r--");
        assert_eq!(format_permissions(0o040755), "rwxr-xr-x");
        assert_eq!(format_permissions(0o104755), "rwsr-xr-x");
        assert_eq!(format_permissions(0o041777), "rwxrwxrwt");
        assert_eq!(format_permissions(0), "---------");
    }

    #[test]
    fn normalizes_remote_directory_trailing_slashes() {
        assert_eq!(
            normalize_remote_directory_path("/home/user/".to_string()),
            "/home/user"
        );
        assert_eq!(normalize_remote_directory_path("/".to_string()), "/");
        assert_eq!(normalize_remote_directory_path("///".to_string()), "/");
    }
}
