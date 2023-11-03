use reflect::Reflect;
use reflect::ReflectDefault;
use utils::Component;

use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::math::Transform;

#[derive(Default, Component, Reflect)]
#[reflect(Default, Component)]
pub struct ComponentTransform {
    pub transform: Transform,
}

impl Component for ComponentTransform {}
