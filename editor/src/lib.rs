// wgpu's nested handle types exceed the default trait-solver depth when
// checking Send/Sync on assets and components that hold GPU resources.
#![recursion_limit = "256"]

use std::env;
use std::path::PathBuf;

use eframe::egui;
use inspector::inspector_registry::InspectorRegistry;
use inspector::pinned_components::PinnedComponents;
use num_traits::FromPrimitive;
use transform_gizmo_egui::{GizmoMode, GizmoOrientation};

use self::panel::*;
use self::prefab_editing::PrefabEditing;
pub use self::project_manager::*;
use self::scene_document::SceneDocument;
// The inspector's list-edit tests patch values the way undo does.
#[cfg(test)]
use self::scene_document::set_json_value;
use self::scene_edit::SceneEdit;
use crate::camera::EditorCamera;
use crate::task_id::TaskId;
use crate::widgets::ThumbnailService;
use eframe::{wgpu, NativeOptions};
use egui::{include_image, Button, CornerRadius, ImageSource, Response, Sense, Ui, Vec2};
use egui::{Align, Layout};
use egui::{Color32, Frame, Margin, Shadow};
use egui_tiles::{Container, Linear, LinearDir, Tiles, Tree};
use engine::context::{AssetContext, GameContext, UpdatedScene};
use engine::core::Ref;
use engine::error::BoxedError;
use engine::input::Input;
use engine::logging::{DefaultLogger, Log};
use engine::render::{
    Camera, ParticleSimulation, SceneRenderer, SceneRendererError, SceneRendererOptions,
};
use engine::runtime::{initial_render_size, wgpu_configuration, FrameCounter};
use engine::scene::Scene;
use rapier3d::prelude::DebugRenderPipeline;
use selection::{Selection, SelectionType};
use transform_gizmo_egui::EnumSet;
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
mod prefab_editing;
mod project_manager;
mod remote;
mod scene_autosave;
mod scene_document;
mod scene_edit;
mod scene_file;
mod scene_history;
mod selection;
mod task_id;
mod widgets;

pub use project_manager::cargo_build_args as project_assembly_build_args;

pub const BASE_FONT_SIZE: f32 = 16.0;

const CLOSE_PREFAB_FIRST: &str = "Close the prefab to return to the scene first";
const PLAY_IN_PREFAB_MODE: &str =
    "Play is not available in prefab mode; close the prefab to play the scene";
/// The fill of the banner that marks prefab mode.
const PREFAB_MODE_COLOR: Color32 = Color32::from_rgb(28, 74, 120);
/// The scene view's clear color, behind the grid where nothing is drawn. The
/// renderer takes it as linear color, so this near-black shows as dark gray.
const SCENE_VIEW_BACKDROP: Color32 = Color32::from_rgb(8, 8, 8);
/// The scene view's clear color in prefab mode: a muted blue-gray in the
/// banner's hue, so prefab mode reads at a glance while the grid, gizmos and
/// objects stay legible. Linear, like [`SCENE_VIEW_BACKDROP`], so it shows
/// as a lighter slate blue.
const PREFAB_MODE_BACKDROP: Color32 = Color32::from_rgb(6, 12, 26);

/// The scene view's clear color: blue-gray in prefab mode, else the normal
/// backdrop.
fn scene_view_backdrop(prefab_mode: bool) -> Color32 {
    if prefab_mode {
        PREFAB_MODE_BACKDROP
    } else {
        SCENE_VIEW_BACKDROP
    }
}

pub struct EditorApp {
    frame_counter: FrameCounter,
    tree: Tree<&'static str>,
    panels: Panels,
    physics_debug_pipeline: DebugRenderPipeline,
    remote: Option<remote::RemoteRuntime>,
    /// Set once closing the window no longer waits on the unsaved-prefab
    /// prompt: the user answered it, or a remote client asked to shut down.
    close_confirmed: bool,
    // Drop runtime/editor state before the project manager so assembly-backed
    // scene components and registry trait objects are destroyed while the
    // project assembly is still loaded.
    state: EditorAppState,
    project_manager: Ref<ProjectManager>,
    _log: Log<DefaultLogger>,
}

pub struct EditorAppState {
    pub game: GameContext,
    pub scene_renderer: SceneRenderer,
    pub game_renderer: SceneRenderer,
    pub inspector_registry: InspectorRegistry,
    pub camera: EditorCamera,
    pub game_aspect: Option<(u32, u32)>,
    pub selection: Selection,
    pub hovered_game_object: Option<uuid::Uuid>,
    pub viewport_size: (f32, f32),
    pub viewport_response: Option<egui::Response>,
    pub game_response: Option<egui::Response>,
    pub game_size: (f32, f32),
    pub gizmo_modes: EnumSet<GizmoMode>,
    pub gizmo_orientation: GizmoOrientation,
    pub thumbnails: ThumbnailService,
    /// Where the inspector panel was drawn last frame, if it was.
    pub(crate) inspector_rect: Option<egui::Rect>,
    pub(crate) scene_document: SceneDocument,
    /// The components the inspector shows first, read at project load.
    pub(crate) pinned_components: PinnedComponents,
    active_scene: ActiveSceneState,
    window_title: String,
    project_path: PathBuf,
    /// The prefab open in prefab mode, if one is.
    prefab: Option<PrefabEditing>,
    /// What waits on the user's answer to the unsaved-prefab prompt.
    prefab_prompt: Option<PrefabPrompt>,
}

