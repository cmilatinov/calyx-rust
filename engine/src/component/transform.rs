use reflect::{Reflect, ReflectDefault, TypeUuid};
use serde::{Deserialize, Serialize};

use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::math::Transform;

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "c5b3b71f-1f14-4b5b-9881-436118684d29"]
#[reflect(Default, Component)]
#[reflect_attr(name = "Transform")]
pub struct ComponentTransform {
    pub transform: Transform,
}

impl Component for ComponentTransform {}
