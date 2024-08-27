use std::env;
use std::io::BufWriter;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use inspector::inspector_registry::InspectorRegistry;
use num_traits::FromPrimitive;
use transform_gizmo_egui::{GizmoMode, GizmoOrientation};

use crate::camera::EditorCamera;
use crate::task_id::TaskId;
use engine::assets::AssetRegistry;
use engine::background::Background;
use engine::class_registry::ClassRegistry;
use engine::core::{LogRegistry, Logger, Ref, Time};
use engine::eframe::{wgpu, NativeOptions};
use engine::egui::{include_image, Button, Rounding, Sense, Vec2};
use engine::egui::{Align, Layout};
use engine::egui::{Color32, Frame, Margin, Shadow};
use engine::egui_tiles::{Container, Linear, LinearDir, Tiles, Tree};
use engine::input::{Input, InputState};
use engine::log::LevelFilter;
use engine::rapier3d::prelude::DebugRenderPipeline;
use engine::reflect::type_registry::TypeRegistry;
use engine::render::{Camera, RenderContext, SceneRenderer, SceneRendererOptions};
use engine::scene::SceneManager;
use engine::transform_gizmo_egui::EnumSet;
use engine::*;
use selection::EditorSelection;
use utils::singleton_with_init;
use winit::platform::windows::EventLoopBuilderExtWindows;

use self::panel::*;
pub use self::project_manager::*;

mod camera;
mod icons;
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
    tree: Tree<&'static str>,
    panel_manager: PanelManager,
    physics_debug_pipeline: DebugRenderPipeline,
    pub scene_renderer: Ref<SceneRenderer>,
    pub game_renderer: Ref<SceneRenderer>,
}

pub struct EditorAppState {
    pub camera: EditorCamera,
    pub scene_renderer: Option<Ref<SceneRenderer>>,
    pub game_renderer: Option<Ref<SceneRenderer>>,
    pub game_aspect: Option<(u32, u32)>,
    pub selection: EditorSelection,
    pub viewport_size: (f32, f32),
    pub game_response: Option<egui::Response>,
    pub game_size: (f32, f32),
    pub gizmo_modes: EnumSet<GizmoMode>,
    pub gizmo_orientation: GizmoOrientation,
}

impl Default for EditorAppState {
    fn default() -> Self {
        Self {
            camera: Default::default(),
            scene_renderer: Default::default(),
            game_renderer: Default::default(),
            game_aspect: None,
            selection: Default::default(),
            viewport_size: Default::default(),
            game_size: Default::default(),
            game_response: Default::default(),
            gizmo_modes: GizmoMode::all_translate(),
            gizmo_orientation: GizmoOrientation::Global,
        }
    }
}

