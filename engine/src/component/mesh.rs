use reflect::{Reflect, ReflectDefault, TypeUuid};

use crate as engine;
use crate::assets::mesh::Mesh;
use crate::component::{Component, ReflectComponent};
use crate::core::OptionRef;

#[derive(Default, TypeUuid, Component, Reflect)]
#[uuid = "93fd32b1-7127-4c88-8e89-893512af58de"]
#[reflect(Default, Component)]
#[reflect_attr(name = "Mesh Renderer")]
pub struct ComponentMesh {
    pub mesh: OptionRef<Mesh>,
}

impl Component for ComponentMesh {}
