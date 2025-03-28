use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::math::Transform;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "c5b3b71f-1f14-4b5b-9881-436118684d29"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Transform")]
#[serde(default)]
pub struct ComponentTransform {
    pub transform: Transform,
}

impl Component for ComponentTransform {}
