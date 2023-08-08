use std::sync::Arc;
use engine::*;
use egui::Ui;
use engine::core::time::Time;
use engine::glm::{Mat4, vec3};
use engine::render::{Camera};
use crate::EditorAppResources;
use crate::panel::Panel;

#[derive(Default)]
pub struct PanelViewport {
    camera: Camera
}

pub const OPENGL_TO_WGPU_MATRIX: Mat4 = Mat4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.5,
    0.0, 0.0, 0.0, 1.0,
);

impl Panel for PanelViewport {
    fn name() -> &'static str {
        "Viewport"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let (rect, res) =
            ui.allocate_exact_size(
                egui::Vec2::new(ui.available_width(), ui.available_height()),
                egui::Sense::click_and_drag()
            );
        if res.dragged() {
            const ROTATION_SPEED: f64 = 1.0;
            let drag = res.drag_delta();
            self.camera.transform.rotate(
                &vec3(-drag.y, -drag.x, 0.0).scale(
                    (Time::static_delta_time() *
                        ROTATION_SPEED) as f32
                )
            );
            println!("{:?}", drag);
        }

        let proj = glm::perspective_lh(rect.width() / rect.height(), 45.0f32.to_radians(), 0.1, 100.0);
        let view = self.camera.transform.get_matrix();

        let cb = egui_wgpu::CallbackFn::new()
            .prepare(move |device, queue, _encoder, paint_callback_resources| {
                let resources: &mut EditorAppResources = paint_callback_resources.get_mut().unwrap();
                resources.scene_renderer.camera.projection.clone_from_slice(&proj.data.0);
                resources.scene_renderer.camera.view.clone_from_slice(&view.data.0);
                resources.scene_renderer.camera.model.clone_from_slice(&Mat4::identity().data.0);
                resources.scene_renderer.prepare(device, queue);
                Vec::new()
            })
            .paint(move |_info, render_pass, paint_callback_resources| {
                let resources: &EditorAppResources = paint_callback_resources.get().unwrap();
                resources.scene_renderer.paint(render_pass);
            });

        let callback = egui::PaintCallback {
            rect,
            callback: Arc::new(cb),
        };

        ui.painter().add(callback);
    }
}