use egui::Ui;

use engine::*;
use crate::EditorAppState;

use crate::panel::Panel;

pub struct PanelInspector;

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&self, ui: &mut Ui) {
        let app_state = EditorAppState::get();
    }
}