/// What the editor does once the user answers the prompt about an open
/// prefab's unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrefabPrompt {
    /// Return to the scene.
    Close,
    /// Open another prefab asset.
    Open(uuid::Uuid),
    /// Close the editor window.
    Quit,
}

#[derive(Debug, Default)]
struct ActiveSceneState {
    label: Option<String>,
}

impl ActiveSceneState {
    fn set_scene(&mut self, asset_name: Option<String>, file: Option<PathBuf>) {
        self.label = asset_name.or_else(|| {
            file.as_ref()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        });
    }

    fn scene_label(&self) -> String {
        self.label
            .as_ref()
            .cloned()
            .unwrap_or_else(|| "Untitled Scene".into())
    }

    fn title(&self, project_name: &str, prefab: Option<&str>, dirty: bool) -> String {
        let dirty_marker = if dirty { " *" } else { "" };
        match prefab {
            Some(prefab) => format!("Calyx - {project_name} - Prefab: {prefab}{dirty_marker}"),
            None => format!(
                "Calyx - {project_name} - {}{dirty_marker}",
                self.scene_label()
            ),
        }
    }
}

impl EditorAppState {
    fn new(
        game: GameContext,
        initial_render_size: (u32, u32),
        project_path: PathBuf,
    ) -> Result<Self, SceneRendererError> {
        let asset_context = game.assets.lock_read();
        let inspector_registry = InspectorRegistry::new(&asset_context.registries.types.read());
        let scene_renderer = SceneRenderer::new(
            &asset_context,
            SceneRendererOptions {
                grid: true,
                gizmos: true,
                samples: 1,
                clear_color: SCENE_VIEW_BACKDROP,
                ..Default::default()
            },
            initial_render_size,
        )?;
        let game_renderer = SceneRenderer::new(
            &asset_context,
            SceneRendererOptions {
                clear_color: Color32::from_rgb(0, 0, 0),
                ..Default::default()
            },
            initial_render_size,
        )?;
        drop(asset_context);
        Ok(Self {
            game,
            camera: Default::default(),
            game_aspect: None,
            selection: Default::default(),
            hovered_game_object: None,
            viewport_size: Default::default(),
            viewport_response: Default::default(),
            game_size: Default::default(),
            game_response: Default::default(),
            gizmo_modes: GizmoMode::all_translate(),
            gizmo_orientation: GizmoOrientation::Global,
            thumbnails: ThumbnailService::default(),
            inspector_rect: None,
            scene_document: SceneDocument::new(&project_path),
            pinned_components: PinnedComponents::load(&project_path),
            active_scene: Default::default(),
            window_title: String::new(),
            project_path,
            prefab: None,
            prefab_prompt: None,
            scene_renderer,
            game_renderer,
            inspector_registry,
        })
    }

    /// Records `edit` as one undo step.
    pub(crate) fn commit_scene_edit(&mut self, edit: SceneEdit) {
        edit.commit(&mut self.scene_document, &self.game);
    }

    /// Makes `object` the object the inspector tracks, committing the edits
    /// pending on the previous one first.
    pub(crate) fn select_inspector_object(&mut self, object: Option<uuid::Uuid>) {
        self.scene_document
            .select_inspector_object(&self.game, object);
    }

    /// Commits every inspector edit still pending, such as a drag in
    /// progress, so it is recorded before another edit or an undo.
    fn flush_pending_inspector_value_edits(&mut self) {
        self.scene_document.flush_pending_values(&self.game);
    }

    fn refresh_inspector_baselines(&mut self) {
        self.scene_document.refresh_baselines(&self.game);
    }

    pub fn can_undo_scene_edit(&self) -> bool {
        self.scene_document.can_undo(&self.game)
    }

    pub fn can_redo_scene_edit(&self) -> bool {
        self.scene_document.can_redo(&self.game)
    }

    pub fn undo_scene_edit(&mut self) {
        self.scene_document.undo(&mut self.game);
    }

    pub fn redo_scene_edit(&mut self) {
        self.scene_document.redo(&mut self.game);
    }

    pub fn is_scene_dirty(&self) -> bool {
        self.scene_document.is_dirty()
    }

    pub fn open_scene_file(&mut self, file: PathBuf) -> bool {
        if self.refuse_scene_change_in_prefab_mode() {
            return false;
        }
        self.scene_document.open_file(&mut self.game, file)
    }

    pub fn open_scene_asset(&mut self, asset_id: uuid::Uuid) {
        if self.refuse_scene_change_in_prefab_mode() {
            return;
        }
        self.scene_document.open_asset(&mut self.game, asset_id);
    }

