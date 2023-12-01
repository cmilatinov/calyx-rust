use uuid::Uuid;

use engine_derive::Component;
use reflect::Reflect;
use reflect::ReflectDefault;

use crate as engine;
use crate::component::{Component, ReflectComponent};

#[derive(Component, Reflect)]
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
