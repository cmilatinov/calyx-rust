use bimap::BiHashMap;
use legion::world::{Entry, EntryRef};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate as engine;
use crate::{
    reflect::{type_registry::TypeRegistry, Reflect, ReflectDefault, TypeInfo},
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

/// Rewrites every [`GameObjectRef`] reachable from `value` through `id_mapping`,
/// walking reflected struct fields and container elements recursively.
///
/// Nil references are left alone. A non-nil reference with no entry in
/// `id_mapping` is left unchanged and passed to `on_unknown` with its field
/// path, which starts at `path`.
pub(crate) fn remap_object_refs(
    value: &mut dyn Reflect,
    types: &TypeRegistry,
    id_mapping: &BiHashMap<Uuid, Uuid>,
    path: &str,
    on_unknown: &mut dyn FnMut(&str, Uuid),
) {
    let mut path = path.to_string();
    remap_value(value, types, id_mapping, &mut path, on_unknown);
}

fn remap_value(
    value: &mut dyn Reflect,
    types: &TypeRegistry,
    id_mapping: &BiHashMap<Uuid, Uuid>,
    path: &mut String,
    on_unknown: &mut dyn FnMut(&str, Uuid),
) {
    if let Some(reference) = value.downcast_mut::<GameObjectRef>() {
        if reference.id.is_nil() {
            return;
        }
        match id_mapping.get_by_left(&reference.id) {
            Some(new_id) => reference.id = *new_id,
            None => on_unknown(path, reference.id),
        }
        return;
    }
    if let Some(TypeInfo::Struct(info)) = types.type_info_by_id(value.uuid()) {
        for (name, field) in &info.fields {
            if let Some(field_value) = field.get_reflect_mut(&mut *value) {
                let len = path.len();
                path.push('.');
                path.push_str(name);
                remap_value(field_value, types, id_mapping, path, on_unknown);
                path.truncate(len);
            }
        }
    }
    let mut index = 0usize;
    value.visit_elements_mut(&mut |element| {
        let len = path.len();
        path.push_str(&format!("[{index}]"));
        index += 1;
        remap_value(element, types, id_mapping, path, on_unknown);
        path.truncate(len);
    });
}
