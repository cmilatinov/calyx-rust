use eframe::NativeOptions;

use log::{LevelFilter};

use editor::*;
use engine::*;
use engine::assets::AssetRegistry;
use engine::core::{Logger, LogRegistry, Time};

use std::env;
use std::path::PathBuf;
use reflect::registry::TypeRegistry;

fn main() -> eframe::Result<()> {
    // LOAD PROJECT
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        println!("Expected 2 arguments, got {}", args.len());
        std::process::exit(1);
    }

    // START ACTUAL EDITOR
    ProjectManager::init();
    {
        ProjectManager::get_mut().load(PathBuf::from(&args[1]));
    }

    Time::init();
    AssetRegistry::init();
    TypeRegistry::init();

    {
        let pm = ProjectManager::get();
        AssetRegistry::get_mut().add_assets_path(
            pm.current_project()
                .assets_directory()
                .into_os_string()
                .into_string()
                .unwrap()
        );
    }

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