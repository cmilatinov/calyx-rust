use std::ops::Deref;
use engine::*;
use editor::*;
use eframe::NativeOptions;
use serde::Deserialize;
use assets_manager::{Asset, loader};
use engine::assets::AssetRegistry;

// The struct you want to load
#[derive(Deserialize)]
struct Point {
    x: i32,
    y: i32,
}

// Specify how you want the structure to be loaded
impl Asset for Point {
    // The extension of the files to look into
    const EXTENSION: &'static str = "ron";

    // The serialization format (RON)
    type Loader = loader::RonLoader;
}

fn main() -> eframe::Result<()> {
    AssetRegistry::init();

    {
        let instance = AssetRegistry::get();
        let handle = instance.load::<Point>("test").unwrap();
        let point = handle.read();
        println!("x: {}", point.x);
        println!("y: {}", point.y);
    }

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
        Box::new(|cc| Box::new(EditorApp::new(cc))),
    )
}