    fn new_scene_from_default(&mut self) {
        if self.refuse_scene_change_in_prefab_mode() {
            return;
        }
        self.scene_document.open_default(&mut self.game);
    }

    fn save_current_scene(&mut self, file: PathBuf) -> bool {
        if self.refuse_scene_change_in_prefab_mode() {
            return false;
        }
        self.scene_document.save(&mut self.game, file)
    }

    /// Logs and returns `true` while a prefab is open, which keeps the scene
    /// set aside until the prefab closes.
    fn refuse_scene_change_in_prefab_mode(&self) -> bool {
        let Some(prefab) = &self.prefab else {
            return false;
        };
        log::warn!(
            "Close the prefab {} before opening or saving a scene",
            prefab.label()
        );
        true
    }

    /// The prefab open in prefab mode, if one is.
    pub(crate) fn prefab(&self) -> Option<&PrefabEditing> {
        self.prefab.as_ref()
    }

    /// Opens the prefab asset `asset_id` in prefab mode, closing the open
    /// prefab first. Refuses while project assemblies are not loaded, since
    /// the prefab's project components would be dropped and lost on save,
    /// and while the open prefab has unsaved changes.
    pub(crate) fn open_prefab(&mut self, asset_id: uuid::Uuid) -> Result<(), String> {
        let assemblies_loaded = self
            .game
            .resources
            .resource::<Ref<ProjectAssemblyStatus>>()
            .is_some_and(|status| status.read().is_loaded());
        if !assemblies_loaded {
            return Err("build project assemblies before opening a prefab".into());
        }
        if let Some(prefab) = &self.prefab {
            if prefab.asset_id() == asset_id {
                return Ok(());
            }
            if self.is_scene_dirty() {
                return Err(format!(
                    "the open prefab {} has unsaved changes; save or close it first",
                    prefab.label()
                ));
            }
            self.close_prefab();
        }
        let prefab = PrefabEditing::open(
            &mut self.game,
            &mut self.scene_document,
            &mut self.selection,
            &self.project_path,
            asset_id,
        )?;
        self.prefab = Some(prefab);
        Ok(())
    }

    /// Saves the open prefab to its asset file.
    pub(crate) fn save_prefab(&mut self) -> Result<(), String> {
        let prefab = self.prefab.as_ref().ok_or("no prefab is open")?;
        prefab.save(&self.game, &mut self.scene_document)
    }

    /// Closes the open prefab, dropping its unsaved changes, and returns to
    /// the scene. `false` when no prefab is open.
    pub(crate) fn close_prefab(&mut self) -> bool {
        let Some(prefab) = self.prefab.take() else {
            return false;
        };
        self.prefab_prompt = None;
        prefab.close(
            &mut self.game,
            &mut self.scene_document,
            &mut self.selection,
        );
        true
    }

    /// Opens the prefab asset `asset_id`, first asking what to do with the
    /// open prefab's unsaved changes, if it has any.
    pub(crate) fn request_open_prefab(&mut self, asset_id: uuid::Uuid) {
        let open_prefab_is_dirty = self
            .prefab
            .as_ref()
            .is_some_and(|prefab| prefab.asset_id() != asset_id && self.is_scene_dirty());
        if open_prefab_is_dirty {
            self.prefab_prompt = Some(PrefabPrompt::Open(asset_id));
        } else if let Err(error) = self.open_prefab(asset_id) {
            log::error!("Failed to open prefab: {error}");
        }
    }

    /// Closes the open prefab, first asking what to do with its unsaved
    /// changes, if it has any.
    pub(crate) fn request_close_prefab(&mut self) {
        if self.prefab.is_none() {
            return;
        }
        if self.is_scene_dirty() {
            self.prefab_prompt = Some(PrefabPrompt::Close);
        } else {
            self.close_prefab();
        }
    }

    fn update_scene_document(&mut self) {
        self.scene_document.update(&self.game);
    }

    fn show_scene_recovery_prompt(&mut self, ctx: &egui::Context) {
        self.scene_document
            .show_recovery_prompt(ctx, &mut self.game);
    }

    fn shutdown_scene_document(&mut self) {
        // The prompt has had its say about the prefab's changes by now; the
        // scene set aside gets its final recovery autosave.
        self.close_prefab();
        self.scene_document.shutdown(&self.game);
    }
}

/// Whether a pointer press at `press` may start an edit outside the inspector
/// drawn at `inspector_rect`, so pending inspector edits must be committed
/// first.
fn press_ends_inspector_edits(
    press: Option<egui::Pos2>,
    inspector_rect: Option<egui::Rect>,
) -> bool {
    match (press, inspector_rect) {
        (Some(press), Some(rect)) => !rect.contains(press),
        _ => true,
    }
}

