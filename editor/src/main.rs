use engine::*;
use editor::*;
use eframe::NativeOptions;
use engine::core::time::Time;
use engine::assets::AssetRegistry;

fn main() -> eframe::Result<()> {
    Time::init();
    AssetRegistry::init();
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
            let mut app_state = EditorAppState::get();
            let app = EditorApp::new(cc);
            app_state.scene_renderer = Some(app.scene_renderer.clone());
            Box::new(app)
        })
    )
}