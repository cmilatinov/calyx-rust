use std::env;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use inspector::inspector_registry::InspectorRegistry;
use num_traits::FromPrimitive;
use transform_gizmo_egui::{GizmoMode, GizmoOrientation};

use self::panel::*;
pub use self::project_manager::*;
use crate::camera::EditorCamera;
use crate::task_id::TaskId;
use engine::background::Background;
use engine::context::{AssetContext, GameContext};
use engine::core::Ref;
use engine::eframe::{wgpu, NativeOptions};
use engine::egui::{include_image, Button, CornerRadius, Sense, Vec2};
use engine::egui::{Align, Layout};
use engine::egui::{Color32, Frame, Margin, Shadow};
use engine::egui_tiles::{Container, Linear, LinearDir, Tiles, Tree};
use engine::egui_wgpu::wgpu::PowerPreference;
use engine::egui_wgpu::{SurfaceErrorAction, WgpuSetup, WgpuSetupCreateNew};
use engine::error::BoxedError;
use engine::input::{Input, InputState};
use engine::rapier3d::prelude::DebugRenderPipeline;
use engine::render::{Camera, SceneRenderer, SceneRendererOptions};
use engine::scene::Scene;
use engine::transform_gizmo_egui::EnumSet;
use engine::*;
use selection::{Selection, SelectionType};
#[cfg(unix)]
#[cfg(feature = "wayland")]
use winit::platform::wayland::EventLoopBuilderExtWayland;
#[cfg(windows)]
use winit::platform::windows::EventLoopBuilderExtWindows;
#[cfg(unix)]
#[cfg(feature = "x11")]
use winit::platform::x11::EventLoopBuilderExtX11;

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
    panels: Panels,
    physics_debug_pipeline: DebugRenderPipeline,
    project_manager: Ref<ProjectManager>,
    state: EditorAppState,
}

pub struct EditorAppState {
    pub game: GameContext,
    pub scene_renderer: SceneRenderer,
    pub game_renderer: SceneRenderer,
    pub inspector_registry: InspectorRegistry,
    pub camera: EditorCamera,
    pub game_aspect: Option<(u32, u32)>,
    pub selection: Selection,
    pub viewport_size: (f32, f32),
    pub game_response: Option<egui::Response>,
    pub game_size: (f32, f32),
    pub gizmo_modes: EnumSet<GizmoMode>,
    pub gizmo_orientation: GizmoOrientation,
}

impl EditorAppState {
    fn new(game: GameContext) -> Self {
        let asset_context = game.assets.lock_read();
        let inspector_registry = InspectorRegistry::new(&asset_context.type_registry.read());
        Self {
            game,
            camera: Default::default(),
            game_aspect: None,
            selection: Default::default(),
            viewport_size: Default::default(),
            game_size: Default::default(),
            game_response: Default::default(),
            gizmo_modes: GizmoMode::all_translate(),
            gizmo_orientation: GizmoOrientation::Global,
            scene_renderer: SceneRenderer::new(
                &asset_context,
                SceneRendererOptions {
                    grid: true,
                    gizmos: true,
                    samples: 1,
                    clear_color: Color32::from_rgb(8, 8, 8),
                },
            ),
            game_renderer: SceneRenderer::new(
                &asset_context,
                SceneRendererOptions {
                    clear_color: Color32::from_rgb(0, 0, 0),
                    ..Default::default()
                },
            ),
            inspector_registry,
        }
    }
}

impl EditorApp {
    pub fn new(
        cc: &eframe::CreationContext,
        project_path: impl Into<PathBuf>,
    ) -> Result<Self, BoxedError> {
        let tree = Self::create_tree();
        let project_path = project_path.into();
        let asset_context = AssetContext::new(cc, project_path.join("assets"))?;
        let background = Background::new();
        let project_manager = ProjectManager::new(asset_context.clone(), project_path, background)?;
        let game = GameContext::new(asset_context);
        let panels = Panels::new(
            project_manager
                .read()
                .current_project()
                .root_directory()
                .clone(),
        );
        Self::apply_style(cc);
        Ok(Self {
            fps: 0,
            fps_counter: 0,
            tree,
            panels,
            physics_debug_pipeline: DebugRenderPipeline::new(
                Default::default(),
                Default::default(),
            ),
            project_manager,
            state: EditorAppState::new(game),
        })
    }

    fn apply_style(cc: &eframe::CreationContext) {
        re_ui::apply_style_and_install_loaders(&cc.egui_ctx);
        cc.egui_ctx.style_mut(|style| {
            style.spacing.text_edit_width = 150.0;
        });
    }

    fn create_tree() -> Tree<&'static str> {
        let mut tiles = Tiles::default();

