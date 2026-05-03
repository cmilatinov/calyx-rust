use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Core identification component present on every scene object.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "02289c92-3412-406e-a7e5-3bbb15d7041e"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Game Object")]
#[serde(default)]
#[repr(C)]
pub struct ComponentID {
    /// Persistent UUID used for serialization and references.
    #[reflect_attr(name = "ID")]
    pub id: Uuid,
    /// Display name shown in editor tooling.
    pub name: String,
    /// Whether the object is visible in editor and rendering flows.
    pub visible: bool,
}

impl Default for ComponentID {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: String::from("Game Object"),
            visible: true,
        }
    }
}

impl Component for ComponentID {}
