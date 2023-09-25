mod camera;
mod inspector;
mod panel;
mod project_manager;
mod task_id;
mod selection;

use crate::camera::EditorCamera;
use eframe::egui;
use egui_dock::{DockArea, NodeIndex, Style, Tree};
use egui_gizmo::GizmoMode;
use num_traits::FromPrimitive;
use engine::core::{OptionRef, Ref, Time};
use engine::render::SceneRenderer;
use engine::scene::Scene;
use engine::*;
use engine::background::Background;
use engine::egui::{Align, Layout};
use selection::EditorSelection;
use utils::{Init, singleton};
use crate::task_id::TaskId;

use self::panel::*;
pub use self::project_manager::*;

pub struct EditorApp {
    count: i32,
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
            count: 0,
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

        self.menu_bar(ctx);

        DockArea::new(&mut self.tree)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show_close_buttons(false)
            .show(ctx, &mut self.panel_manager);

        self.status_bar(ctx);

        self.fps += 1;
        if Time::timer("fps") >= 1.0 {
            println!("{} fps", self.fps);
            self.fps = 0;
            Time::reset_timer("fps");
        }
        if Time::timer("count") >= 0.5 {
            self.count = (self.count + 1) % 3;
            Time::reset_timer("count");
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

impl EditorApp {
    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {}
                    if ui.button("Open").clicked() {}
                    if ui.button("Save").clicked() {}
                    if ui.button("Save As").clicked() {}
                });
                ui.menu_button("Build", |ui| {
                    if ui.button("Clean").clicked() {}
                    if ui.button("Build").clicked() {}
                    if ui.button("Rebuild").clicked() {}
                });
            });
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let background = Background::get();
                    let task_list = background.task_list();
                    if !task_list.is_empty() {
                        ui.add(egui::Spinner::new().size(15.0));
                    }
                    match task_list.len() {
                        0 => {},
                        1 => {
                            let id = task_list.iter().next().unwrap();
                            if let Some(task_id) = TaskId::from_isize(*id) {
                                ui.label(task_id.message());
                            }
                        }
                        len => {
                            ui.label(format!("{} tasks", len));
                        }
                    }
                });
            });
        });
    }
}