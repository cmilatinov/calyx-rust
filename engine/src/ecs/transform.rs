use specs::{VecStorage};

use crate::math::transform::Transform;

#[derive(Default)]
pub struct ComponentTransform {
    pub transform: Transform
}

impl specs::Component for ComponentTransform {
    type Storage = VecStorage<Self>;
}