singleton_with_init!(EditorAppState);

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let tree = Self::create_tree();
        RenderContext::get_mut().init_from_eframe(cc);
        re_ui::apply_style_and_install_loaders(&cc.egui_ctx);
        Self {
            fps: 0,
            fps_counter: 0,
            tree,
            panel_manager: PanelManager::default(),
            physics_debug_pipeline: DebugRenderPipeline::new(
                Default::default(),
                Default::default(),
            ),
            scene_renderer: Ref::new(SceneRenderer::new(SceneRendererOptions {
                grid: true,
                gizmos: true,
                samples: 1,
                clear_color: Color32::from_rgb(8, 8, 8),
            })),
            game_renderer: Ref::new(SceneRenderer::new(SceneRendererOptions {
                clear_color: Color32::from_rgb(0, 0, 0),
                ..Default::default()
            })),
        }
    }

    fn create_tree() -> Tree<&'static str> {
        let mut tiles = Tiles::default();

        let scene_hierarchy = tiles.insert_pane(PanelSceneHierarchy::name());
        let viewport = tiles.insert_pane(PanelViewport::name());
        let game = tiles.insert_pane(PanelGame::name());
        let inspector = tiles.insert_pane(PanelInspector::name());
        let content_browser = tiles.insert_pane(PanelContentBrowser::name());
        let terminal = tiles.insert_pane(PanelTerminal::name());

        let center = tiles.insert_tab_tile(vec![viewport, game]);
        let bottom = tiles.insert_tab_tile(vec![content_browser, terminal]);

        let mut middle_linear = Linear::new(LinearDir::Vertical, vec![center, bottom]);
        middle_linear.shares.set_share(center, 0.75);
        middle_linear.shares.set_share(bottom, 0.25);
        let middle = tiles.insert_container(Container::Linear(middle_linear));

        let left = scene_hierarchy;
        let right = inspector;

        let mut root_linear = Linear::new(LinearDir::Horizontal, vec![left, middle, right]);
        root_linear.shares.set_share(left, 0.25);
        root_linear.shares.set_share(middle, 0.75);
        root_linear.shares.set_share(right, 0.25);
        let root = tiles.insert_container(Container::Linear(root_linear));
        Tree::new("Calyx Editor", root, tiles)
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
            app_state.game_response = None;
            SceneManager::get()
                .simulation_scene()
                .clear_transform_cache();
            let render_state = frame.wgpu_render_state().unwrap();
            let mut renderer = self.scene_renderer.write();
            let (width, height) = EditorApp::get_physical_size(ctx, app_state.viewport_size);
            if width != 0 && height != 0 {
                renderer.resize_textures(width, height);
                app_state.camera.camera.aspect = width as f32 / height as f32;
            }
            app_state.camera.camera.update_projection();
            let scene_manager = SceneManager::get();
            let scene = scene_manager.simulation_scene();
            renderer.render_scene(
                render_state,
                &app_state.camera.camera,
                &app_state.camera.transform,
                scene,
                Some(&mut self.physics_debug_pipeline),
            );
            if let Some((node, c)) = scene.get_main_camera(&scene.world) {
                let mut renderer = self.game_renderer.write();
                {
                    let options = renderer.options_mut();
                    options.clear_color = c.clear_color;
                }
                let (width, height) = EditorApp::get_physical_size(ctx, app_state.game_size);
                if width != 0 && height != 0 {
                    renderer.resize_textures(width, height);
                }
                let transform = scene.get_world_transform(node);
                let mut camera = Camera::new(
                    width as f32 / height as f32,
                    c.fov,
                    c.near_plane,
                    c.far_plane,
                );
                camera.update_projection();
                renderer.render_scene(render_state, &camera, &transform, scene, None)
            } else {
                let device = &render_state.device;
                let queue = &render_state.queue;
                let renderer = self.game_renderer.read();
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                encoder.clear_texture(&renderer.scene_texture().texture, &Default::default());
                queue.submit(Some(encoder.finish()));
            }
        }

        self.menu_bar(ctx);

        egui::CentralPanel::default()
            .frame(Frame {
                inner_margin: Margin::ZERO,
                outer_margin: Margin::ZERO,
                rounding: Rounding::ZERO,
                shadow: Shadow::NONE,
                fill: Default::default(),
                stroke: Default::default(),
            })
            .show(ctx, |ui| {
                self.tree.ui(&mut self.panel_manager, ui);
            });

        self.status_bar(ctx);

        {
            let app_state = EditorAppState::get();
            let mut scene_manager = SceneManager::get_mut();
            scene_manager.prepare();
            let last_cursor_pos = app_state
                .game_response
                .as_ref()
                .map(|res| res.rect.center());
            let input = Input::from_ctx(
                ctx,
                app_state.game_response.as_ref(),
                InputState {
                    is_active: self.is_game_focused(),
                    last_cursor_pos,
                },
            );
            scene_manager.update(&input);
        }

        self.fps_counter += 1;
        if Time::timer("fps") >= 1.0 {
            self.fps = self.fps_counter;
            self.fps_counter = 0;
            Time::reset_timer("fps");
        }

        SceneManager::get_mut()
            .current_scene_mut()
            .delete_game_objects();
        AssetRegistry::get().reload_assets();

        ctx.request_repaint();
    }
}

impl EditorApp {
    fn get_physical_size(ctx: &egui::Context, viewport_size: (f32, f32)) -> (u32, u32) {
        let window_size = ctx
            .input(|i| i.viewport().inner_rect)
            .unwrap_or(egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::ZERO));
        let pixels_per_point = ctx.pixels_per_point();
        let width = window_size.width() * viewport_size.0 * pixels_per_point;
        let height = window_size.height() * viewport_size.1 * pixels_per_point;
        (width as u32, height as u32)
    }
}

