use eframe::egui;
use egui_dock::{DockArea, NodeIndex, Style};
use egui_gizmo::GizmoMode;
use num_traits::FromPrimitive;

use engine::background::Background;
use engine::core::{OptionRef, Ref, Time};
use engine::egui::FontFamily;
use engine::egui::TextStyle;
use engine::egui::{Align, FontId, Layout};
use engine::egui_dock::DockState;
use engine::render::{RenderContext, SceneRenderer};
use engine::scene::Scene;
use engine::*;
use selection::EditorSelection;
use utils::singleton_with_init;

use crate::camera::EditorCamera;
use crate::task_id::TaskId;

use self::panel::*;
pub use self::project_manager::*;

mod camera;
mod inspector;
mod panel;
mod project_manager;
mod selection;
mod task_id;
mod widgets;

pub const BASE_FONT_SIZE: f32 = 16.0;

pub struct EditorApp {
    fps_counter: i32,
    fps: i32,
    dock_state: DockState<String>,
    dock_style: Style,
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

singleton_with_init!(EditorAppState);

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let mut dock_style = Style::from_egui(&cc.egui_ctx.style());
        dock_style.tab_bar.bg_fill = egui::Color32::from_rgb(18, 18, 18);
        let mut dock_state = DockState::new(vec![PanelSceneHierarchy::name().to_owned()]);
        let surface = dock_state.main_surface_mut();
        let [_, b] = surface.split_right(
            NodeIndex::root(),
            0.2,
            vec![PanelViewport::name().to_owned()],
        );
        let [c, _] = surface.split_right(b, 0.7, vec![PanelInspector::name().to_owned()]);
        let [_, _] = surface.split_below(
            c,
            0.7,
            vec![
                PanelContentBrowser::name().to_owned(),
                PanelTerminal::name().to_owned(),
            ],
        );
        Self::configure_styles(&cc.egui_ctx);
        RenderContext::get_mut().init_from_eframe(cc);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self {
            fps: 0,
            fps_counter: 0,
            dock_state,
            dock_style,
            panel_manager: PanelManager::default(),
            scene_renderer: Ref::new(SceneRenderer::new(cc)),
        }
    }
}

impl Drop for EditorApp {
    fn drop(&mut self) {
        RenderContext::get_mut().destroy();
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

        egui::CentralPanel::default().show(ctx, |_| {
            DockArea::new(&mut self.dock_state)
                .style(self.dock_style.clone())
                .show_close_buttons(false)
                .show(ctx, &mut self.panel_manager);
        });

        self.status_bar(ctx);

        self.fps_counter += 1;
        if Time::timer("fps") >= 1.0 {
            self.fps = self.fps_counter;
            self.fps_counter = 0;
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

    fn configure_styles(ctx: &egui::Context) {
        // ctx.set_pixels_per_point(1.25);
        let mut style = (*ctx.style()).clone();
        style.spacing.scroll_bar_width = 5.0;
        style.spacing.scroll_bar_inner_margin = 0.0;
        style.spacing.scroll_bar_outer_margin = 2.0;
        style.visuals.indent_has_left_vline = false;
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(BASE_FONT_SIZE * 5.0 / 7.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(BASE_FONT_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(BASE_FONT_SIZE, FontFamily::Monospace),
            ),
            (
                TextStyle::Button,
                FontId::new(BASE_FONT_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(BASE_FONT_SIZE * 10.0 / 7.0, FontFamily::Proportional),
            ),
        ]
        .into();
        ctx.set_style(style);
    }
}

impl EditorApp {
    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Open").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Save").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Save As").clicked() {
                        ui.close_menu();
                    }
                });
                ui.menu_button("Build", |ui| {
                    if ui.button("Clean").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Build").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Rebuild").clicked() {
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom_panel")
            .exact_height(24.0)
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.fps > 0 {
                            ui.label(format!("{}", self.fps));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let background = Background::get();
                            let task_list = background.task_list();
                            if !task_list.is_empty() {
                                ui.add(egui::Spinner::new().size(15.0));
                            }
                            match task_list.len() {
                                0 => {}
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
            });
    }
}
