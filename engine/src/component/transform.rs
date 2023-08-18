use specs::{VecStorage};

use crate::math::Transform;

#[derive(Default)]
pub struct ComponentTransform {
    pub transform: Transform
}

impl specs::Component for ComponentTransform {
    type Storage = VecStorage<Self>;
}
