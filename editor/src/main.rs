use eframe::NativeOptions;

use log::{LevelFilter};

use editor::*;
use engine::*;
use engine::assets::AssetRegistry;
use engine::core::{Logger, LogRegistry, Time};

use std::env;
use std::path::PathBuf;
use project::Project;

fn main() -> eframe::Result<()> {
    // LOAD PROJECT
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        println!("Expected 2 arguments, got {}", args.len());
        std::process::exit(1);
    }

    let _project = Project::load(PathBuf::from(&args[1])).expect("Unable to load project");

    // START ACTUAL EDITOR
    Time::init();
    AssetRegistry::init();
    LogRegistry::init();

    log::set_boxed_logger(Box::new(Logger)).expect("Unable to setup logger");
    log::set_max_level(LevelFilter::Debug);

    let options = NativeOptions {
        decorated: true,
        transparent: true,
        min_window_size: Some(egui::vec2(1280.0, 720.0)),
        initial_window_size: Some(egui::vec2(1280.0, 720.0)),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Calyx",
        options,
        Box::new(|cc| {
            let mut app_state = EditorAppState::get_mut();
            let app = EditorApp::new(cc);
            app_state.scene_renderer = Some(app.scene_renderer.clone());
            Box::new(app)
        })
    )
}