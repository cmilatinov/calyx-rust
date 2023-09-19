mod camera;
mod inspector;
mod panel;
mod project_manager;
mod selection;

use crate::camera::EditorCamera;
use eframe::egui;
use egui_dock::{DockArea, NodeIndex, Style, Tree};
use egui_gizmo::GizmoMode;
use engine::core::{OptionRef, Ref, Time};
use engine::render::SceneRenderer;
use engine::scene::Scene;
use engine::*;
use selection::EditorSelection;
use utils::{singleton, Init};

use self::panel::*;
pub use self::project_manager::*;

pub struct EditorApp {
    fps: i32,
    tree: Tree<String>,
    panel_manager: PanelManager,
    pub scene_renderer: Ref<SceneRenderer>,
}

pub struct EditorAppState {
    pub scene: Scene,
    pub camera: EditorCamera,
    pub scene_renderer: OptionRef<SceneRenderer>,
    pub selection: Option<EditorSelection>,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub gizmo_mode: GizmoMode,
}

impl Default for EditorAppState {
    fn default() -> Self {
        Self {
            scene: Scene::default(),
            camera: EditorCamera::default(),
            scene_renderer: None,
            selection: None,
            viewport_width: 0.0,
            viewport_height: 0.0,
            gizmo_mode: GizmoMode::Translate,
        }
    }
}

impl Init for EditorAppState {
    fn initialize(&mut self) {}
}

singleton!(EditorAppState);

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let mut tree = Tree::new(vec![PanelSceneHierarchy::name().to_owned()]);
        let [_, b] = tree.split_right(
            NodeIndex::root(),
            0.2,
            vec![PanelViewport::name().to_owned()],
        );
        let [c, _] = tree.split_right(b, 0.7, vec![PanelInspector::name().to_owned()]);
        let [_, _] = tree.split_below(
            c,
            0.7,
            vec![
                PanelContentBrowser::name().to_owned(),
                PanelTerminal::name().to_owned(),
            ],
        );
        cc.egui_ctx.set_pixels_per_point(1.25);
        Self {
            fps: 0,
            tree,
            panel_manager: PanelManager::default(),
            scene_renderer: Ref::new(SceneRenderer::new(cc)),
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        Time::update_time();

        {
            let mut app_state = EditorAppState::get_mut();
            app_state.scene.clear_transform_cache();
            let render_state = frame.wgpu_render_state().unwrap();
            let mut renderer = self.scene_renderer.write().unwrap();
            let (width, height) = EditorApp::get_physical_size(
                ctx,
                frame,
                app_state.viewport_width,
                app_state.viewport_height,
            );
            if width != 0 && height != 0 {
                renderer.resize_textures(ctx, render_state, width, height);
                app_state.camera.camera.aspect = width as f32 / height as f32;
            }
            app_state.camera.camera.update_projection();
            renderer.render_scene(
                render_state,
                &app_state.camera.camera,
                &app_state.camera.transform,
                &app_state.scene,
            );
        }

        DockArea::new(&mut self.tree)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show_close_buttons(false)
            .show(ctx, &mut self.panel_manager);

        self.fps += 1;
        if Time::timer("fps") >= 1.0 {
            println!("{} fps", self.fps);
            self.fps = 0;
            Time::reset_timer("fps");
        }

        ctx.request_repaint();
    }
}

impl EditorApp {
    fn get_physical_size(
        ctx: &egui::Context,
        frame: &eframe::Frame,
        viewport_width: f32,
        viewport_height: f32,
    ) -> (u32, u32) {
        let window_size = frame.info().window_info.size;
        let pixels_per_point = ctx.pixels_per_point();
        let width = window_size.x * viewport_width * pixels_per_point;
        let height = window_size.y * viewport_height * pixels_per_point;
        (width as u32, height as u32)
    }
}
