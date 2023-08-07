use std::sync::Arc;
use engine::*;
use egui::Ui;
use crate::EditorAppResources;
use crate::panel::Panel;

#[derive(Default)]
pub struct PanelViewport {
    angle: f32
}

impl Panel for PanelViewport {
    fn name() -> &'static str {
        "Viewport"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let (rect, res) =
            ui.allocate_exact_size(
                egui::Vec2::new(ui.available_width(), ui.available_height()),
                egui::Sense::drag()
            );
        self.angle += res.drag_delta().x * 0.01;
        let angle = self.angle;

        let cb = egui_wgpu::CallbackFn::new()
            .prepare(move |device, queue, _encoder, paint_callback_resources| {
                let resources: &EditorAppResources = paint_callback_resources.get().unwrap();
                resources.scene_renderer.prepare(device, queue, angle);
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