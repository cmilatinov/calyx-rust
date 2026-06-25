use egui::text::LayoutJob;
use egui::{Color32, FontId, Margin, TextFormat, TextStyle, Ui};
use log::LevelFilter;
use std::any::Any;

use crate::panel::Panel;
use crate::EditorAppState;

const TOOLBAR_MARGIN_X: i8 = 10;
const TOOLBAR_MARGIN_TOP: i8 = 6;
const TOOLBAR_MARGIN_BOTTOM: i8 = 2;
const LOG_TOP_PADDING: f32 = 6.0;
const LOG_BOTTOM_PADDING: f32 = 32.0;
const LOG_BACKGROUND: Color32 = Color32::from_rgb(3, 4, 4);
const LOG_TEXT_COLOR: Color32 = Color32::from_rgb(205, 205, 205);

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
        let item_spacing_y = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;

        egui::Frame::NONE
            .inner_margin(Margin {
                left: TOOLBAR_MARGIN_X,
                right: TOOLBAR_MARGIN_X,
                top: TOOLBAR_MARGIN_TOP,
                bottom: TOOLBAR_MARGIN_BOTTOM,
            })
            .show(ui, |ui| {
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
            });
        ui.separator();

        let filter = self.filter.to_ascii_lowercase();
        egui::Frame::NONE.fill(LOG_BACKGROUND).show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            egui::ScrollArea::vertical()
                .id_salt("console_log_scroll")
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .stick_to_bottom(self.stick_to_bottom)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = item_spacing_y;
                    ui.add_space(LOG_TOP_PADDING);

                    for entry in engine::logging::recent_log_entries() {
                        if !Self::passes_level(entry.level, self.min_level) {
                            continue;
                        }
                        let line = entry.line();
                        if !filter.is_empty() && !line.to_ascii_lowercase().contains(&filter) {
                            continue;
                        }
                        ui.label(Self::entry_text(ui, &entry));
                    }
                    ui.add_space(LOG_BOTTOM_PADDING);
                });
        });

        ui.spacing_mut().item_spacing.y = item_spacing_y;
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
            log::Level::Info => Color32::from_rgb(0, 220, 0),
            log::Level::Debug => Color32::from_rgb(143, 190, 255),
            log::Level::Trace => Color32::from_gray(150),
        }
    }

    fn entry_text(ui: &Ui, entry: &engine::logging::LogEntry) -> LayoutJob {
        let font_id = TextStyle::Monospace.resolve(ui.style());
        let level_color = Self::level_color(entry.level);
        let mut job = LayoutJob::default();

        job.append(
            &format!("{} ", entry.timestamp),
            0.0,
            Self::text_format(font_id.clone(), LOG_TEXT_COLOR),
        );
        job.append(
            &format!("{:<5}", entry.level),
            0.0,
            Self::text_format(font_id.clone(), level_color),
        );
        job.append(
            &format!(" {} - {}", entry.target, entry.message),
            0.0,
            Self::text_format(font_id, LOG_TEXT_COLOR),
        );

        job
    }

    fn text_format(font_id: FontId, color: Color32) -> TextFormat {
        TextFormat {
            font_id,
            color,
            ..Default::default()
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
