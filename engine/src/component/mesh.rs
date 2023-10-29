use reflect::Reflect;
use reflect::ReflectDefault;
use utils::Component;

use crate as engine;
use crate::assets::mesh::Mesh;
use crate::component::{Component, ReflectComponent};
use crate::core::Ref;

#[derive(Default, Component, Reflect)]
#[reflect(Default, Component)]
pub struct ComponentMesh {
    pub mesh: Ref<Mesh>,
}

impl Component for ComponentMesh {}
