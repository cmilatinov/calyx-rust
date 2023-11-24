use reflect::Reflect;
use reflect::ReflectDefault;
use utils::Component;

use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::math::Transform;

#[derive(Default, Clone, Component, Reflect)]
#[reflect(Default, Component)]
#[reflect_attr(name = "Transform")]
pub struct ComponentTransform {
    pub transform: Transform,
}

impl Component for ComponentTransform {}
