use egui::{Align, Key, ScrollArea, SidePanel, Ui};
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

        egui::TextEdit::singleline(&mut self.input).show(ui);

        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            self.history.push(self.input.clone());
            self.input.clear();
        }

        // // Command input
        // if ui.horizontal(|ui| {
        //     ui.text_edit_singleline(&mut self.input);
        //     if ui.button("Submit").clicked() || ui.input(|i| i.raw.key_down(Key::Enter)).unwrap_or_default() {
        //         self.history.push(self.input.clone());
        //         self.input.clear();
        //     }
        // }).anything_gained_focus()
        // {
        //     // If the user clicked on the input or pressed Enter, scroll to the bottom to show the latest input
        //     ui.scroll_to_cursor(Some(Align::BOTTOM));
        // }
    }
}