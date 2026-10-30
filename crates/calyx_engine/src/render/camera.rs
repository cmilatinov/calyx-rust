use crate::core::Time;
use crate::input::Input;
use crate::math::{self, Transform};
use nalgebra_glm::{Mat4, Vec2, Vec3, Vec4};

/// World-space ray produced by unprojecting a screen position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Camera position the ray starts from.
    pub origin: Vec3,
    /// Normalized direction the ray travels in.
    pub direction: Vec3,
}

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

    /// Unprojects a pixel position into a world-space ray.
    ///
    /// `screen_position` is measured in pixels from the top-left corner of a viewport of
    /// `viewport_size`. Returns `None` for an empty viewport or a degenerate view.
    pub fn screen_ray(
        &self,
        camera_transform: &Transform,
        screen_position: Vec2,
        viewport_size: Vec2,
    ) -> Option<Ray> {
        if viewport_size.x <= 0.0 || viewport_size.y <= 0.0 {
            return None;
        }

        let ndc_x = (screen_position.x / viewport_size.x) * 2.0 - 1.0;
        let ndc_y = 1.0 - (screen_position.y / viewport_size.y) * 2.0;
        let view_projection = self.projection * camera_transform.inverse_matrix();
        let inverse_view_projection = view_projection.try_inverse()?;

        let near = unproject(&inverse_view_projection, Vec4::new(ndc_x, ndc_y, 0.0, 1.0))?;
        let far = unproject(&inverse_view_projection, Vec4::new(ndc_x, ndc_y, 1.0, 1.0))?;
        let direction = (far - near).try_normalize(f32::EPSILON)?;
        Some(Ray {
            origin: camera_transform.position,
            direction,
        })
    }
}

/// Maps a clip-space point back to world space by perspective division.
fn unproject(inverse_view_projection: &Mat4, clip: Vec4) -> Option<Vec3> {
    let world = inverse_view_projection * clip;
    (world.w.abs() > f32::EPSILON).then(|| world.xyz() / world.w)
}

/// Trait for camera controllers that update from time and input.
pub trait CameraLike {
    /// Updates the camera/controller state for one frame.
    fn update(&mut self, time: &Time, input: &Input);
}
#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra_glm::{vec2, vec3};

    fn square_camera() -> Camera {
        // 90 degree horizontal and vertical field of view, so the screen corners sit at x = +-1, y = +-1.
        Camera::new(1.0, 90f32.to_radians(), 0.1, 1000.0)
    }

    #[test]
    fn screen_centre_ray_points_down_camera_forward() {
        let camera = square_camera();
        let transform = Transform::from_xyz(0.0, 0.0, 0.0);

        let ray = camera
            .screen_ray(&transform, vec2(100.0, 100.0), vec2(200.0, 200.0))
            .expect("centre ray should exist");

        assert!(ray.direction.x.abs() < 1e-5);
        assert!(ray.direction.y.abs() < 1e-5);
        assert!((ray.direction.z - 1.0).abs() < 1e-5);
    }

    #[test]
    fn top_left_corner_ray_points_to_upper_left_of_view() {
        let camera = square_camera();
        let transform = Transform::from_xyz(0.0, 0.0, 0.0);

        let ray = camera
            .screen_ray(&transform, vec2(0.0, 0.0), vec2(200.0, 200.0))
            .expect("corner ray should exist");

        let expected = vec3(-1.0, 1.0, 1.0).normalize();
        assert!((ray.direction - expected).norm() < 1e-5);
        assert!(ray.origin.norm() < 1e-5);
    }

    #[test]
    fn empty_viewport_has_no_ray() {
        let camera = square_camera();
        let transform = Transform::from_xyz(0.0, 0.0, 0.0);

        assert!(camera
            .screen_ray(&transform, vec2(0.0, 0.0), vec2(0.0, 200.0))
            .is_none());
    }
}
