use anyhow::{Result, bail};
use uuid::Uuid;

use super::config::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionSortKey {
    Name,
    Host,
    User,
    LastUsed,
}

pub(crate) fn is_group_or_descendant(group: &str, ancestor: &str) -> bool {
    group == ancestor || group.starts_with(&format!("{ancestor}/"))
}

pub(crate) fn group_matches_query(group: &str, sessions: &[Session], query: &str) -> bool {
    if query.is_empty() || group.to_lowercase().contains(query) {
        return true;
    }
    sessions.iter().any(|session| {
        session.group.as_deref().is_some_and(|session_group| {
            is_group_or_descendant(session_group, group)
                && (session.name.to_lowercase().contains(query)
                    || session.host.to_lowercase().contains(query)
                    || session.user.to_lowercase().contains(query))
        })
    })
}

pub(crate) fn sort_sessions(sessions: &mut [Session], key: ConnectionSortKey, descending: bool) {
    sessions.sort_by(|left, right| {
        let ordering = match key {
            ConnectionSortKey::Name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
            ConnectionSortKey::Host => left.host.to_lowercase().cmp(&right.host.to_lowercase()),
            ConnectionSortKey::User => left.user.to_lowercase().cmp(&right.user.to_lowercase()),
            ConnectionSortKey::LastUsed => left.last_used.cmp(&right.last_used),
        }
        .then_with(|| left.id.cmp(&right.id));
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

pub(crate) fn copy_session(
    config: &mut crate::session::config::ConfigStore,
    id: &str,
    destination_group: Option<&str>,
) -> Result<String> {
    let Some(source) = config.get(id).cloned() else {
        bail!("connection not found");
    };
    let mut copied = source;
    copied.id = Uuid::new_v4().to_string();
    copied.group = destination_group.map(str::to_string);
    copied.name = unique_session_name(config.sessions(), copied.group.as_deref(), &copied.name);
    config.upsert(copied.clone());
    Ok(copied.id)
}

pub(crate) fn move_session(
    config: &mut crate::session::config::ConfigStore,
    id: &str,
    destination_group: Option<&str>,
) -> Result<()> {
    let Some(mut session) = config.get(id).cloned() else {
        bail!("connection not found");
    };
    session.group = destination_group.map(str::to_string);
    config.upsert(session);
    Ok(())
}

pub(crate) fn move_connection_group(
    config: &mut crate::session::config::ConfigStore,
    source_group: &str,
    destination_parent: Option<&str>,
) -> Result<String> {
    if !config
        .connection_groups()
        .iter()
        .any(|group| group == source_group)
    {
        bail!("connection group not found");
    }
    if destination_parent.is_some_and(|parent| is_group_or_descendant(parent, source_group)) {
        bail!("connection group cannot be moved into itself");
    }
    if destination_parent.is_some_and(|parent| {
        !config
            .connection_groups()
            .iter()
            .any(|group| group == parent)
    }) {
        bail!("destination connection group not found");
    }

    let leaf = source_group.rsplit('/').next().unwrap_or(source_group);
    let requested = destination_parent
        .map(|parent| format!("{parent}/{leaf}"))
        .unwrap_or_else(|| leaf.to_string());
    if requested == source_group {
        return Ok(source_group.to_string());
    }
    let destination = unique_group_name(config.connection_groups(), &requested);
    config.rename_connection_group(source_group, destination.clone());
    Ok(destination)
}

pub(crate) fn copy_connection_group(
    config: &mut crate::session::config::ConfigStore,
    source_group: &str,
    destination_parent: Option<&str>,
) -> Result<String> {
    if !config
        .connection_groups()
        .iter()
        .any(|group| group == source_group)
    {
        bail!("connection group not found");
    }
    if destination_parent.is_some_and(|parent| {
        !config
            .connection_groups()
            .iter()
            .any(|group| group == parent)
    }) {
        bail!("destination connection group not found");
    }
    let leaf = source_group.rsplit('/').next().unwrap_or(source_group);
    let requested = destination_parent
        .map(|parent| format!("{parent}/{leaf}"))
        .unwrap_or_else(|| leaf.to_string());
    let destination = unique_group_name(config.connection_groups(), &requested);
    let old_prefix = format!("{source_group}/");
    let new_prefix = format!("{destination}/");
    let groups = config
        .connection_groups()
        .iter()
        .filter(|group| *group == source_group || group.starts_with(&old_prefix))
        .cloned()
        .collect::<Vec<_>>();
    for group in groups {
        let copied_group = if group == source_group {
            destination.clone()
        } else {
            format!("{new_prefix}{}", &group[old_prefix.len()..])
        };
        config.add_connection_group(copied_group);
    }
    let sessions = config
        .sessions()
        .iter()
        .filter(|session| {
            session
                .group
                .as_deref()
                .is_some_and(|group| group == source_group || group.starts_with(&old_prefix))
        })
        .cloned()
        .collect::<Vec<_>>();
    for mut session in sessions {
        session.id = Uuid::new_v4().to_string();
        session.group = session.group.map(|group| {
            if group == source_group {
                destination.clone()
            } else {
                format!("{new_prefix}{}", &group[old_prefix.len()..])
            }
        });
        session.name =
            unique_session_name(config.sessions(), session.group.as_deref(), &session.name);
        config.upsert(session);
    }
    Ok(destination)
}

pub(crate) fn session_address(session: &Session) -> String {
    format!("ssh://{}@{}:{}", session.user, session.host, session.port)
}

pub(crate) fn parse_session_address(address: &str) -> Result<Session> {
    let value = address.strip_prefix("ssh://").unwrap_or(address);
    let Some((user, host_port)) = value.split_once('@') else {
        bail!("SSH address must contain a user");
    };
    if user.is_empty() {
        bail!("SSH address user cannot be empty");
    }
    let Some((host, port)) = host_port.rsplit_once(':') else {
        bail!("SSH address must contain a port");
    };
    let port = port
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("invalid SSH port"))?;
    if host.is_empty() || port == 0 {
        bail!("SSH address host or port is invalid");
    }
    Ok(Session::password(
        host.to_string(),
        port,
        user.to_string(),
        String::new(),
    ))
}

fn unique_group_name(existing: &[String], requested: &str) -> String {
    unique_name(existing.iter().map(String::as_str), requested)
}

fn unique_session_name(sessions: &[Session], group: Option<&str>, requested: &str) -> String {
    unique_name(
        sessions
            .iter()
            .filter(|session| session.group.as_deref() == group)
            .map(|session| session.name.as_str()),
        requested,
    )
}

fn unique_name<'a>(existing: impl Iterator<Item = &'a str>, requested: &str) -> String {
    let existing = existing.collect::<std::collections::HashSet<_>>();
    if !existing.contains(requested) {
        return requested.to_string();
    }
    (2_u32..)
        .map(|suffix| format!("{requested} ({suffix})"))
        .find(|candidate| !existing.contains(candidate.as_str()))
        .unwrap_or_else(|| format!("{requested} ({})", Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(name: &str, host: &str, user: &str, group: Option<&str>) -> Session {
        let mut value = Session::password(host.to_string(), 22, user.to_string(), String::new());
        value.id = name.to_string();
        value.name = name.to_string();
        value.group = group.map(str::to_string);
        value
    }

    #[test]
    fn copy_and_move_keep_identity_rules_and_avoid_name_collisions() {
        let mut config = crate::session::config::ConfigStore::in_memory();
        config.add_connection_group("prod".to_string());
        let source = session("database", "10.0.0.2", "root", Some("prod"));
        let source_id = source.id.clone();
        config.upsert(source);

        let copied_id = copy_session(&mut config, &source_id, Some("prod")).unwrap();
        assert_ne!(copied_id, source_id);
        assert!(
            config
                .sessions()
                .iter()
                .any(|item| item.name == "database (2)")
        );
        move_session(&mut config, &copied_id, None).unwrap();
        assert!(
            config
                .get(&copied_id)
                .is_some_and(|item| item.group.is_none())
        );
    }

    #[test]
    fn copied_group_rebuilds_tree_and_session_ids() {
        let mut config = crate::session::config::ConfigStore::in_memory();
        config.add_connection_group("prod".to_string());
        config.add_connection_group("prod/eu".to_string());
        let source = session("database", "10.0.0.2", "root", Some("prod/eu"));
        let source_id = source.id.clone();
        config.upsert(source);

        let copied_root = copy_connection_group(&mut config, "prod", None).unwrap();
        assert_eq!(copied_root, "prod (2)");
        assert!(
            config
                .connection_groups()
                .iter()
                .any(|group| group == "prod (2)/eu")
        );
        assert!(
            config.sessions().iter().any(|item| {
                item.id != source_id && item.group.as_deref() == Some("prod (2)/eu")
            })
        );
    }

    #[test]
    fn copied_and_moved_groups_respect_destination_parent() {
        let mut config = crate::session::config::ConfigStore::in_memory();
        for group in ["prod", "prod/eu", "archive"] {
            config.add_connection_group(group.to_string());
        }
        config.upsert(session("database", "10.0.0.2", "root", Some("prod/eu")));

        let copied = copy_connection_group(&mut config, "prod", Some("archive")).unwrap();
        assert_eq!(copied, "archive/prod");
        assert!(
            config
                .connection_groups()
                .iter()
                .any(|group| group == "archive/prod/eu")
        );
        assert!(
            config
                .sessions()
                .iter()
                .any(|item| { item.group.as_deref() == Some("archive/prod/eu") })
        );

        let moved = move_connection_group(&mut config, "prod", Some("archive")).unwrap();
        assert_eq!(moved, "archive/prod (2)");
        assert!(
            !config
                .connection_groups()
                .iter()
                .any(|group| group == "prod")
        );
        assert!(
            config
                .sessions()
                .iter()
                .any(|item| { item.group.as_deref() == Some("archive/prod (2)/eu") })
        );
    }

    #[test]
    fn moving_group_rejects_descendants_and_missing_destinations() {
        let mut config = crate::session::config::ConfigStore::in_memory();
        config.add_connection_group("prod".to_string());
        config.add_connection_group("prod/eu".to_string());

        assert!(move_connection_group(&mut config, "prod", Some("prod/eu")).is_err());
        assert!(move_connection_group(&mut config, "prod", Some("missing")).is_err());
        assert!(
            config
                .connection_groups()
                .iter()
                .any(|group| group == "prod")
        );
    }

    #[test]
    fn ssh_address_round_trip_is_explicit_and_strict() {
        let source = session("database", "example.com", "alice", None);
        let restored = parse_session_address(&session_address(&source)).unwrap();
        assert_eq!(restored.host, source.host);
        assert_eq!(restored.port, source.port);
        assert_eq!(restored.user, source.user);
        assert!(parse_session_address("ssh://example.com:22").is_err());
    }

    #[test]
    fn descendant_groups_match_their_ancestor() {
        assert!(is_group_or_descendant("prod/eu", "prod"));
        assert!(!is_group_or_descendant("production", "prod"));
    }

    #[test]
    fn group_query_keeps_groups_with_matching_sessions() {
        let sessions = vec![session("database", "10.0.0.2", "root", Some("prod"))];
        assert!(group_matches_query("prod", &sessions, "database"));
        assert!(!group_matches_query("dev", &sessions, "database"));
    }

    #[test]
    fn soft_deleted_session_can_be_restored_with_its_group() {
        let mut config = crate::session::config::ConfigStore::in_memory();
        config.add_connection_group("prod/eu".to_string());
        let session = session("database", "10.0.0.2", "root", Some("prod/eu"));
        let id = session.id.clone();
        config.upsert(session);

        assert!(config.soft_delete_session(&id));
        assert!(config.get(&id).is_none());
        assert_eq!(config.deleted_sessions().len(), 1);

        assert!(config.restore_deleted_session(&id));
        assert!(config.get(&id).is_some());
        assert_eq!(config.deleted_sessions().len(), 0);
    }

    #[test]
    fn soft_deleted_group_restores_descendants_and_sessions() {
        let mut config = crate::session::config::ConfigStore::in_memory();
        config.add_connection_group("prod".to_string());
        config.add_connection_group("prod/eu".to_string());
        let session = session("database", "10.0.0.2", "root", Some("prod/eu"));
        let id = session.id.clone();
        config.upsert(session);

        assert!(config.soft_delete_connection_group("prod"));
        assert!(config.connection_groups().is_empty());
        assert!(config.get(&id).is_none());

        assert!(config.restore_deleted_connection_group("prod"));
        assert!(
            config
                .connection_groups()
                .iter()
                .any(|group| group == "prod/eu")
        );
        assert_eq!(
            config.get(&id).and_then(|item| item.group.as_deref()),
            Some("prod/eu")
        );
    }
}
