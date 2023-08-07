use specs::{Component, VecStorage};
use crate::math::transform::Transform;

pub struct ComponentTransform {
    pub transform: Transform
}

impl Component for ComponentTransform {
    type Storage = VecStorage<Self>;
}
