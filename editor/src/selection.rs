use std::collections::HashSet;

use engine::indextree::NodeId;
use engine::uuid::Uuid;

#[derive(Clone, PartialEq, Debug)]
pub enum EditorSelection {
    Entity(HashSet<NodeId>),
    Asset(HashSet<Uuid>),
}

impl EditorSelection {
    pub fn first_entity(&self) -> Option<NodeId> {
        if let EditorSelection::Entity(selection) = self {
            selection.iter().next().copied()
        } else {
            None
        }
    }
    pub fn first_asset(&self) -> Option<Uuid> {
        if let EditorSelection::Asset(selection) = self {
            selection.iter().next().copied()
        } else {
            None
        }
    }
}
