use glm::Mat4;

pub struct Camera {
    pub projection: Mat4
}

pub trait CameraLike {
    fn update(&mut self, _ui: &mut egui::Ui, _res: &egui::Response) {}
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            projection: Mat4::identity()
        }
    }
}
