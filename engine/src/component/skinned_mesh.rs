use serde::{Deserialize, Serialize};

use crate as engine;
use crate::assets::material::Material;
use crate::assets::mesh::{BoneTransform, Mesh};
use crate::core::Ref;
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::GameObjectRef;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};

use super::{Component, ReflectComponent};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "bb784426-a5ec-4995-a86a-c40e7e2cb3ab"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Skinned Mesh Renderer")]
#[serde(default)]
pub struct ComponentSkinnedMesh {
    pub material: Option<Ref<Material>>,
    pub mesh: Option<Ref<Mesh>>,
    pub root_bone: Option<GameObjectRef>,
    #[reflect_skip]
    #[serde(skip_serializing, skip_deserializing)]
    pub bone_transforms: Vec<BoneTransform>,
}

impl Component for ComponentSkinnedMesh {}
