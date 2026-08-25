pub(super) fn parse_path_from_title(title: &str, home_dir: &str) -> Option<String> {
    const CWD_MARKERS: [&str; 2] = ["TINY_SHELL_CWD:", "ASHELL_CWD:"];
    let title = title.trim();
    let path_part = CWD_MARKERS
        .iter()
        .find_map(|marker| title.rsplit_once(marker).map(|(_, path)| path.trim()))
        .or_else(|| {
            title
                .match_indices(':')
                .rev()
                .map(|(index, _)| title[index + 1..].trim())
                .find(|candidate| looks_like_terminal_path(candidate))
        })
        .or_else(|| looks_like_terminal_path(title).then_some(title))?;

    let path_part = [" — ", " | "]
        .into_iter()
        .filter_map(|separator| path_part.split_once(separator).map(|(path, _)| path.trim()))
        .next()
        .unwrap_or(path_part);

    if path_part.starts_with('/') {
        Some(path_part.to_string())
    } else if path_part == "~" {
        Some(home_dir.to_string())
    } else if let Some(rest) = path_part.strip_prefix("~/") {
        let home = home_dir.trim_end_matches('/');
        Some(format!("{home}/{rest}"))
    } else {
        None
    }
}

fn looks_like_terminal_path(candidate: &str) -> bool {
    candidate.starts_with('/') || candidate == "~" || candidate.starts_with("~/")
}

#[cfg(test)]
mod tests {
    use super::parse_path_from_title;

    #[test]
    fn parses_absolute_terminal_path() {
        assert_eq!(
            parse_path_from_title("user@host:/srv/app", "/home/user"),
            Some("/srv/app".to_string())
        );
    }

    #[test]
    fn expands_home_terminal_path() {
        assert_eq!(
            parse_path_from_title("TINY_SHELL_CWD:~/projects", "/home/user"),
            Some("/home/user/projects".to_string())
        );
    }

    #[test]
    fn parses_path_after_decorated_title_prefix() {
        assert_eq!(
            parse_path_from_title("ssh: user@host:/srv/app", "/home/user"),
            Some("/srv/app".to_string())
        );
    }

    #[test]
    fn strips_common_title_suffixes() {
        assert_eq!(
            parse_path_from_title("user@host:/srv/app — zsh", "/home/user"),
            Some("/srv/app".to_string())
        );
        assert_eq!(
            parse_path_from_title("~/projects | main", "/home/user"),
            Some("/home/user/projects".to_string())
        );
    }

    #[test]
    fn accepts_legacy_explicit_cwd_marker() {
        assert_eq!(
            parse_path_from_title("ASHELL_CWD:~/legacy", "/home/user"),
            Some("/home/user/legacy".to_string())
        );
    }

    #[test]
    fn rejects_titles_without_a_remote_path() {
        assert_eq!(parse_path_from_title("user@host", "/home/user"), None);
    }
}
