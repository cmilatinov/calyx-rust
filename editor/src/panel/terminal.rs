use egui::{ScrollArea, Ui};
use std::any::Any;

use engine::core::LogRegistry;
use engine::egui;

use crate::panel::Panel;

#[derive(Default)]
pub struct PanelTerminal {
    // input: String,
    history: Vec<String>,
}

impl Panel for PanelTerminal {
    fn name() -> &'static str
    where
        Self: Sized,
    {
        "Console"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut scroll_area = ScrollArea::new([false, true]);
        scroll_area = scroll_area.stick_to_bottom(true);

        scroll_area.show(ui, |ui| {
            self.history
                .append(&mut LogRegistry::get_mut().drain_logs());
            for message in &self.history {
                ui.label(message);
            }
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
