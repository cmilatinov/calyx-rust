use std::process::Command;
use std::env;

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
    eframe::run_native("Launcher App", native_options, Box::new(|cc| Box::new(LauncherApp::new(cc))));
}

pub fn launch_editor() {
    let args: Vec<String> = env::args().collect();

    // Set up the necessary environment variable or argument
    // Here I'm setting an environment variable, but you can choose other methods
    env::set_var("LAUNCHED_THROUGH_LAUNCHER", "true");

    // Now, execute the editor with the provided path
    let status = Command::new("path_to_editor_executable")
        .arg(&args[1])
        .status()
        .expect("failed to start editor");

    if !status.success() {
        eprintln!("Editor terminated with an error.");
    }
}
