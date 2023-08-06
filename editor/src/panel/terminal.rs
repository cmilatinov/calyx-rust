use egui::{Id, ScrollArea, Ui};
use crate::panel::Panel;

pub struct PanelTerminal {
    input: String,
    history: Vec<String>,
}

impl Default for PanelTerminal {
    fn default() -> Self {
        PanelTerminal {
            input: String::new(),
            history: Vec::new(),
        }
    }
}

impl Panel for PanelTerminal {
    fn name() -> &'static str where Self: Sized {
        "Console"
    }

    fn ui(&mut self, ui: &mut Ui) {
        // Display history
        ScrollArea::vertical().show(ui, |ui| {
            for message in &self.history {
                ui.label(message);
            }
        });
    }
}