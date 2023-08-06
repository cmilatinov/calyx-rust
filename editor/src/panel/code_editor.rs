use egui::{Align, Ui, Widget};
use crate::panel::Panel;

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct PanelCodeEditor {
    language: String,
    code: String,
}

impl Default for PanelCodeEditor {
    fn default() -> Self {
        Self {
            language: "rs".into(),
            code: "// A very simple example\n\
fn main() {\n\
\tprintln!(\"Hello world!\");\n\
}\n\
"
                .into(),
        }
    }
}

impl Panel for PanelCodeEditor {
    fn name() -> &'static str where Self: Sized {
        "Code Editor"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let Self { language, code } = self;

        let theme = crate::syntax_highlighting::CodeTheme::from_memory(ui.ctx());

        let mut layouter = |ui: &egui::Ui, string: &str, wrap_width: f32| {
            let mut layout_job =
                crate::syntax_highlighting::highlight(ui.ctx(), &theme, string, language);
            layout_job.wrap.max_width = wrap_width;
            ui.fonts(|f| f.layout_job(layout_job))
        };

        ui.with_layout(egui::Layout::left_to_right(Align::Center).with_cross_justify(true), |ui| {
            egui::TextEdit::multiline(code)
                .font(egui::TextStyle::Monospace)
                .code_editor()
                .lock_focus(true)
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter)
                .ui(ui);
        });
    }
}
