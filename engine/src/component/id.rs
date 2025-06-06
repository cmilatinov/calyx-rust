use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "02289c92-3412-406e-a7e5-3bbb15d7041e"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Game Object")]
#[serde(default)]
#[repr(C)]
pub struct ComponentID {
    #[reflect_attr(name = "ID")]
    pub id: Uuid,
    pub name: String,
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
