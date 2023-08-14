use eframe::NativeOptions;

use editor::*;
use engine::*;
use engine::assets::AssetRegistry;
use engine::core::Time;
use engine::ecs::ComponentInfo;
use engine::type_registry::TypeRegistry;

fn main() -> eframe::Result<()> {
    Time::init();
    AssetRegistry::init();
    TypeRegistry::init();
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