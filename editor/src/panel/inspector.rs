use egui::Ui;

use engine::*;

use crate::panel::Panel;

pub struct PanelInspector;

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut Ui) {
        ui.heading("Inspector");
    }
}