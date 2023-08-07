use specs::{Component, VecStorage};
use crate::math::transform::Transform;

impl Component for Transform {
    type Storage = VecStorage<Self>;
}
