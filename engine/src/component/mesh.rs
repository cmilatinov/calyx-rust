use reflect::Reflect;
use reflect::ReflectDefault;
use utils::utils_derive::Component;
use crate::assets::mesh::Mesh;
use crate::core::{Ref};
use crate::component::{Component, ReflectComponent};
use crate as engine;

#[derive(Default, Component, Reflect)]
#[reflect(Default, Component)]
pub struct ComponentMesh {
    pub mesh: Ref<Mesh>
}

impl Component for ComponentMesh {}