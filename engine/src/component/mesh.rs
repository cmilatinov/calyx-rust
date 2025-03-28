use crate as engine;
use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::AssetRef;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "93fd32b1-7127-4c88-8e89-893512af58de"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Mesh Renderer")]
#[serde(default)]
pub struct ComponentMesh {
    pub mesh: AssetRef<Mesh>,
    pub material: AssetRef<Material>,
}

impl Component for ComponentMesh {}
