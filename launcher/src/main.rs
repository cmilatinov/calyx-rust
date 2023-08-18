mod app;
use app::LauncherApp;

fn main() {
    let native_options = eframe::NativeOptions {
        decorated: true,
        transparent: true,
        min_window_size: Some(egui::vec2(500.0, 400.0)),
        initial_window_size: Some(egui::vec2(500.0, 400.0)),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("Launcher App", native_options, Box::new(|cc| Box::new(LauncherApp::new(cc))))
        .expect("Unable to open application.");
}
