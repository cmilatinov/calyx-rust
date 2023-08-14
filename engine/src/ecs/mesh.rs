use bevy_reflect::TypeRegistry;
use bevy_reflect_derive::impl_reflect_value;
use specs::{Component, VecStorage};

use std::sync::{Arc, RwLock};

use crate::assets::mesh::Mesh;
use crate::component;
use crate::core::{OptionRef, Ref};
use crate::ecs::ComponentInfo;

impl_reflect_value!(Ref<Mesh>(Debug));

component! {
    pub struct ComponentMesh {
        pub mesh: Ref<Mesh>
    }
}
