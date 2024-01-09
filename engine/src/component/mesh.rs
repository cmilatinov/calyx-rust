use serde::{Deserialize, Serialize};

use crate as engine;
use crate::assets::mesh::Mesh;
use crate::component::{Component, ReflectComponent};
use crate::core::Ref;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "93fd32b1-7127-4c88-8e89-893512af58de"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Mesh Renderer")]
pub struct ComponentMesh {
    pub mesh: Option<Ref<Mesh>>,
}

impl Component for ComponentMesh {}
