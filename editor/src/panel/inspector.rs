use engine::egui::Ui;
use crate::panel::Panel;

pub struct PanelInspector;

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut Ui) {
        // TODO: Reflect needs to be fully implemented
    }
}
