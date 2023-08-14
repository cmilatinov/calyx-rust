use std::any::Any;
use bevy_reflect::prelude::ReflectDefault;
use bevy_reflect::{
    ReflectSerialize, ReflectDeserialize,
    ReflectMut, ReflectOwned, ReflectRef,
    TypeInfo, TypeRegistry
};
use bevy_reflect_derive::impl_reflect_value;
use specs::VecStorage;
use uuid::Uuid;
use crate::component;
use crate::ecs::ComponentInfo;

#[derive(Default, Debug, Clone, Hash)]
pub struct UUID(pub Uuid);

impl std::ops::Deref for UUID {
    type Target = Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl_reflect_value!(Uuid(Debug, Hash));
impl_reflect_value!(UUID(Debug, Hash));

component! {
    pub struct ComponentID {
        pub id: UUID,
        pub name: String
    }
}

impl ComponentID {
    pub fn new() -> Self {
        Self {
            id: UUID(Uuid::new_v4()),
            name: "Game Object".to_string(),
        }
    }
}
