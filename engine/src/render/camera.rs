use glm::Mat4;

use crate::math::transform::Transform;

pub struct Camera {
    pub projection: Mat4,
    pub transform: Transform
}

pub trait CameraLike {
    fn update(&mut self) {}
}

impl CameraLike for Camera {}

impl Default for Camera {
    fn default() -> Self {
        Self {
            projection: Mat4::identity(),
            transform: Transform::default()
        }
    }
}
