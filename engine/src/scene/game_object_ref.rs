use std::cell::RefCell;

use bimap::BiHashMap;
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
        Uuid::deserialize(deserializer).map(|id| Self { id: remap_id(id) })
    }
}

thread_local! {
    /// Id map applied to every [`GameObjectRef`] deserialized on this thread
    /// while a [`RemapScope`] is held.
    static ID_REMAP: RefCell<Option<BiHashMap<Uuid, Uuid>>> = const { RefCell::new(None) };
}

/// Remaps `id` through the active [`RemapScope`], if any. Nil ids are left
/// alone; an id the map does not know is left unchanged with a warning.
fn remap_id(id: Uuid) -> Uuid {
    if id.is_nil() {
        return id;
    }
    ID_REMAP.with_borrow(|map| {
        let Some(map) = map else {
            return id;
        };
        match map.get_by_left(&id) {
            Some(new_id) => *new_id,
            None => {
                log::warn!(
                    "Object reference to {id} is not part of the prefab being instantiated; leaving it unchanged"
                );
                id
            }
        }
    })
}

/// Guard that remaps every [`GameObjectRef`] deserialized on this thread
/// through an id map while it is alive.
///
/// Prefab instantiation holds one while it deserializes component data, so
/// references are remapped wherever serde reaches them: nested structs, lists,
/// enum variants and tuple structs alike. Dropping the guard restores whatever
/// map was active before it, so scopes nest.
#[must_use = "remapping stops as soon as the scope is dropped"]
pub(crate) struct RemapScope {
    previous: Option<BiHashMap<Uuid, Uuid>>,
}

impl RemapScope {
    /// Activates `map` until the returned guard is dropped.
    pub(crate) fn enter(map: BiHashMap<Uuid, Uuid>) -> Self {
        let previous = ID_REMAP.with_borrow_mut(|active| active.replace(map));
        Self { previous }
    }
}

impl Drop for RemapScope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        ID_REMAP.with_borrow_mut(|active| *active = previous);
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

    /// Returns the persistent UUID stored by this reference.
    pub fn id(&self) -> Uuid {
        self.id
    }
}
