use crate::net::NetworkObjectId;
use crate::scene::GameObject;
use std::collections::HashMap;

/// Index of a scene's network objects by id.
///
/// Every object that claims an id stays listed under it, so when two objects
/// briefly share an id (a prefab instance made from a live network object,
/// before it gets its own id) and one of them lets go, the id resolves to the
/// other again. A lookup returns the latest claimant.
#[derive(Debug, Default)]
pub(crate) struct NetworkObjectIndex {
    /// Claimants of each id, oldest first.
    by_id: HashMap<NetworkObjectId, Vec<GameObject>>,
    /// Id each indexed object claims.
    by_object: HashMap<GameObject, NetworkObjectId>,
}

impl NetworkObjectIndex {
    /// Records that `game_object` claims `network_object_id`, as its latest
    /// claimant, and forgets any other id it claimed.
    pub(crate) fn insert(&mut self, network_object_id: NetworkObjectId, game_object: GameObject) {
        self.remove(game_object);
        self.by_object.insert(game_object, network_object_id);
        self.by_id
            .entry(network_object_id)
            .or_default()
            .push(game_object);
    }

    /// Forgets the id `game_object` claims, if any.
    pub(crate) fn remove(&mut self, game_object: GameObject) {
        let Some(network_object_id) = self.by_object.remove(&game_object) else {
            return;
        };
        if let Some(claimants) = self.by_id.get_mut(&network_object_id) {
            claimants.retain(|&claimant| claimant != game_object);
            if claimants.is_empty() {
                self.by_id.remove(&network_object_id);
            }
        }
    }

    /// Returns the latest claimant of `network_object_id`.
    pub(crate) fn get(&self, network_object_id: NetworkObjectId) -> Option<GameObject> {
        self.by_id
            .get(&network_object_id)
            .and_then(|claimants| claimants.last())
            .copied()
    }
}

impl FromIterator<(NetworkObjectId, GameObject)> for NetworkObjectIndex {
    fn from_iter<T: IntoIterator<Item = (NetworkObjectId, GameObject)>>(iter: T) -> Self {
        let mut index = Self::default();
        for (network_object_id, game_object) in iter {
            index.insert(network_object_id, game_object);
        }
        index
    }
}
