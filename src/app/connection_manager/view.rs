#![allow(dead_code)]

use super::actions::ConnectionManagerAction;
use super::state::{ConnectionNodeId, ConnectionTreeNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionContext {
    Session,
    Group,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionMenuItem {
    Connect,
    Edit,
    Rename,
    Copy,
    Cut,
    Paste,
    Delete,
    Restore,
    CopyAddress,
    PasteAddress,
    MoveToGroup,
    NewConnection,
    NewGroup,
    Sort,
    Import,
    Export,
}

pub fn context_for(node: Option<&ConnectionTreeNode>) -> ConnectionContext {
    match node {
        Some(ConnectionTreeNode::Session { .. } | ConnectionTreeNode::DeletedSession { .. }) => {
            ConnectionContext::Session
        }
        Some(ConnectionTreeNode::Group { .. } | ConnectionTreeNode::DeletedGroup { .. }) => {
            ConnectionContext::Group
        }
        None => ConnectionContext::Empty,
    }
}

pub fn menu_items(context: ConnectionContext, deleted: bool) -> Vec<ConnectionMenuItem> {
    let mut items = match context {
        ConnectionContext::Session => vec![
            ConnectionMenuItem::Connect,
            ConnectionMenuItem::Edit,
            ConnectionMenuItem::Rename,
            ConnectionMenuItem::Copy,
            ConnectionMenuItem::Cut,
            ConnectionMenuItem::Paste,
            ConnectionMenuItem::Delete,
            ConnectionMenuItem::CopyAddress,
            ConnectionMenuItem::PasteAddress,
            ConnectionMenuItem::MoveToGroup,
        ],
        ConnectionContext::Group => vec![
            ConnectionMenuItem::NewConnection,
            ConnectionMenuItem::NewGroup,
            ConnectionMenuItem::Rename,
            ConnectionMenuItem::Copy,
            ConnectionMenuItem::Cut,
            ConnectionMenuItem::Paste,
            ConnectionMenuItem::Delete,
        ],
        ConnectionContext::Empty => vec![
            ConnectionMenuItem::NewConnection,
            ConnectionMenuItem::NewGroup,
            ConnectionMenuItem::Paste,
        ],
    };
    if deleted {
        items.push(ConnectionMenuItem::Restore);
    }
    items.extend([
        ConnectionMenuItem::Sort,
        ConnectionMenuItem::Import,
        ConnectionMenuItem::Export,
    ]);
    items
}

pub fn action_for(
    item: ConnectionMenuItem,
    node: &ConnectionNodeId,
) -> Option<ConnectionManagerAction> {
    match (item, node) {
        (ConnectionMenuItem::Copy, ConnectionNodeId::Session(id)) => {
            Some(ConnectionManagerAction::CopySession { id: id.clone() })
        }
        (ConnectionMenuItem::Cut, ConnectionNodeId::Session(id)) => {
            Some(ConnectionManagerAction::CutSession { id: id.clone() })
        }
        (ConnectionMenuItem::CopyAddress, ConnectionNodeId::Session(id)) => {
            Some(ConnectionManagerAction::CopyAddress { id: id.clone() })
        }
        (ConnectionMenuItem::Copy, ConnectionNodeId::Group(name)) => {
            Some(ConnectionManagerAction::CopyGroup { name: name.clone() })
        }
        (ConnectionMenuItem::Delete, ConnectionNodeId::Session(id)) => {
            Some(ConnectionManagerAction::DeleteSession { id: id.clone() })
        }
        (ConnectionMenuItem::Delete, ConnectionNodeId::Group(name)) => {
            Some(ConnectionManagerAction::DeleteGroup { name: name.clone() })
        }
        (ConnectionMenuItem::Restore, ConnectionNodeId::DeletedSession(id)) => {
            Some(ConnectionManagerAction::RestoreSession { id: id.clone() })
        }
        (ConnectionMenuItem::Restore, ConnectionNodeId::DeletedGroup(name)) => {
            Some(ConnectionManagerAction::RestoreGroup { name: name.clone() })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_menus_cover_session_group_and_empty_regions() {
        assert!(
            menu_items(ConnectionContext::Session, false)
                .contains(&ConnectionMenuItem::CopyAddress)
        );
        assert!(
            menu_items(ConnectionContext::Group, false).contains(&ConnectionMenuItem::NewGroup)
        );
        assert!(menu_items(ConnectionContext::Empty, false).contains(&ConnectionMenuItem::Import));
    }
}
