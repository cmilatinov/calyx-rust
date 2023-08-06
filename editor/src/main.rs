use eframe::NativeOptions;
use editor::*;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        decorated: true,
        transparent: true,
        min_window_size: Some(egui::vec2(1280.0, 720.0)),
        initial_window_size: Some(egui::vec2(1280.0, 720.0)),
        ..Default::default()
    };
    eframe::run_native(
        "Calyx",
        options,
        Box::new(|_cc| Box::<Editor>::default()),
    )
}