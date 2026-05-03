use super::{Component, ReflectComponent};
use crate as engine;
use crate::assets::material::Material;
use crate::assets::mesh::{BoneTransform, Mesh};
use crate::assets::AssetRef;
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::GameObjectRef;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Mesh renderer component driven by a bone hierarchy.
#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "bb784426-a5ec-4995-a86a-c40e7e2cb3ab"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Skinned Mesh Renderer")]
#[serde(default)]
#[repr(C)]
pub struct ComponentSkinnedMesh {
    /// Material asset applied to the mesh.
    pub material: AssetRef<Material>,
    /// Skinned mesh asset.
    pub mesh: AssetRef<Mesh>,
    /// Root bone object for the skeleton.
    pub root_bone: GameObjectRef,
    /// Runtime bone matrices uploaded to the renderer.
    #[reflect_skip]
    #[serde(skip)]
    pub bone_transforms: Vec<BoneTransform>,
}

impl Component for ComponentSkinnedMesh {}