impl EditorApp {
    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        SceneManager::get_mut().load_default_scene();
                        ui.close_menu();
                    }
                    if ui.button("Open").clicked() {
                        SceneManager::get_mut().load_scene(
                            ProjectManager::get()
                                .current_project()
                                .assets_directory()
                                .join("scene.cxscene"),
                        );
                        ui.close_menu();
                    }
                    if ui.button("Save").clicked() {
                        let res = std::fs::OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(true)
                            .open(
                                ProjectManager::get()
                                    .current_project()
                                    .assets_directory()
                                    .join("scene.cxscene"),
                            );
                        if let Ok(file) = res {
                            let writer = BufWriter::new(file);
                            serde_json::to_writer_pretty(
                                writer,
                                SceneManager::get().simulation_scene(),
                            )
                            .unwrap();
                        }
                        ui.close_menu();
                    }
                    if ui.button("Save As").clicked() {
                        ui.close_menu();
                    }
                });

                {
                    let png = include_image!("../../resources/icons/compile_dark.png");
                    let image = egui::Image::new(png)
                        .fit_to_exact_size(Vec2::new(BASE_FONT_SIZE, BASE_FONT_SIZE));
                    if ui
                        .add(
                            Button::image(image)
                                .rounding(Rounding::ZERO)
                                .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        ProjectManager::get().build_assemblies();
                    }
                }

                {
                    let png_play = include_image!("../../resources/icons/execute_dark.png");
                    let image_play = egui::Image::new(png_play)
                        .fit_to_exact_size(Vec2::new(BASE_FONT_SIZE, BASE_FONT_SIZE));
                    let png_pause = include_image!("../../resources/icons/pause_dark.png");
                    let image_pause = egui::Image::new(png_pause)
                        .fit_to_exact_size(Vec2::new(BASE_FONT_SIZE, BASE_FONT_SIZE));
                    if ui
                        .add(
                            Button::image(if SceneManager::get().is_simulating() {
                                image_pause
                            } else {
                                image_play
                            })
                            .rounding(Rounding::ZERO)
                            .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        let mut sm = SceneManager::get_mut();
                        if sm.is_simulating() {
                            sm.pause_simulation();
                        } else {
                            sm.start_simulation();
                        }
                    }
                }

                {
                    let png = include_image!("../../resources/icons/suspend_dark.png");
                    let image = egui::Image::new(png)
                        .fit_to_exact_size(Vec2::new(BASE_FONT_SIZE, BASE_FONT_SIZE));
                    if ui
                        .add_enabled(
                            SceneManager::get().has_simulation_scene(),
                            Button::image(image)
                                .rounding(Rounding::ZERO)
                                .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        SceneManager::get_mut().stop_simulation();
                    }
                }
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

    fn is_game_focused(&self) -> bool {
        self.panel_manager
            .panel::<PanelGame>()
            .map(|panel| panel.is_cursor_grabbed)
            .unwrap_or_default()
    }
}

impl EditorApp {
    pub fn run() -> eframe::Result<()> {
        // LOAD PROJECT
        let args: Vec<String> = env::args().collect();
        if args.len() != 2 {
            eprintln!("Expected 2 arguments, got {}", args.len());
            std::process::exit(1);
        }

        // START ACTUAL EDITOR
        ProjectManager::init();
        ProjectManager::get_mut().load(PathBuf::from(&args[1]));
        // ProjectManager::get().build_assemblies();

        Time::init();
        AssetRegistry::init();
        AssetRegistry::get_mut()
            .set_root_path(ProjectManager::get().current_project().assets_directory());

        SceneManager::init();

        TypeRegistry::init();
        {
            let mut registry = TypeRegistry::get_mut();
            for f in inventory::iter::<ReflectRegistrationFn>() {
                (f.0)(&mut registry);
            }
        }
        ClassRegistry::init();
        InspectorRegistry::init();
        LogRegistry::init();

        log::set_boxed_logger(Box::new(Logger)).expect("Unable to setup logger");
        log::set_max_level(LevelFilter::Debug);

        let options = NativeOptions {
            viewport: egui::ViewportBuilder {
                inner_size: Some(egui::vec2(1280.0, 720.0)),
                min_inner_size: Some(egui::vec2(1280.0, 720.0)),
                maximized: Some(true),
                transparent: Some(true),
                decorations: Some(true),
                ..Default::default()
            },
            persist_window: true,
            renderer: eframe::Renderer::Wgpu,
            wgpu_options: egui_wgpu::WgpuConfiguration {
                device_descriptor: Arc::new(|_adapter| wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
                        | wgpu::Features::POLYGON_MODE_LINE
                        | wgpu::Features::CLEAR_TEXTURE
                        | wgpu::Features::FLOAT32_FILTERABLE
                        | wgpu::Features::DEPTH32FLOAT_STENCIL8,
                    required_limits: wgpu::Limits {
                        max_color_attachment_bytes_per_sample: 48,
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true);
            })),
            ..Default::default()
        };
        let name = format!("Calyx — {}", ProjectManager::get().current_project().name());
        eframe::run_native(
            name.as_str(),
            options,
            Box::new(|cc| {
                let mut app_state = EditorAppState::get_mut();
                let app = EditorApp::new(cc);
                app_state.scene_renderer = app.scene_renderer.clone().into();
                app_state.game_renderer = app.game_renderer.clone().into();
                Ok(Box::new(app))
            }),
        )
    }
}