impl EditorApp {
    pub fn new(
        cc: &eframe::CreationContext,
        project_path: impl Into<PathBuf>,
        log: Log<DefaultLogger>,
    ) -> Result<Self, BoxedError> {
        let tree = Self::create_tree();
        let project_path = project_path.into();
        log::info!("Starting editor for project {}", project_path.display());
        let asset_context = AssetContext::new(cc, project_path.join("assets"))?;
        let content_browser_root = {
            let registry = asset_context.registries.assets.read();
            registry
                .asset_paths()
                .first()
                .cloned()
                .unwrap_or_else(|| registry.root_path().clone())
        };
        let mut game = GameContext::new(asset_context.clone());
        let assembly_status = Ref::new(ProjectAssemblyStatus::default());
        game.resources.insert(assembly_status.clone());
        let project_manager = ProjectManager::new(
            asset_context,
            project_path.clone(),
            game.resources.background().clone(),
            assembly_status,
        )?;
        if !project_manager
            .write()
            .load_existing_assemblies(&mut game.scenes)
        {
            project_manager.read().build_assemblies();
        }
        let panels = Panels::new(content_browser_root);
        Self::apply_style(cc);
        let remote = remote::remote_config_from_env(&project_path).and_then(|config| {
            match remote::RemoteServer::start(config) {
                Ok(server) => Some(remote::RemoteRuntime::new(server, project_path.clone())),
                Err(error) => {
                    log::error!("Failed to start remote control server: {error}");
                    None
                }
            }
        });
        Ok(Self {
            frame_counter: FrameCounter::default(),
            tree,
            panels,
            remote,
            close_confirmed: false,
            physics_debug_pipeline: DebugRenderPipeline::new(
                Default::default(),
                Default::default(),
            ),
            project_manager,
            state: EditorAppState::new(
                game,
                initial_render_size(&cc.egui_ctx),
                project_path.clone(),
            )?,
            _log: log,
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
        self.load_completed_build();
        self.state.game.resources.time_mut().update_time();
        self.state.game_response = None;
        self.state.viewport_response = None;
        self.remote_update_begin(ctx);
        // A press outside the inspector starts any edit another panel makes
        // (hierarchy, gizmo, menus) and ends a pending text-field session, so
        // the session's undo step is committed before that panel writes the
        // scene. Presses inside the inspector, such as moving the caret, are
        // left to the inspector's own drag and focus tracking.
        let press = ctx.input(|input| {
            input
                .pointer
                .any_pressed()
                .then(|| input.pointer.interact_pos())
        });
        let inspector_rect = self.state.inspector_rect.take();
        if let Some(press) = press {
            if press_ends_inspector_edits(press, inspector_rect) {
                self.state.flush_pending_inspector_value_edits();
            }
        }
        self.render_views(ctx, frame);
        self.process_thumbnail_jobs(frame);
        // Asset selectors drawn by inspectors reach the thumbnail service
        // through the egui context, since they never see `EditorAppState`.
        self.state.thumbnails.handle().install(ctx);

        self.menu_bar(ctx);
        self.prefab_banner(ctx);

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
        self.edit_shortcuts(ctx);

        self.status_bar(ctx);

        let simulated = self.state.game.scenes.is_simulating();
        self.update_game(ctx);
        self.render_view_outline();
        self.update_window_title(ctx);
        self.state.show_scene_recovery_prompt(ctx);
        self.intercept_close_with_unsaved_prefab(ctx);
        self.show_prefab_prompt(ctx);

        self.frame_counter.tick(self.state.game.resources.time());

        self.state.game.scenes.current_scene_mut().flush_deletes();
        self.state.update_scene_document();
        self.state
            .game
            .assets
            .registries
            .assets
            .read()
            .reload_assets();

        self.remote_update_end(simulated);
        ctx.request_repaint();
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.remote_raw_input_hook(raw_input);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.state.shutdown_scene_document();
    }
}

impl EditorApp {
    /// Loads the assembly a finished background build reported, if any.
    fn load_completed_build(&mut self) {
        let Some(load) = self.project_manager.read().completed_build() else {
            return;
        };
        // The load rebuilds every open scene, so commit the inspector's
        // pending edits first and take its baselines again afterwards.
        self.state.refresh_inspector_baselines();
        self.project_manager
            .write()
            .load_build(load, &mut self.state.game.scenes);
    }

    fn process_thumbnail_jobs(&mut self, frame: &mut eframe::Frame) {
        let Some(render_state) = frame.wgpu_render_state() else {
            return;
        };
        let asset_context = self.state.game.assets.lock_read();
        self.state.thumbnails.process(&asset_context, render_state);
    }

    fn update_game(&mut self, ctx: &egui::Context) {
        self.state.game.prepare_scene(UpdatedScene::Simulation);
        let is_active = self.is_game_focused() && self.state.game_response.is_some();
        let input = Input::from_ctx(
            ctx,
            self.state.game_response.as_ref(),
            self.state.game.resources.input_mut(),
            is_active,
        );
        self.state
            .game
            .update_scene(UpdatedScene::Simulation, &input);
    }

    fn update_window_title(&mut self, ctx: &egui::Context) {
        let scene_meta = self.state.game.scenes.current_scene_meta();
        self.state
            .active_scene
            .set_scene(scene_meta.asset_name.clone(), scene_meta.file.clone());

        let project_name = self.project_manager.read().current_project().name().clone();
        let title = self.state.active_scene.title(
            project_name.as_str(),
            self.state.prefab().map(PrefabEditing::label),
            self.state.is_scene_dirty(),
        );
        if self.state.window_title != title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.state.window_title = title;
        }
    }

    fn render_view_outline(&mut self) {
        self.state
            .scene_renderer
            .set_hovered_game_object(self.state.hovered_game_object);
        self.state
            .scene_renderer
            .set_selected_game_object(self.state.selection.first(SelectionType::GameObject));
        self.state.scene_renderer.finalize_scene();
    }

    fn render_views(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Self {
            physics_debug_pipeline,
            state:
                EditorAppState {
                    game:
                        GameContext {
                            scenes, particles, ..
                        },
                    scene_renderer,
                    game_renderer,
                    game_size,
                    prefab,
                    camera:
                        EditorCamera {
                            camera, transform, ..
                        },
                    ..
                },
            ..
        } = self;

        let render_state = frame.wgpu_render_state().unwrap();
        let (width, height) = Self::get_physical_size(ctx, self.state.viewport_size);
        if width != 0 && height != 0 {
            scene_renderer.resize_textures(width, height);
            camera.aspect = width as f32 / height as f32;
        }
        camera.update_projection();
        scene_renderer.options_mut().clear_color = scene_view_backdrop(prefab.is_some());
        scene_renderer.set_hovered_game_object(None);
        scene_renderer.set_selected_game_object(None);

        let scene = scenes.simulation_scene();
        let particles = Some(&*particles);
        scene_renderer.render_scene_base(
            camera,
            transform,
            scene,
            particles,
            Some(physics_debug_pipeline),
        );
        if scene.main_camera().is_some() {
            let size = Self::get_physical_size(ctx, *game_size);
            if !Self::has_renderable_size(size.0, size.1) {
                let mut encoder =
                    render_state
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Scene Renderer Encoder"),
                        });
                encoder.clear_texture(&game_renderer.scene_texture().texture, &Default::default());
                render_state.queue.submit(Some(encoder.finish()));
                return;
            }
            Self::render_game_view(game_renderer, scene, particles, size);
        } else {
            let device = &render_state.device;
            let queue = &render_state.queue;
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            encoder.clear_texture(&game_renderer.scene_texture().texture, &Default::default());
            queue.submit(Some(encoder.finish()));
        }
    }

    /// Renders `scene` from its main camera into `game_renderer` at
    /// `width` x `height` physical pixels. Returns `false`, rendering
    /// nothing, when the scene has no main camera.
    fn render_game_view(
        game_renderer: &mut SceneRenderer,
        scene: &Scene,
        particles: Option<&ParticleSimulation>,
        (width, height): (u32, u32),
    ) -> bool {
        let Some((node, c)) = scene.main_camera() else {
            return false;
        };
        game_renderer.options_mut().clear_color = c.clear_color;
        game_renderer.resize_textures(width, height);
        let transform = scene.world_transform(node);
        let camera = Camera::new(
            width as f32 / height as f32,
            c.fov,
            c.near_plane,
            c.far_plane,
        );
        game_renderer.render_scene_base(&camera, &transform, scene, particles, None);
        game_renderer.finalize_scene();
        true
    }

    fn get_physical_size(ctx: &egui::Context, viewport_size: (f32, f32)) -> (u32, u32) {
        let window_size = ctx
            .input(|i| i.viewport().inner_rect)
            .unwrap_or(egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::ZERO));
        let pixels_per_point = ctx.pixels_per_point();
        let width = window_size.width() * viewport_size.0 * pixels_per_point;
        let height = window_size.height() * viewport_size.1 * pixels_per_point;
        (width as u32, height as u32)
    }

    fn has_renderable_size(width: u32, height: u32) -> bool {
        width != 0 && height != 0
    }
}

