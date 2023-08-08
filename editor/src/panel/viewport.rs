use std::ops::Mul;
use std::sync::Arc;
use engine::*;
use egui::Ui;
use engine::core::time::Time;
use engine::glm::{Vec3, vec3};
use engine::render::Camera;
use crate::EditorAppResources;
use crate::panel::Panel;

#[derive(Default)]
pub struct PanelViewport {
    camera: Camera
}

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
            const ROTATION_SPEED: f64 = 150.0;
            let drag = res.drag_delta();
            self.camera.transform.rotate(
                &vec3(-drag.y, -drag.x, 0.0).scale(
                    (Time::static_delta_time() *
                        ROTATION_SPEED) as f32
                )
            );
        }

        let cb = egui_wgpu::CallbackFn::new()
            .prepare(move |device, queue, _encoder, paint_callback_resources| {
                let mut resources: &EditorAppResources = paint_callback_resources.get().unwrap();
                resources.scene_renderer.prepare(device, queue, &self.camera);
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