use super::{Component, ReflectComponent};
use crate as engine;
use crate::assets::material::Material;
use crate::assets::mesh::{BoneTransform, Mesh};
use crate::assets::AssetRef;
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::GameObjectRef;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "bb784426-a5ec-4995-a86a-c40e7e2cb3ab"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Skinned Mesh Renderer")]
#[serde(default)]
#[repr(C)]
pub struct ComponentSkinnedMesh {
    pub material: AssetRef<Material>,
    pub mesh: AssetRef<Mesh>,
    pub root_bone: GameObjectRef,
    #[reflect_skip]
    #[serde(skip)]
    pub bone_transforms: Vec<BoneTransform>,
}

impl Component for ComponentSkinnedMesh {}
