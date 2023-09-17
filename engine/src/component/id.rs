use uuid::Uuid;
use reflect::Reflect;
use reflect::ReflectDefault;
use utils::utils_derive::Component;
use crate::component::{Component, ReflectComponent};
use crate as engine;

#[derive(Component, Reflect)]
#[reflect(Default, Component)]
pub struct ComponentID {
    pub name: String,
    pub id: Uuid
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