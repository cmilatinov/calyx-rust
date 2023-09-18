use eframe::NativeOptions;
use std::default::Default;

use log::LevelFilter;

use editor::*;
use engine::assets::AssetRegistry;
use engine::core::{LogRegistry, Logger, Time};
use engine::*;

use engine::class_registry::ClassRegistry;
use engine::eframe::wgpu;
use reflect::type_registry::TypeRegistry;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

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

    {
        AssetRegistry::get_mut().set_root(ProjectManager::get().current_project().root_directory().clone());
    }

    TypeRegistry::init();
    ClassRegistry::init();
    LogRegistry::init();

    log::set_boxed_logger(Box::new(Logger)).expect("Unable to setup logger");
    log::set_max_level(LevelFilter::Debug);

    let options = NativeOptions {
        maximized: true,
        decorated: true,
        transparent: true,
        min_window_size: Some(egui::vec2(1280.0, 720.0)),
        initial_window_size: Some(egui::vec2(1280.0, 720.0)),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            device_descriptor: Arc::new(|_adapter| wgpu::DeviceDescriptor {
                label: Some("Beans"),
                features: wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "Calyx",
        options,
        Box::new(|cc| {
            let mut app_state = EditorAppState::get_mut();
            let app = EditorApp::new(cc);
            app_state.viewport_width = 0.0;
            app_state.viewport_height = 0.0;
            app_state.scene_renderer = Some(app.scene_renderer.clone());
            Box::new(app)
        }),
    )
}
