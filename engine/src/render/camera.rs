use glm::Mat4;

pub struct Camera {
    pub projection: Mat4
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            projection: Mat4::identity()
        }
    }
}

pub trait CameraLike {
    fn update(&mut self, _ui: &mut egui::Ui, _res: &egui::Response) {}
}
