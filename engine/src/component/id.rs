use uuid::Uuid;

use reflect::Reflect;
use reflect::ReflectDefault;
use utils::Component;

use crate as engine;
use crate::component::{Component, ReflectComponent};

#[derive(Clone, Component, Reflect)]
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
