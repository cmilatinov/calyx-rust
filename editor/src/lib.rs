mod camera;
mod panel;
mod inspector;
mod project_manager;

use std::collections::HashSet;
use eframe::egui;
use egui_dock::{DockArea, NodeIndex, Style, Tree};
use engine::*;
use engine::core::{OptionRef, Ref, Time};
use engine::render::SceneRenderer;
use engine::scene::Scene;
use engine::indextree::{NodeId};
use engine::uuid::Uuid;
use utils::{Init, singleton};
use crate::camera::EditorCamera;

use self::panel::*;
pub use self::project_manager::*;

pub struct EditorApp {
    fps: i32,
    tree: Tree<String>,
    panel_manager: PanelManager,
    pub scene_renderer: Ref<SceneRenderer>
}

#[derive(Default)]
pub struct EditorAppState {
    pub scene: Scene,
    pub camera: EditorCamera,
    pub scene_renderer: OptionRef<SceneRenderer>,
    pub selection: Option<EditorSelection>
}

#[derive(Clone, PartialEq, Debug)]
pub enum EditorSelection {
    Entity(HashSet<NodeId>),
    Asset(HashSet<Uuid>)
}

impl Init for EditorAppState {
    fn initialize(instance: &mut Self) {

    }
}

singleton!(EditorAppState);

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let mut tree = Tree::new(vec![
            PanelSceneHierarchy::name().to_owned(),
        ]);
        let [_, b] = tree.split_right(NodeIndex::root(), 0.2, vec![
            PanelViewport::name().to_owned(),
        ]);
        let [c, _] = tree.split_right(b, 0.75, vec![PanelInspector::name().to_owned()]);
        let [_, _] = tree.split_below(
            c, 0.7, vec![
                PanelContentBrowser::name().to_owned(),
                PanelTerminal::name().to_owned()
            ]
        );
        cc.egui_ctx.set_pixels_per_point(1.25);
        Self {
            fps: 0,
            tree,
            panel_manager: PanelManager::default(),
            scene_renderer: Ref::new(SceneRenderer::new(cc))
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
            println!("{} fps", self.fps);
            self.fps = 0;
            Time::reset_timer("fps");
        }

        ctx.request_repaint();
    }
}