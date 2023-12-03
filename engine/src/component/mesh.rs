use reflect::Reflect;
use reflect::ReflectDefault;

use crate as engine;
use crate::assets::mesh::Mesh;
use crate::component::{Component, ReflectComponent};
use crate::core::OptionRef;

#[derive(Default, Component, Reflect)]
#[reflect(Default, Component)]
#[reflect_attr(name = "Mesh Renderer")]
pub struct ComponentMesh {
    pub mesh: OptionRef<Mesh>,
}

impl Component for ComponentMesh {}
