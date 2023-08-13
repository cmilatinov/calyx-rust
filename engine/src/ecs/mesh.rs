use specs::{Component, VecStorage};
use crate::assets::Mesh;
use crate::core::refs::Ref;

pub struct ComponentMesh {
    pub mesh: Ref<Mesh>
}

impl Component for ComponentMesh {
    type Storage = VecStorage<Self>;
}
