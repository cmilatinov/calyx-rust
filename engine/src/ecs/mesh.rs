use specs::{Component, VecStorage};

use crate::assets::mesh::Mesh;
use crate::core::Ref;

pub struct ComponentMesh {
    pub mesh: Ref<Mesh>
}

impl Component for ComponentMesh {
    type Storage = VecStorage<Self>;
}
