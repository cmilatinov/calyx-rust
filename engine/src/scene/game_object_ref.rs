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

#[derive(Default, Clone, Copy, TypeUuid, Reflect)]
#[uuid = "a20d9c21-adea-4af1-ad75-05828aad89de"]
#[reflect(Default)]
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
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl GameObjectRef {
    pub fn game_object(&self, scene: &Scene) -> Option<GameObject> {
        scene.get_game_object_by_uuid(self.id)
    }

    pub fn entry<'a, T: legion::storage::Component>(
        &self,
        scene: &'a Scene,
    ) -> Option<EntryRef<'a>> {
        self.game_object(scene).and_then(|go| scene.entry(go))
    }

    pub fn entry_mut<'a, T: legion::storage::Component>(
        &self,
        scene: &'a mut Scene,
    ) -> Option<Entry<'a>> {
        self.game_object(scene).and_then(|go| scene.entry_mut(go))
    }

    pub fn id(&self) -> Uuid {
        self.id
    }
}
