use egui::Ui;
use crate::panel::Panel;

pub struct PanelInspector;

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut Ui) {
        use egui::special_emojis::{OS_APPLE, OS_LINUX, OS_WINDOWS};

        ui.heading("egui");
        ui.label(format!(
            "egui is an immediate mode GUI library written in Rust. egui runs both on the web and natively on {}{}{}. \
            On the web it is compiled to WebAssembly and rendered with WebGL.{}",
            OS_APPLE, OS_LINUX, OS_WINDOWS,
            if cfg!(target_arch = "wasm32") {
                " Everything you see is rendered as textured triangles. There is no DOM, HTML, JS or CSS. Just Rust."
            } else {""}
        ));
        ui.label("egui is designed to be easy to use, portable, and fast.");

        ui.add_space(12.0);
        ui.heading("Immediate mode");

        ui.add_space(12.0);
        ui.heading("Links");
    }
}