        let scene_hierarchy = tiles.insert_pane(PanelSceneHierarchy::name());
        let viewport = tiles.insert_pane(PanelViewport::name());
        let animator = tiles.insert_pane(PanelAnimator::name());
        let game = tiles.insert_pane(PanelGame::name());
        let inspector = tiles.insert_pane(PanelInspector::name());
        let content_browser = tiles.insert_pane(PanelContentBrowser::name());
        let terminal = tiles.insert_pane(PanelTerminal::name());

        let center = tiles.insert_tab_tile(vec![viewport, game, animator]);
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

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        {
            let Self {
                physics_debug_pipeline,
                state:
                    EditorAppState {
                        game: GameContext { scenes, time, .. },
                        scene_renderer,
                        game_renderer,
                        game_response,
                        game_size,
                        camera:
                            EditorCamera {
                                camera, transform, ..
                            },
                        ..
                    },
                ..
            } = self;

            time.update_time();
            *game_response = None;
            scenes.simulation_scene().clear_transform_cache();
            let render_state = frame.wgpu_render_state().unwrap();
            let (width, height) = EditorApp::get_physical_size(ctx, self.state.viewport_size);
            if width != 0 && height != 0 {
                scene_renderer.resize_textures(width, height);
                camera.aspect = width as f32 / height as f32;
            }
            camera.update_projection();

            {
                let scene = scenes.simulation_scene();
                scene_renderer.render_scene(
                    render_state,
                    camera,
                    transform,
                    scene,
                    Some(physics_debug_pipeline),
                );
                if let Some((node, c)) = scene.get_main_camera(&scene.world) {
                    game_renderer.options_mut().clear_color = c.clear_color;
                    let (width, height) = EditorApp::get_physical_size(ctx, *game_size);
                    if width != 0 && height != 0 {
                        game_renderer.resize_textures(width, height);
                    }
                    let transform = scene.get_world_transform(node);
                    let mut camera = Camera::new(
                        width as f32 / height as f32,
                        c.fov,
                        c.near_plane,
                        c.far_plane,
                    );
                    camera.update_projection();
                    game_renderer.render_scene(render_state, &camera, &transform, scene, None)
                } else {
                    let device = &render_state.device;
                    let queue = &render_state.queue;
                    let mut encoder = device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                    encoder
                        .clear_texture(&game_renderer.scene_texture().texture, &Default::default());
                    queue.submit(Some(encoder.finish()));
                }
            }
        }

        self.menu_bar(ctx);

        egui::CentralPanel::default()
            .frame(Frame {
                inner_margin: Margin::ZERO,
                outer_margin: Margin::ZERO,
                corner_radius: CornerRadius::ZERO,
                shadow: Shadow::NONE,
                fill: Default::default(),
                stroke: Default::default(),
            })
            .show(ctx, |ui| {
                let Self { panels, state, .. } = self;
                let mut panel_manager = PanelManager { panels, state };
                self.tree.ui(&mut panel_manager, ui);
            });

        self.status_bar(ctx);

        {
            self.state.game.scenes.prepare();
            let last_cursor_pos = self
                .state
                .game_response
                .as_ref()
                .map(|res| res.rect.center());
            let input = Input::from_ctx(
                ctx,
                self.state.game_response.as_ref(),
                InputState {
                    is_active: self.is_game_focused(),
                    last_cursor_pos,
                },
            );
            let GameContext { scenes, time, .. } = &mut self.state.game;
            scenes.update(time, &input);
        }

        self.fps_counter += 1;
        if self.state.game.time.timer("fps") >= 1.0 {
            self.fps = self.fps_counter;
            self.fps_counter = 0;
            self.state.game.time.reset_timer("fps");
        }