impl EditorApp {
    fn file_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("File", |ui| {
            let prefab_mode = self.state.prefab().is_some();
            let new_response = ui
                .add_enabled(!prefab_mode, Button::new("New"))
                .on_disabled_hover_text(CLOSE_PREFAB_FIRST);
            if new_response.clicked() {
                self.new_interaction();
                ui.close_menu();
            }
            let open_response = ui
                .add_enabled(
                    !prefab_mode && self.project_manager.read().assemblies_loaded(),
                    Button::new("Open"),
                )
                .on_disabled_hover_text(if prefab_mode {
                    CLOSE_PREFAB_FIRST
                } else {
                    "Build project assemblies before opening scenes"
                });
            if open_response.clicked() {
                self.open_interaction();
                ui.close_menu();
            }
            if ui.button("Save").clicked() {
                self.save_interaction(false);
                ui.close_menu();
            }
            let save_as_response = ui
                .add_enabled(!prefab_mode, Button::new("Save As"))
                .on_disabled_hover_text("A prefab saves to its own asset file");
            if save_as_response.clicked() {
                self.save_interaction(true);
                ui.close_menu();
            }
            if prefab_mode {
                ui.separator();
                if ui.button("Close Prefab").clicked() {
                    self.state.request_close_prefab();
                    ui.close_menu();
                }
            }
        });
    }

    fn edit_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Edit", |ui| {
            if ui
                .add_enabled(self.state.can_undo_scene_edit(), Button::new("Undo"))
                .clicked()
            {
                self.state.undo_scene_edit();
                ui.close_menu();
            }
            if ui
                .add_enabled(self.state.can_redo_scene_edit(), Button::new("Redo"))
                .clicked()
            {
                self.state.redo_scene_edit();
                ui.close_menu();
            }
        });
    }

    fn tools_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Tools", |ui| {
            if ui.button("Invalidate Thumbnail Cache").clicked() {
                self.state.thumbnails.invalidate_cache();
                log::info!("Queued thumbnail cache invalidation");
                ui.close_menu();
            }
        });
    }

    fn new_interaction(&mut self) {
        self.state.new_scene_from_default();
    }

    fn open_interaction(&mut self) {
        let Some(file) = Self::pick_scene_open_file() else {
            return;
        };
        self.state.open_scene_file(file);
    }

    fn save_interaction(&mut self, save_as: bool) {
        if self.state.prefab().is_some() {
            if let Err(error) = self.state.save_prefab() {
                log::error!("{error}");
            }
            return;
        }
        let Some(file) = self.scene_save_file(save_as) else {
            return;
        };
        self.state.save_current_scene(file);
    }

    fn pick_scene_open_file() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name(".cxscene")
            .add_filter("Calyx Scene", &["cxscene"])
            .pick_file()
    }

    fn pick_scene_save_file() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name(".cxscene")
            .add_filter("Calyx Scene", &["cxscene"])
            .save_file()
    }

    fn scene_save_file(&self, save_as: bool) -> Option<PathBuf> {
        if save_as {
            return Self::pick_scene_save_file();
        }
        if self.state.game.scenes.current_scene_meta().file.is_some() {
            return self.state.game.scenes.current_scene_meta().file.clone();
        }
        Self::pick_scene_save_file()
    }

    fn icon_button(ui: &mut Ui, source: ImageSource) -> Response {
        let image =
            egui::Image::new(source).fit_to_exact_size(Vec2::new(BASE_FONT_SIZE, BASE_FONT_SIZE));
        ui.add(
            Button::image(image)
                .corner_radius(CornerRadius::ZERO)
                .sense(Sense::click()),
        )
    }

    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                self.file_menu(ui);
                self.edit_menu(ui);
                self.tools_menu(ui);

                if Self::icon_button(ui, include_image!("../../resources/icons/compile_dark.png"))
                    .clicked()
                {
                    log::info!("Queued project assembly build");
                    self.project_manager.read().build_assemblies();
                }

                let is_simulating = self.is_simulating();
                let prefab_mode = self.state.prefab().is_some();
                let play_response = ui
                    .add_enabled_ui(!prefab_mode, |ui| {
                        Self::icon_button(
                            ui,
                            if is_simulating {
                                include_image!("../../resources/icons/pause_dark.png")
                            } else {
                                include_image!("../../resources/icons/execute_dark.png")
                            },
                        )
                    })
                    .inner
                    .on_disabled_hover_text(PLAY_IN_PREFAB_MODE);
                if play_response.clicked() {
                    if is_simulating {
                        log::info!(
                            "User paused scene simulation; objects={}",
                            self.state.game.scenes.simulation_scene().objects().count()
                        );
                        self.state.game.scenes.pause_simulation();
                    } else if let Err(error) = self.start_simulation() {
                        log::error!("{error}");
                    } else {
                        log::info!(
                            "User started scene simulation; objects={}",
                            self.state.game.scenes.current_scene().objects().count()
                        );
                    }
                }

                if Self::icon_button(ui, include_image!("../../resources/icons/suspend_dark.png"))
                    .clicked()
                {
                    log::info!(
                        "User stopped scene simulation; had_simulation_scene={}",
                        self.state.game.scenes.has_simulation_scene()
                    );
                    self.state.game.scenes.stop_simulation();
                }
            });
        });
    }

    /// Shows, while a prefab is open, the banner naming the prefab asset with
    /// its save and close actions.
    fn prefab_banner(&mut self, ctx: &egui::Context) {
        let Some(prefab) = self.state.prefab() else {
            return;
        };
        let label = prefab.label().to_owned();
        let dirty = self.state.is_scene_dirty();
        let root_count = self
            .state
            .game
            .scenes
            .current_scene()
            .root_objects()
            .count();
        let mut save = false;
        let mut close = false;
        egui::TopBottomPanel::top("prefab_mode_banner")
            .frame(Frame {
                inner_margin: Margin::symmetric(8, 4),
                fill: PREFAB_MODE_COLOR,
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("PREFAB MODE")
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.separator();
                    let dirty_marker = if dirty { " *" } else { "" };
                    ui.label(
                        egui::RichText::new(format!("{label}{dirty_marker}"))
                            .monospace()
                            .color(Color32::WHITE),
                    );
                    if root_count != 1 {
                        ui.label(
                            egui::RichText::new(format!(
                                "A prefab needs exactly one root object ({root_count} now)"
                            ))
                            .color(Color32::from_rgb(255, 200, 80)),
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        close = ui
                            .button("Close Prefab")
                            .on_hover_text("Return to the scene")
                            .clicked();
                        save = ui
                            .add_enabled(dirty, Button::new("Save Prefab"))
                            .on_hover_text(
                                "Write the prefab asset; scene instances stay as they are",
                            )
                            .clicked();
                        ui.label(
                            egui::RichText::new("Scene instances are not updated")
                                .italics()
                                .color(Color32::from_gray(200)),
                        );
                    });
                });
            });
        if save {
            if let Err(error) = self.state.save_prefab() {
                log::error!("{error}");
            }
        }
        if close {
            self.state.request_close_prefab();
        }
    }

    /// Keeps the window open while the open prefab has unsaved changes, and
    /// asks the user about them instead.
    fn intercept_close_with_unsaved_prefab(&mut self, ctx: &egui::Context) {
        let close_requested = ctx.input(|input| input.viewport().close_requested());
        if !close_requested
            || self.close_confirmed
            || self.state.prefab().is_none()
            || !self.state.is_scene_dirty()
        {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        self.state.prefab_prompt = Some(PrefabPrompt::Quit);
    }

    /// Asks what to do with the open prefab's unsaved changes before the
    /// action waiting on the answer: save them, discard them, or cancel.
    fn show_prefab_prompt(&mut self, ctx: &egui::Context) {
        let (Some(action), Some(prefab)) = (self.state.prefab_prompt, self.state.prefab()) else {
            self.state.prefab_prompt = None;
            return;
        };
        let label = prefab.label().to_owned();
        enum Answer {
            Save,
            Discard,
            Cancel,
        }
        let mut answer = None;
        egui::Modal::new(egui::Id::new("prefab_unsaved_changes")).show(ctx, |ui| {
            ui.heading("Unsaved Prefab Changes");
            ui.separator();
            ui.label(format!("{label} has unsaved changes."));
            ui.label("Save them to the prefab asset before closing it?");
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    answer = Some(Answer::Save);
                }
                if ui.button("Don't Save").clicked() {
                    answer = Some(Answer::Discard);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(Answer::Cancel);
                }
            });
        });
        let Some(answer) = answer else {
            return;
        };
        self.state.prefab_prompt = None;
        match answer {
            Answer::Cancel => return,
            Answer::Save => {
                if let Err(error) = self.state.save_prefab() {
                    log::error!("{error}");
                    return;
                }
            }
            Answer::Discard => {}
        }
        match action {
            PrefabPrompt::Close => {
                self.state.close_prefab();
            }
            PrefabPrompt::Open(asset_id) => {
                self.state.close_prefab();
                if let Err(error) = self.state.open_prefab(asset_id) {
                    log::error!("Failed to open prefab: {error}");
                }
            }
            PrefabPrompt::Quit => {
                self.close_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom_panel")
            .exact_height(24.0)
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let fps = self.frame_counter.fps();
                        if fps > 0 {
                            ui.label(format!("{fps}"));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let background_ref = self.state.game.resources.background().clone();
                            let background = background_ref.read();
                            if !background.task_list().is_empty() {
                                ui.add(egui::Spinner::new().size(15.0));
                            }
                            match background.task_count() {
                                0 => {}
                                1 => {
                                    let id = background.task_list().keys().next().unwrap();
                                    if let Some(task_id) = TaskId::from_isize(*id) {
                                        ui.label(task_id.message());
                                    }
                                }
                                len => {
                                    ui.label(format!("{len} tasks"));
                                }
                            }
                        });
                    });
                });
            });
    }

    fn edit_shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl_shift = egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        if ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::CTRL, egui::Key::Y)
                || input.consume_key(ctrl_shift, egui::Key::Z)
        }) {
            self.state.redo_scene_edit();
            return;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) {
            self.state.undo_scene_edit();
        }
    }
    fn is_game_focused(&self) -> bool {
        self.panels
            .panel::<PanelGame>()
            .map(|panel| panel.is_cursor_grabbed)
            .unwrap_or_default()
    }

    /// Starts or resumes simulation as the toolbar's play button does: the
    /// project's runtime window sets the game aspect, and a fresh simulation
    /// runs the configured startup scene when the project has one.
    pub(crate) fn start_simulation(&mut self) -> Result<(), String> {
        if self.state.prefab().is_some() {
            return Err(PLAY_IN_PREFAB_MODE.into());
        }
        self.state.refresh_inspector_baselines();
        let window_size = self
            .project_manager
            .read()
            .runtime_settings()
            .window
            .non_zero_size();
        if let Some(size) = window_size {
            self.state.game_aspect = Some(size);
        }
        match self
            .project_manager
            .read()
            .start_simulation(&mut self.state.game.scenes)
        {
            Ok(Some(path)) => log::info!(
                "Loaded configured startup scene for simulation: {}",
                path.display()
            ),
            Ok(None) => {}
            Err(error) => return Err(format!("Failed to load configured startup scene: {error}")),
        }
        self.state.game.scenes.start_simulation();
        Ok(())
    }

    fn is_simulating(&self) -> bool {
        self.state.game.scenes.is_simulating()
    }
}

