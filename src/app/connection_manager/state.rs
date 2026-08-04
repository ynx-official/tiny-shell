use std::collections::HashSet;

use crate::session::{
    config::{ConfigStore, DeletedConnectionGroup, DeletedSession, Session},
    connection_catalog::{self, ConnectionSortKey},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ConnectionNodeId {
    Group(String),
    Session(String),
    DeletedGroup(String),
    DeletedSession(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionSort {
    Name,
    Host,
    User,
    LastUsed,
}

impl From<ConnectionSort> for ConnectionSortKey {
    fn from(value: ConnectionSort) -> Self {
        match value {
            ConnectionSort::Name => Self::Name,
            ConnectionSort::Host => Self::Host,
            ConnectionSort::User => Self::User,
            ConnectionSort::LastUsed => Self::LastUsed,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ConnectionTreeNode {
    Group {
        id: ConnectionNodeId,
        name: String,
        depth: usize,
        expanded: bool,
    },
    Session {
        id: ConnectionNodeId,
        session_id: String,
        depth: usize,
    },
    DeletedGroup {
        id: ConnectionNodeId,
        name: String,
        depth: usize,
    },
    DeletedSession {
        id: ConnectionNodeId,
        session: Box<Session>,
        depth: usize,
    },
}

impl ConnectionTreeNode {
    pub(crate) fn depth(&self) -> usize {
        match self {
            Self::Group { depth, .. }
            | Self::Session { depth, .. }
            | Self::DeletedGroup { depth, .. }
            | Self::DeletedSession { depth, .. } => *depth,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ConnectionManagerState {
    pub query: String,
    pub show_deleted: bool,
    pub sort: ConnectionSort,
    pub descending: bool,
    pub selected: Option<ConnectionNodeId>,
    pub expanded: HashSet<String>,
    previous_expanded: Option<HashSet<String>>,
}

impl Default for ConnectionManagerState {
    fn default() -> Self {
        Self {
            query: String::new(),
            show_deleted: false,
            sort: ConnectionSort::Name,
            descending: false,
            selected: None,
            expanded: HashSet::new(),
            previous_expanded: None,
        }
    }
}

impl ConnectionManagerState {
    pub fn set_query(&mut self, query: String) {
        let was_empty = self.query.is_empty();
        let is_empty = query.is_empty();
        if was_empty != is_empty {
            if is_empty {
                if let Some(previous) = self.previous_expanded.take() {
                    self.expanded = previous;
                }
            } else {
                self.previous_expanded = Some(self.expanded.clone());
            }
        }
        self.query = query.trim().to_lowercase();
    }

    pub fn toggle_deleted(&mut self) {
        self.show_deleted = !self.show_deleted;
    }

    pub fn toggle_group(&mut self, group: &str) {
        if !self.expanded.remove(group) {
            self.expanded.insert(group.to_string());
        }
    }

    pub fn select(&mut self, node: ConnectionNodeId) {
        self.selected = Some(node);
    }

    pub fn visible_nodes(&self, config: &ConfigStore) -> Vec<ConnectionTreeNode> {
        let mut sessions = config.sessions().to_vec();
        connection_catalog::sort_sessions(&mut sessions, self.sort.into(), self.descending);

        let groups = visible_groups(config.connection_groups(), &sessions, &self.query);
        let mut nodes = Vec::new();
        for group in groups.iter().filter(|group| group_depth(group) == 0) {
            self.append_group(&mut nodes, group, &groups, &sessions, config);
        }
        for session in sessions.iter().filter(|session| session.group.is_none()) {
            if session_matches(session, &self.query) {
                nodes.push(ConnectionTreeNode::Session {
                    id: ConnectionNodeId::Session(session.id.clone()),
                    session_id: session.id.clone(),
                    depth: 0,
                });
            }
        }
        if self.show_deleted {
            append_deleted_nodes(
                &mut nodes,
                config.deleted_connection_groups(),
                config.deleted_sessions(),
            );
        }
        nodes
    }

    fn append_group(
        &self,
        nodes: &mut Vec<ConnectionTreeNode>,
        group: &str,
        groups: &[String],
        sessions: &[Session],
        config: &ConfigStore,
    ) {
        let depth = group_depth(group);
        let expanded = if self.query.is_empty() {
            self.expanded.contains(group)
        } else {
            true
        };
        nodes.push(ConnectionTreeNode::Group {
            id: ConnectionNodeId::Group(group.to_string()),
            name: group_name(group),
            depth,
            expanded,
        });
        if !expanded {
            return;
        }
        let prefix = format!("{group}/");
        for child in groups.iter().filter(|candidate| {
            candidate.starts_with(&prefix) && !candidate[prefix.len()..].contains('/')
        }) {
            self.append_group(nodes, child, groups, sessions, config);
        }
        for session in sessions.iter().filter(|session| {
            session.group.as_deref() == Some(group) && session_matches(session, &self.query)
        }) {
            nodes.push(ConnectionTreeNode::Session {
                id: ConnectionNodeId::Session(session.id.clone()),
                session_id: session.id.clone(),
                depth: depth + 1,
            });
        }
        let _ = config;
    }
}

fn visible_groups(groups: &[String], sessions: &[Session], query: &str) -> Vec<String> {
    if query.is_empty() {
        return groups.to_vec();
    }

    let mut result = groups
        .iter()
        .filter(|group| connection_catalog::group_matches_query(group, sessions, query))
        .cloned()
        .collect::<Vec<_>>();
    result.sort_by_key(|group| (group_depth(group), group.to_lowercase()));
    result
}

fn append_deleted_nodes(
    nodes: &mut Vec<ConnectionTreeNode>,
    groups: &[DeletedConnectionGroup],
    sessions: &[DeletedSession],
) {
    for group in groups {
        nodes.push(ConnectionTreeNode::DeletedGroup {
            id: ConnectionNodeId::DeletedGroup(group.name.clone()),
            name: group.name.clone(),
            depth: 0,
        });
        for session in &group.sessions {
            nodes.push(ConnectionTreeNode::DeletedSession {
                id: ConnectionNodeId::DeletedSession(session.id.clone()),
                session: Box::new(session.clone()),
                depth: 1,
            });
        }
    }
    for session in sessions {
        nodes.push(ConnectionTreeNode::DeletedSession {
            id: ConnectionNodeId::DeletedSession(session.session.id.clone()),
            session: Box::new(session.session.clone()),
            depth: 0,
        });
    }
}

fn session_matches(session: &Session, query: &str) -> bool {
    query.is_empty()
        || session.name.to_lowercase().contains(query)
        || session.host.to_lowercase().contains(query)
        || session.user.to_lowercase().contains(query)
}

fn group_depth(group: &str) -> usize {
    group.split('/').count().saturating_sub(1)
}

fn group_name(group: &str) -> String {
    group.rsplit('/').next().unwrap_or(group).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(name: &str, group: Option<&str>) -> Session {
        let mut value =
            Session::password("host".to_string(), 22, "user".to_string(), String::new());
        value.id = name.to_string();
        value.name = name.to_string();
        value.group = group.map(str::to_string);
        value
    }

    #[test]
    fn search_keeps_matching_ancestor_and_expands_it() {
        let mut config = ConfigStore::in_memory();
        config.add_connection_group("prod".to_string());
        config.add_connection_group("prod/eu".to_string());
        config.upsert(session("database", Some("prod/eu")));
        let mut state = ConnectionManagerState::default();
        state.set_query("database".to_string());
        let nodes = state.visible_nodes(&config);
        assert!(
            matches!(nodes[0], ConnectionTreeNode::Group { ref id, expanded: true, .. } if id == &ConnectionNodeId::Group("prod".to_string()))
        );
        assert!(nodes.iter().any(|node| matches!(node, ConnectionTreeNode::Session { session_id, .. } if session_id == "database")));
    }

    #[test]
    fn clearing_search_restores_expansion_state() {
        let mut state = ConnectionManagerState::default();
        state.expanded.insert("prod".to_string());
        state.set_query("db".to_string());
        state.expanded.clear();
        state.set_query(String::new());
        assert!(state.expanded.contains("prod"));
    }

    #[test]
    fn deleted_group_projection_keeps_its_session_snapshot() {
        let mut config = ConfigStore::in_memory();
        config.add_connection_group("prod".to_string());
        config.upsert(session("database", Some("prod")));
        assert!(config.soft_delete_connection_group("prod"));

        let state = ConnectionManagerState {
            show_deleted: true,
            ..ConnectionManagerState::default()
        };
        let nodes = state.visible_nodes(&config);

        assert!(nodes.iter().any(|node| matches!(
            node,
            ConnectionTreeNode::DeletedSession { session, depth: 1, .. }
                if session.id == "database" && session.name == "database"
        )));
    }
}
