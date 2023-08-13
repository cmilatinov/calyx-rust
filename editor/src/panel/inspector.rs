use egui::Ui;

use engine::*;

use crate::panel::Panel;

pub struct PanelInspector;

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&self, ui: &mut Ui) {
        ui.heading("Inspector");
    }
}