impl EditorApp {
    pub fn run() -> eframe::Result<()> {
        let log = Log::new(
            DefaultLogger::builder()
                .app_vendor("Calyx")
                .app_name("Editor")
                .build(),
        );
        let args: Vec<String> = env::args().collect();

        let Some(project_path) = args.get(1).cloned().map(PathBuf::from) else {
            log::error!("Expected 2 arguments, got {}", args.len());
            std::process::exit(1);
        };

        let options = NativeOptions {
            viewport: egui::ViewportBuilder {
                inner_size: Some(egui::vec2(1600.0, 900.0)),
                min_inner_size: Some(egui::vec2(1600.0, 900.0)),
                decorations: Some(true),
                ..Default::default()
            },
            persist_window: true,
            renderer: eframe::Renderer::Wgpu,
            wgpu_options: wgpu_configuration(),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true);
            })),
            ..Default::default()
        };
        // let name = format!("Calyx — {}", ProjectManager::get().current_project().name());
        eframe::run_native(
            "Calyx",
            options,
            Box::new(move |cc| Ok(Box::new(EditorApp::new(cc, project_path, log)?))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        press_ends_inspector_edits, scene_view_backdrop, ActiveSceneState, EditorApp,
        PREFAB_MODE_BACKDROP, SCENE_VIEW_BACKDROP,
    };
    use std::path::PathBuf;

    #[test]
    fn active_scene_title_uses_untitled_fallback() {
        let mut state = ActiveSceneState::default();

        state.set_scene(None, None);
        assert_eq!(
            state.title("Sandbox", None, false),
            "Calyx - Sandbox - Untitled Scene"
        );
    }

    #[test]
    fn active_scene_title_uses_asset_name() {
        let mut state = ActiveSceneState::default();
        let file = PathBuf::from("assets/scene.cxscene");

        state.set_scene(Some("scene".into()), Some(file));
        assert_eq!(
            state.title("Sandbox", None, false),
            "Calyx - Sandbox - scene"
        );
    }

    #[test]
    fn active_scene_title_uses_file_name_fallback() {
        let mut state = ActiveSceneState::default();
        let file = PathBuf::from("assets/scene.cxscene");

        state.set_scene(None, Some(file));
        assert_eq!(
            state.title("Sandbox", None, false),
            "Calyx - Sandbox - scene.cxscene"
        );
    }

    #[test]
    fn active_scene_title_marks_dirty_scene() {
        let mut state = ActiveSceneState::default();

        state.set_scene(Some("scene".into()), None);
        assert_eq!(
            state.title("Sandbox", None, true),
            "Calyx - Sandbox - scene *"
        );
    }

    #[test]
    fn prefab_mode_title_names_the_prefab_asset() {
        let mut state = ActiveSceneState::default();

        state.set_scene(Some("scene".into()), None);
        assert_eq!(
            state.title("Sandbox", Some("prefabs/crate.cxprefab"), true),
            "Calyx - Sandbox - Prefab: prefabs/crate.cxprefab *"
        );
    }

    #[test]
    fn only_prefab_mode_turns_the_scene_view_backdrop_blue() {
        assert_eq!(scene_view_backdrop(true), PREFAB_MODE_BACKDROP);
        assert_eq!(scene_view_backdrop(false), SCENE_VIEW_BACKDROP);
        let [r, g, b, _] = PREFAB_MODE_BACKDROP.to_array();
        assert!(b > g && g > r, "the prefab backdrop is blue");
    }

    #[test]
    fn only_presses_outside_the_inspector_end_its_edits() {
        let inspector =
            egui::Rect::from_min_max(egui::pos2(1000.0, 0.0), egui::pos2(1300.0, 800.0));
        assert!(!press_ends_inspector_edits(
            Some(egui::pos2(1100.0, 200.0)),
            Some(inspector)
        ));
        assert!(press_ends_inspector_edits(
            Some(egui::pos2(100.0, 200.0)),
            Some(inspector)
        ));
        assert!(press_ends_inspector_edits(
            Some(egui::pos2(1100.0, 200.0)),
            None
        ));
        assert!(press_ends_inspector_edits(None, Some(inspector)));
    }

    #[test]
    fn zero_sized_game_viewport_is_not_renderable() {
        assert!(!EditorApp::has_renderable_size(0, 720));
        assert!(!EditorApp::has_renderable_size(1280, 0));
        assert!(EditorApp::has_renderable_size(1280, 720));
    }
}
