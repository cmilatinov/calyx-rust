use glm::Mat4;

use crate::math;

pub struct Camera {
    pub projection: Mat4,
    pub aspect: f32,
    pub fov_x: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera::new(16.0 / 9.0, 70f32.to_radians(), 0.1, 1000.0)
    }
}

impl Camera {
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

    pub fn update_projection(&mut self) {
        self.projection = glm::perspective_lh::<f32>(
            self.aspect,
            math::to_fov_y(self.aspect, self.fov_x),
            self.near_plane,
            self.far_plane,
        );
    }
}

pub trait CameraLike {
    fn update(&mut self, _ui: &mut egui::Ui, _res: &egui::Response) {}
}
