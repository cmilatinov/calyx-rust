use app::LauncherApp;

mod app;

fn main() {
    let native_options = eframe::NativeOptions {
        decorated: true,
        transparent: true,
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Calyx Launcher",
        native_options,
        Box::new(|cc| Box::new(LauncherApp::new(cc))),
    )
    .expect("Unable to open application.");
}
