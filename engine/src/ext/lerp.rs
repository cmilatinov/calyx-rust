use crate::math::Transform;
use lerp::Lerp;

impl Lerp<f32> for Transform {
    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            position: self.position.lerp(other.position, t),
            rotation: self.rotation.slerp(&other.rotation, t),
            scale: self.scale.lerp(other.scale, t),
        }
    }
}
