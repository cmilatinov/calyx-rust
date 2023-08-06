use engine::*;
use egui::Ui;
use crate::panel::Panel;

pub struct PanelViewport;

impl Panel for PanelViewport {
    fn name() -> &'static str {
        "Viewport"
    }

    fn ui(&mut self, ui: &mut Ui) {
        // egui_wgpu::CallbackFn::new()
        ui.heading("test");
    }
}