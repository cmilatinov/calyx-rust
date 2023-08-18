use crate::assets::mesh::Mesh;
use crate::component;
use crate::core::{Ref};


component! {
    pub struct ComponentMesh {
        pub mesh: Ref<Mesh>
    }
}
