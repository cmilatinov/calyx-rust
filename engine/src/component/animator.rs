use serde::{Deserialize, Serialize};

use crate as engine;
use crate::assets::animation::Animation;
use crate::core::Ref;
use crate::{
    reflect::{Reflect, ReflectDefault},
    utils::{ReflectTypeUuidDynamic, TypeUuid},
};

use super::{Component, ReflectComponent};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "f24db81d-7054-40b8-8f3c-d9740c03948e"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Animator")]
#[serde(default)]
pub struct ComponentAnimator {
    pub animation: Option<Ref<Animation>>,
    pub time: f64,
}

impl Component for ComponentAnimator {}
