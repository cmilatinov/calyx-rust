use specs::{Component, VecStorage};
use crate::math::transform::Transform;

#[derive(Default)]
pub struct ComponentTransform {
    pub transform: Transform
}

impl Component for ComponentTransform {
    type Storage = VecStorage<Self>;
}
