use reflect::Reflect;
use reflect::ReflectDefault;
use utils::utils_derive::Component;
use crate::math::Transform;
use crate::component::{Component, ReflectComponent};
use crate as engine;

#[derive(Default, Component, Reflect)]
#[reflect(Default, Component)]
pub struct ComponentTransform {
    pub transform: Transform
}