        self.state
            .game
            .scenes
            .current_scene_mut()
            .delete_game_objects();
        self.state
            .game
            .assets
            .asset_registry
            .write()
            .reload_assets();

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
                        self.state.game.scenes.load_default_scene();
                        ui.close_menu();
                    }
                    if ui.button("Open").clicked() {
                        if let Ok(scene) = self
                            .state
                            .game
                            .assets
                            .asset_registry
                            .read()
                            .load::<Scene>("scene")
                        {
                            self.state.game.scenes.load_scene(scene.readonly());
                        }
                        ui.close_menu();
                    }
                    if ui.button("Save").clicked() {
                        let res = std::fs::OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(true)
                            .open(
                                self.project_manager
                                    .read()
                                    .current_project()
                                    .assets_directory()
                                    .join("scene.cxscene"),
                            );
                        if let Ok(file) = res {
                            let writer = BufWriter::new(file);
                            serde_json::to_writer_pretty(
                                writer,
                                self.state.game.scenes.simulation_scene(),
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
                                .corner_radius(CornerRadius::ZERO)
                                .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        self.project_manager.read().build_assemblies();
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
                            Button::image(if self.is_simulating() {
                                image_pause
                            } else {
                                image_play
                            })
                            .corner_radius(CornerRadius::ZERO)
                            .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        if self.is_simulating() {
                            self.state.game.scenes.pause_simulation();
                        } else {
                            self.state.game.scenes.start_simulation();
                        }
                    }
                }

                {
                    let png = include_image!("../../resources/icons/suspend_dark.png");
                    let image = egui::Image::new(png)
                        .fit_to_exact_size(Vec2::new(BASE_FONT_SIZE, BASE_FONT_SIZE));
                    if ui
                        .add_enabled(
                            self.state.game.scenes.has_simulation_scene(),
                            Button::image(image)
                                .corner_radius(CornerRadius::ZERO)
                                .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        self.state.game.scenes.stop_simulation();
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
                            let task_list_ref = self.state.game.background.task_list();
                            let task_list = task_list_ref.read();
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
        self.panels
            .panel::<PanelGame>()
            .map(|panel| panel.is_cursor_grabbed)
            .unwrap_or_default()
    }

    fn is_simulating(&self) -> bool {
        self.state.game.scenes.is_simulating()
    }
}

impl EditorApp {
    pub fn run() -> eframe::Result<()> {
        // LOAD PROJECT
        let args: Vec<String> = env::args().collect();
        if args.len() != 2 {}

        let Some(project_path) = env::args().nth(1).map(|arg| PathBuf::from(arg)) else {
            eprintln!("Expected 2 arguments, got {}", args.len());
            std::process::exit(1);
        };

        // START ACTUAL EDITOR
        // ProjectManager::init();
        // ProjectManager::get_mut().load(PathBuf::from(&args[1]));
        // ProjectManager::get().build_assemblies();

        // Time::init();
        // AssetRegistry::init();
        // AssetRegistry::get_mut()
        //     .set_root_path(ProjectManager::get().current_project().assets_directory());

        // SceneRegistry::init();
        //
        // TypeRegistry::init();
        // {
        //     let mut registry = TypeRegistry::get_mut();
        //     for f in inventory::iter::<ReflectRegistrationFn>() {
        //         (f.0)(&mut registry);
        //     }
        // }
        // ComponentRegistry::init();
        // InspectorRegistry::init();
        // LogRegistry::init();

        // log::set_boxed_logger(Box::new(Logger)).expect("Unable to setup logger");
        // log::set_max_level(LevelFilter::Debug);

        let options = NativeOptions {
            viewport: egui::ViewportBuilder {
                inner_size: Some(egui::vec2(1280.0, 720.0)),
                min_inner_size: Some(egui::vec2(1280.0, 720.0)),
                decorations: Some(true),
                ..Default::default()
            },
            persist_window: true,
            renderer: eframe::Renderer::Wgpu,
            wgpu_options: egui_wgpu::WgpuConfiguration {
                present_mode: Default::default(),
                desired_maximum_frame_latency: None,
                on_surface_error: Arc::new(|_| SurfaceErrorAction::SkipFrame),
                wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew {
                    instance_descriptor: Default::default(),
                    power_preference: PowerPreference::HighPerformance,
                    native_adapter_selector: None,
                    device_descriptor: Arc::new(|_adapter| {
                        wgpu::DeviceDescriptor {
                            required_features: wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
                                | wgpu::Features::POLYGON_MODE_LINE
                                | wgpu::Features::CLEAR_TEXTURE
                                | wgpu::Features::FLOAT32_FILTERABLE
                                | wgpu::Features::DEPTH32FLOAT_STENCIL8
                                | wgpu::Features::BUFFER_BINDING_ARRAY
                                | wgpu::Features::TEXTURE_BINDING_ARRAY
                                | wgpu::Features::STORAGE_RESOURCE_BINDING_ARRAY
                                | wgpu::Features::UNIFORM_BUFFER_AND_STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING,
                            required_limits: wgpu::Limits {
                                max_storage_textures_per_shader_stage: 5,
                                max_uniform_buffers_per_shader_stage: 30,
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    }),
                    trace_path: None,
                }),
            },
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true);
            })),
            ..Default::default()
        };
        // let name = format!("Calyx — {}", ProjectManager::get().current_project().name());
        eframe::run_native(
            "Calyx",
            options,
            Box::new(|cc| Ok(Box::new(EditorApp::new(cc, project_path)?))),
        )
    }
}
