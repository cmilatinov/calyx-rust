use std::collections::HashSet;
use engine::indextree::NodeId;
use engine::uuid::Uuid;

#[derive(Clone, PartialEq, Debug)]
pub enum EditorSelection {
    Entity(HashSet<NodeId>),
    Asset(HashSet<Uuid>)
}

impl EditorSelection {
    pub fn first_entity(&self) -> Option<NodeId> {
        if let EditorSelection::Entity(selection) = self {
            return selection.iter().next().map(|n| *n);
        }
        None
    }
}