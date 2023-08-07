use specs::{Component, VecStorage};
use crate::assets::Mesh;

struct ComponentMesh(Mesh);
impl Component for ComponentMesh {
    type Storage = VecStorage<Self>;
}