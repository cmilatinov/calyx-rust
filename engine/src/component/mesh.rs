use crate as engine;
use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::AssetRef;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Mesh renderer component for static geometry.
#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "93fd32b1-7127-4c88-8e89-893512af58de"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Mesh Renderer")]
#[serde(default)]
#[repr(C)]
pub struct ComponentMesh {
    /// Mesh asset to draw.
    pub mesh: AssetRef<Mesh>,
    /// Material asset applied to the mesh.
    pub material: AssetRef<Material>,
}

impl Component for ComponentMesh {}
