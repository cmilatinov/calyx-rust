use std::collections::HashSet;

use engine::uuid::Uuid;

#[derive(Default, Clone, PartialEq, Debug)]
pub enum EditorSelection {
    GameObject(HashSet<Uuid>),
    Asset(HashSet<Uuid>),
    #[default]
    None,
}

impl EditorSelection {
    pub fn none() -> Self {
        Self::None
    }

    pub fn from_game_object_id(id: Uuid) -> Self {
        Self::GameObject([id].into())
    }

    pub fn from_asset_id(id: Uuid) -> Self {
        Self::Asset([id].into())
    }

    pub fn game_objects_iter<'a>(&'a self) -> Option<impl Iterator<Item = Uuid> + 'a> {
        if let EditorSelection::GameObject(set) = self {
            Some(set.iter().copied())
        } else {
            None
        }
    }

    pub fn game_objects_set(&self) -> Option<&HashSet<Uuid>> {
        if let EditorSelection::GameObject(set) = self {
            Some(set)
        } else {
            None
        }
    }

    pub fn assets_iter<'a>(&'a self) -> Option<impl Iterator<Item = Uuid> + 'a> {
        if let EditorSelection::Asset(set) = self {
            Some(set.iter().copied())
        } else {
            None
        }
    }

    pub fn assets_set(&self) -> Option<&HashSet<Uuid>> {
        if let EditorSelection::Asset(set) = self {
            Some(set)
        } else {
            None
        }
    }

    pub fn first_game_object(&self) -> Option<Uuid> {
        self.game_objects_iter().and_then(|mut iter| iter.next())
    }

    pub fn last_game_object(&self) -> Option<Uuid> {
        self.game_objects_iter().and_then(|iter| iter.last())
    }

    pub fn first_asset(&self) -> Option<Uuid> {
        self.assets_iter().and_then(|mut iter| iter.next())
    }

    pub fn last_asset(&self) -> Option<Uuid> {
        self.assets_iter().and_then(|iter| iter.last())
    }

    pub fn contains_game_object(&self, id: Uuid) -> bool {
        self.game_objects_set()
            .map(|set| set.contains(&id))
            .unwrap_or(false)
    }

    pub fn contains_asset(&self, id: Uuid) -> bool {
        self.assets_set()
            .map(|set| set.contains(&id))
            .unwrap_or(false)
    }
}
