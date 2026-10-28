use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::math::Transform;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Scene component attached to a skeleton bone node. Each skinned mesh finds its bones by name
/// and keeps their indices and inverse bind matrices itself, since meshes sharing a skeleton can
/// order its bones differently.
#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "4a83ab8d-8a10-462a-90bd-8a0cf7f32b7f"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Bone")]
#[serde(default)]
pub struct ComponentBone {
    /// Bone name as authored in the source mesh.
    pub name: String,
    /// Rest-pose local transform, captured before the animator first overwrites it.
    #[reflect_skip]
    pub bind_pose: Option<Transform>,
}

impl Component for ComponentBone {}
