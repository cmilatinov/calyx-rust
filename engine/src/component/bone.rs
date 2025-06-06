use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::Mat4;
use serde::{Deserialize, Serialize};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "4a83ab8d-8a10-462a-90bd-8a0cf7f32b7f"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Bone")]
#[serde(default)]
#[repr(C)]
pub struct ComponentBone {
    pub name: String,
    #[reflect_attr(hide)]
    pub index: usize,
    #[reflect_skip]
    pub offset_matrix: Mat4,
}

impl Component for ComponentBone {}
