use egui::{Color32, RichText, Ui};
use log::LevelFilter;
use std::any::Any;

use crate::panel::Panel;
use crate::EditorAppState;

pub struct PanelTerminal {
    min_level: LevelFilter,
    filter: String,
    stick_to_bottom: bool,
}

impl Default for PanelTerminal {
    fn default() -> Self {
        Self {
            min_level: LevelFilter::Info,
            filter: String::new(),
            stick_to_bottom: true,
        }
    }
}

impl Panel for PanelTerminal {
    fn name() -> &'static str
    where
        Self: Sized,
    {
        "Console"
    }

    fn ui(&mut self, ui: &mut Ui, _state: &mut EditorAppState) {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("console_min_level")
                .selected_text(Self::level_filter_label(self.min_level))
                .show_ui(ui, |ui| {
                    for level in [
                        LevelFilter::Error,
                        LevelFilter::Warn,
                        LevelFilter::Info,
                        LevelFilter::Debug,
                        LevelFilter::Trace,
                    ] {
                        ui.selectable_value(
                            &mut self.min_level,
                            level,
                            Self::level_filter_label(level),
                        );
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter")
                    .desired_width(180.0),
            );
            ui.checkbox(&mut self.stick_to_bottom, "Auto-scroll");
        });
        ui.separator();

        let filter = self.filter.to_ascii_lowercase();
        egui::ScrollArea::vertical()
            .stick_to_bottom(self.stick_to_bottom)
            .show(ui, |ui| {
                for entry in engine::logging::recent_log_entries() {
                    if !Self::passes_level(entry.level, self.min_level) {
                        continue;
                    }
                    let line = entry.line();
                    if !filter.is_empty() && !line.to_ascii_lowercase().contains(&filter) {
                        continue;
                    }
                    ui.label(
                        RichText::new(line)
                            .monospace()
                            .color(Self::level_color(entry.level)),
                    );
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

impl PanelTerminal {
    fn passes_level(level: log::Level, min_level: LevelFilter) -> bool {
        level.to_level_filter() <= min_level
    }

    fn level_filter_label(level: LevelFilter) -> &'static str {
        match level {
            LevelFilter::Error => "Error",
            LevelFilter::Warn => "Warn",
            LevelFilter::Info => "Info",
            LevelFilter::Debug => "Debug",
            LevelFilter::Trace => "Trace",
            LevelFilter::Off => "Off",
        }
    }

    fn level_color(level: log::Level) -> Color32 {
        match level {
            log::Level::Error => Color32::from_rgb(255, 92, 92),
            log::Level::Warn => Color32::from_rgb(232, 178, 80),
            log::Level::Info => Color32::from_rgb(185, 214, 180),
            log::Level::Debug => Color32::from_rgb(143, 190, 255),
            log::Level::Trace => Color32::from_gray(150),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PanelTerminal;
    use log::{Level, LevelFilter};

    #[test]
    fn level_filter_includes_more_severe_entries() {
        assert!(PanelTerminal::passes_level(Level::Error, LevelFilter::Warn));
        assert!(PanelTerminal::passes_level(Level::Warn, LevelFilter::Warn));
        assert!(!PanelTerminal::passes_level(Level::Info, LevelFilter::Warn));
    }
}
