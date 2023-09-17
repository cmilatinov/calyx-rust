use egui::{ScrollArea, Ui};
use engine::core::LogRegistry;
use engine::egui;
use crate::panel::Panel;

pub struct PanelTerminal {
    // input: String,
    history: Vec<String>,
}

impl Default for PanelTerminal {
    fn default() -> Self {
        PanelTerminal {
            // input: String::new(),
            history: Vec::new(),
        }
    }
}

impl Panel for PanelTerminal {
    fn name() -> &'static str where Self: Sized {
        "Console"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut scroll_area = ScrollArea::new([false, true]);
        scroll_area = scroll_area.stick_to_bottom(true);

        scroll_area.show(ui, |ui| {
            self.history.append(&mut LogRegistry::get_mut().drain_logs());
            for message in &self.history {
                ui.label(message);
            }
        });
    }
}