use engine_derive::impl_reflect_value;
use legion::world::{Entry, EntryRef};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate as engine;
use crate::{
    reflect::{Reflect, ReflectDefault},
    scene::Scene,
    utils::TypeUuid,
};

use super::GameObject;

#[derive(Default, Clone, Copy, TypeUuid, Serialize, Deserialize, Reflect)]
#[uuid = "a20d9c21-adea-4af1-ad75-05828aad89de"]
#[serde(transparent)]
#[repr(transparent)]
pub struct GameObjectRef {
    pub(crate) id: Uuid,
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
}

impl_reflect_value!(Option<GameObjectRef>(Default));
