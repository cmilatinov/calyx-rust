use crate::core::Time;
use crate::input::Input;
use crate::math;
use nalgebra_glm::Mat4;

/// Render camera projection data derived from scene camera components.
pub struct Camera {
    /// Projection matrix.
    pub projection: Mat4,
    /// Aspect ratio used to build the projection.
    pub aspect: f32,
    /// Horizontal field of view in radians.
    pub fov_x: f32,
    /// Near clipping plane distance.
    pub near_plane: f32,
    /// Far clipping plane distance.
    pub far_plane: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera::new(16.0 / 9.0, 70f32.to_radians(), 0.1, 1000.0)
    }
}

impl Camera {
    /// Creates a camera projection with the supplied parameters.
    pub fn new(aspect: f32, fov_x: f32, near_plane: f32, far_plane: f32) -> Self {
        let mut camera = Self {
            projection: Mat4::identity(),
            aspect,
            fov_x,
            near_plane,
            far_plane,
        };
        camera.update_projection();
        camera
    }

    /// Rebuilds the projection matrix from the current camera parameters.
    pub fn update_projection(&mut self) {
        self.projection = nalgebra_glm::perspective_lh::<f32>(
            self.aspect,
            math::to_fov_y(self.aspect, self.fov_x),
            self.near_plane,
            self.far_plane,
        );
    }
}

/// Trait for camera controllers that update from time and input.
pub trait CameraLike {
    /// Updates the camera/controller state for one frame.
    fn update(&mut self, time: &Time, input: &Input);
}
