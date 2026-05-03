use legion::world::{Entry, EntryRef};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate as engine;
use crate::{
    reflect::{Reflect, ReflectDefault},
    scene::Scene,
    utils::TypeUuid,
};

use super::GameObject;

/// Serializable reference to a [`GameObject`] by UUID.
///
/// This is the stable handle that components should store when they need to
/// point at another game object across serialization, prefab instantiation, or
/// scene reloads.
#[derive(Default, Clone, Copy, TypeUuid, Reflect)]
#[uuid = "a20d9c21-adea-4af1-ad75-05828aad89de"]
#[reflect(Default)]
#[repr(C)]
pub struct GameObjectRef {
    id: Uuid,
}

impl Serialize for GameObjectRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.id.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GameObjectRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Uuid::deserialize(deserializer).map(|id| Self { id })
    }
}

impl GameObjectRef {
    /// Creates a reference from a game object's persistent UUID.
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl GameObjectRef {
    /// Resolves the referenced game object inside `scene`.
    pub fn game_object(&self, scene: &Scene) -> Option<GameObject> {
        scene.find(self.id)
    }

    /// Returns an immutable Legion entry for the referenced object when it
    /// exists in `scene`.
    pub fn entry<'a>(&self, scene: &'a Scene) -> Option<EntryRef<'a>> {
        self.game_object(scene).and_then(|go| scene.entry(go))
    }

    /// Returns a mutable Legion entry for the referenced object when it exists
    /// in `scene`.
    pub fn entry_mut<'a>(&self, scene: &'a mut Scene) -> Option<Entry<'a>> {
        self.game_object(scene).and_then(|go| scene.entry_mut(go))
    }

    /// Returns the persistent UUID stored by this reference.
    pub fn id(&self) -> Uuid {
        self.id
    }
}
