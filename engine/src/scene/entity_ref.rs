use engine_derive::impl_reflect_value;
use indextree::NodeId;
use legion::world::{Entry, EntryRef};
use legion::{Entity, EntityStore, World};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate as engine;
use crate::{
    reflect::{Reflect, ReflectDefault},
    scene::Scene,
    utils::TypeUuid,
};

#[derive(Default, Clone, Copy, TypeUuid, Serialize, Deserialize, Reflect)]
#[uuid = "a20d9c21-adea-4af1-ad75-05828aad89de"]
#[serde(transparent)]
#[repr(transparent)]
pub struct EntityRef {
    id: Uuid,
}

impl EntityRef {
    pub fn node(&self, scene: &Scene) -> Option<NodeId> {
        scene.get_node_by_uuid(self.id)
    }

    pub fn entity(&self, scene: &Scene) -> Option<Entity> {
        scene.get_entity_by_uuid(self.id)
    }

    pub fn entry_ref<'a, T: legion::storage::Component>(
        &self,
        scene: &Scene,
        world: &'a World,
    ) -> Option<EntryRef<'a>> {
        self.entity(scene).and_then(|e| world.entry_ref(e).ok())
    }

    pub fn entry<'a, T: legion::storage::Component>(
        &self,
        scene: &Scene,
        world: &'a mut World,
    ) -> Option<Entry<'a>> {
        self.entity(scene).and_then(|e| world.entry(e))
    }
}

impl_reflect_value!(Option<EntityRef>(Default));
