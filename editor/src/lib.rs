use std::sync::{Arc, RwLock};
use eframe::egui;
use egui_dock::{DockArea, NodeIndex, Style, Tree};
use engine::*;
use engine::core::Time;
use engine::render::SceneRenderer;
use engine::scene::Scene;
use engine::utils::Init;
use crate::camera::EditorCamera;

use self::panel::*;

mod camera;
mod panel;

pub struct EditorApp {
    fps: i32,
    tree: Tree<String>,
    panel_manager: PanelManager,
    pub scene_renderer: Arc<RwLock<SceneRenderer>>
}

#[derive(Default)]
pub struct EditorAppState {
    pub scene: Scene,
    pub camera: EditorCamera,
    pub scene_renderer: Option<Arc<RwLock<SceneRenderer>>>
}

singleton_with_init!(EditorAppState);

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let mut tree = Tree::new(vec![
            PanelSceneHierarchy::name().to_owned(),
        ]);
        let [_, b] = tree.split_right(NodeIndex::root(), 0.2, vec![
            PanelViewport::name().to_owned(),
        ]);
        let [c, _] = tree.split_right(b, 0.8, vec![PanelInspector::name().to_owned()]);
        let [_, _] = tree.split_below(c, 0.7, vec![PanelContentBrowser::name().to_owned()]);

        Self {
            fps: 0,
            tree,
            panel_manager: PanelManager::default(),
            scene_renderer: Arc::new(RwLock::new(SceneRenderer::new(cc)))
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        Time::update_time();

        {
            let app_state = EditorAppState::get();
            let render_state = frame.wgpu_render_state().unwrap();
            self.scene_renderer.read().unwrap()
                .render_scene(
                    render_state,
                    &app_state.camera.transform,
                    &app_state.camera.camera,
                    &app_state.scene
                );
        }

        DockArea::new(&mut self.tree)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show_close_buttons(false)
            .show(ctx, &mut self.panel_manager);

        self.fps += 1;
        if Time::timer("fps") >= 1.0 {
            println!("{}", self.fps);
            self.fps = 0;
            Time::reset_timer("fps");
        }

        ctx.request_repaint();
    }
}