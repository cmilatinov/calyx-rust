use uuid::Uuid;

use engine_derive::TypeUuid;
use reflect::Reflect;
use reflect::ReflectDefault;

use crate as engine;
use crate::component::{Component, ReflectComponent};

#[derive(TypeUuid, Component, Reflect)]
#[uuid = "02289c92-3412-406e-a7e5-3bbb15d7041e"]
#[reflect(Default, Component)]
#[reflect_attr(name = "Game Object")]
pub struct ComponentID {
    pub name: String,
    pub id: Uuid,
}

impl Default for ComponentID {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "Game Object".to_string(),
        }
    }
}

impl Component for ComponentID {}
