use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use inspector::inspector_registry::InspectorRegistry;
use num_traits::FromPrimitive;
use transform_gizmo_egui::{GizmoMode, GizmoOrientation};

use self::panel::*;
pub use self::project_manager::*;
use self::scene_autosave::EditorSceneAutosave;
use self::scene_document::{scene_fingerprint, write_scene_file};
use self::scene_history::{
    InspectorValueDebouncer, InspectorValueEdit, InspectorValueKey, SceneEditSnapshot,
    SceneHistory, SceneHistoryRestore,
};
use crate::camera::EditorCamera;
use crate::task_id::TaskId;
use crate::widgets::ThumbnailService;
use eframe::{wgpu, NativeOptions};
use egui::{include_image, Button, CornerRadius, ImageSource, Response, Sense, Ui, Vec2};
use egui::{Align, Layout};
use egui::{Color32, Frame, Margin, Shadow};
use egui_tiles::{Container, Linear, LinearDir, Tiles, Tree};
use egui_wgpu::wgpu::PowerPreference;
use egui_wgpu::{SurfaceErrorAction, WgpuSetup, WgpuSetupCreateNew};
use engine::component::ComponentTransform;
use engine::context::{AssetContext, GameContext};
use engine::core::Ref;
use engine::error::BoxedError;
use engine::input::{Input, InputState};
use engine::logging::{DefaultLogger, Log};
use engine::render::{Camera, SceneRenderer, SceneRendererOptions};
use engine::scene::Scene;
use engine::utils::TypeUuid;
use engine::*;
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
mod project_manager;
mod scene_autosave;
mod scene_document;
mod scene_history;
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
    pub game_response: Option<egui::Response>,
    pub game_size: (f32, f32),
    pub gizmo_modes: EnumSet<GizmoMode>,
    pub gizmo_orientation: GizmoOrientation,
    pub thumbnails: ThumbnailService,
    scene_history: SceneHistory,
    pending_inspector_value_edits: InspectorValueDebouncer,
    scene_autosave: EditorSceneAutosave,
    active_scene: ActiveSceneState,
    window_title: String,
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

    fn title(&self, project_name: &str, dirty: bool) -> String {
        let dirty_marker = if dirty { " *" } else { "" };
        format!(
            "Calyx - {project_name} - {}{dirty_marker}",
            self.scene_label()
        )
    }
}

impl EditorAppState {
    fn new(game: GameContext, initial_render_size: (u32, u32), project_path: PathBuf) -> Self {
        let asset_context = game.assets.lock_read();
        let inspector_registry = InspectorRegistry::new(&asset_context.registries.types.read());
        Self {
            game,
            camera: Default::default(),
            game_aspect: None,
            selection: Default::default(),
            hovered_game_object: None,
            viewport_size: Default::default(),
            game_size: Default::default(),
            game_response: Default::default(),
            gizmo_modes: GizmoMode::all_translate(),
            gizmo_orientation: GizmoOrientation::Global,
            thumbnails: ThumbnailService::default(),
            scene_history: SceneHistory::default(),
            pending_inspector_value_edits: InspectorValueDebouncer::default(),
            scene_autosave: EditorSceneAutosave::new(&project_path),
            active_scene: Default::default(),
            window_title: String::new(),
            scene_renderer: SceneRenderer::new(
                &asset_context,
                SceneRendererOptions {
                    grid: true,
                    gizmos: true,
                    samples: 1,
                    clear_color: Color32::from_rgb(8, 8, 8),
                    ..Default::default()
                },
                initial_render_size,
            ),
            game_renderer: SceneRenderer::new(
                &asset_context,
                SceneRendererOptions {
                    clear_color: Color32::from_rgb(0, 0, 0),
                    ..Default::default()
                },
                initial_render_size,
            ),
            inspector_registry,
        }
    }

    pub fn mark_scene_dirty(&mut self) {
        if self.game.scenes.has_simulation_scene() {
            return;
        }
        self.scene_autosave.mark_dirty();
    }

    pub fn scene_edit_snapshot(&self) -> Option<SceneEditSnapshot> {
        if self.game.scenes.has_simulation_scene() {
            return None;
        }
        Some(SceneEditSnapshot::capture(self.game.scenes.current_scene()))
    }

    pub fn commit_scene_edit(
        &mut self,
        label: impl Into<String>,
        before: Option<SceneEditSnapshot>,
    ) {
        if self.game.scenes.has_simulation_scene() {
            return;
        }
        self.flush_pending_inspector_value_edits();

        let Some(before) = before else {
            self.mark_scene_dirty();
            return;
        };

        let after = SceneEditSnapshot::capture(self.game.scenes.current_scene());
        let fingerprint = after.fingerprint().map(str::to_owned);
        if self.scene_history.push(label, before, after) {
            self.scene_autosave.sync_dirty_to_fingerprint(fingerprint);
        }
    }

    pub fn record_inspector_value_edits(
        &mut self,
        game_object: uuid::Uuid,
        component: uuid::Uuid,
        component_name: &str,
        changes: Vec<(Vec<String>, serde_json::Value, serde_json::Value)>,
    ) {
        if self.game.scenes.has_simulation_scene() || changes.is_empty() {
            return;
        }

        for (value_path, before, after) in changes {
            let value_name = if value_path.is_empty() {
                "value".to_owned()
            } else {
                value_path.join(".")
            };
            self.pending_inspector_value_edits.record(
                InspectorValueEdit {
                    label: format!("Edit {component_name} {value_name}"),
                    key: InspectorValueKey {
                        game_object,
                        component,
                        value_path,
                    },
                    before,
                    after,
                },
                std::time::Instant::now(),
            );
        }
        self.sync_scene_dirty_to_current_fingerprint();
    }

    fn flush_due_inspector_value_edits(&mut self) {
        if self.game.scenes.has_simulation_scene() {
            self.pending_inspector_value_edits.clear();
            return;
        }
        let edits = self
            .pending_inspector_value_edits
            .drain_due(std::time::Instant::now());
        self.commit_inspector_value_edits(edits);
    }

    fn flush_pending_inspector_value_edits(&mut self) {
        if self.game.scenes.has_simulation_scene() {
            self.pending_inspector_value_edits.clear();
            return;
        }
        let edits = self.pending_inspector_value_edits.drain_all();
        self.commit_inspector_value_edits(edits);
    }

    fn commit_inspector_value_edits(&mut self, edits: Vec<InspectorValueEdit>) {
        if edits.is_empty() {
            return;
        }

        let mut committed = false;
        for edit in edits {
            committed |= self.scene_history.push_inspector_value(edit);
        }
        if committed {
            self.sync_scene_dirty_to_current_fingerprint();
        }
    }

    fn sync_scene_dirty_to_current_fingerprint(&mut self) {
        self.scene_autosave
            .sync_dirty_to_fingerprint(scene_fingerprint(self.game.scenes.current_scene()));
    }

    pub fn can_undo_scene_edit(&self) -> bool {
        !self.game.scenes.has_simulation_scene()
            && (self.scene_history.can_undo() || self.pending_inspector_value_edits.has_pending())
    }

    pub fn can_redo_scene_edit(&self) -> bool {
        !self.game.scenes.has_simulation_scene()
            && !self.pending_inspector_value_edits.has_pending()
            && self.scene_history.can_redo()
    }

    pub fn undo_scene_edit(&mut self) {
        if self.game.scenes.has_simulation_scene() {
            return;
        }
        self.flush_pending_inspector_value_edits();
        let Some(restore) = self.scene_history.undo() else {
            return;
        };
        match restore {
            SceneHistoryRestore::Scene {
                label,
                snapshot,
                fingerprint,
            } => {
                self.game.scenes.restore_current_scene_snapshot(snapshot);
                self.scene_autosave.sync_dirty_to_fingerprint(fingerprint);
                log::info!("Undid scene edit: {label}");
            }
            SceneHistoryRestore::InspectorValue { label, key, value } => {
                if self.restore_inspector_value(&key, value) {
                    self.sync_scene_dirty_to_current_fingerprint();
                    log::info!("Undid scene edit: {label}");
                }
            }
        }
    }

    pub fn redo_scene_edit(&mut self) {
        if self.game.scenes.has_simulation_scene() {
            return;
        }
        self.flush_pending_inspector_value_edits();
        let Some(restore) = self.scene_history.redo() else {
            return;
        };
        match restore {
            SceneHistoryRestore::Scene {
                label,
                snapshot,
                fingerprint,
            } => {
                self.game.scenes.restore_current_scene_snapshot(snapshot);
                self.scene_autosave.sync_dirty_to_fingerprint(fingerprint);
                log::info!("Redid scene edit: {label}");
            }
            SceneHistoryRestore::InspectorValue { label, key, value } => {
                if self.restore_inspector_value(&key, value) {
                    self.sync_scene_dirty_to_current_fingerprint();
                    log::info!("Redid scene edit: {label}");
                }
            }
        }
    }

    pub fn is_scene_dirty(&self) -> bool {
        self.scene_autosave.is_dirty()
    }

    fn clear_scene_history(&mut self) {
        self.scene_history.clear();
        self.pending_inspector_value_edits.clear();
    }

    fn restore_inspector_value(
        &mut self,
        key: &InspectorValueKey,
        value: serde_json::Value,
    ) -> bool {
        let component_registry_ref = self.game.assets.registries.components.clone();
        let component_registry = component_registry_ref.read();
        let Some(component) = component_registry.component(key.component) else {
            log::warn!(
                "Skipped inspector history restore because component {} is no longer registered",
                key.component
            );
            return false;
        };
        let scene = self.game.scenes.current_scene_mut();
        let Some(game_object) = scene.find(key.game_object) else {
            log::warn!(
                "Skipped inspector history restore because game object {} no longer exists",
                key.game_object
            );
            return false;
        };
        let Some(instance) = (unsafe {
            scene
                .get_component_ptr(game_object, component)
                .map(|ptr| &mut *ptr)
        }) else {
            log::warn!(
                "Skipped inspector history restore because component {} is missing from game object {}",
                key.component,
                key.game_object
            );
            return false;
        };
        let Some(mut serialized) = instance.serialize() else {
            log::warn!(
                "Skipped inspector history restore because component {} cannot be serialized",
                key.component
            );
            return false;
        };
        if !set_json_value(&mut serialized, &key.value_path, value) {
            log::warn!(
                "Skipped inspector history restore because value path {:?} no longer exists",
                key.value_path
            );
            return false;
        }
        if !instance.deserialize_in_place(&serialized) {
            log::warn!(
                "Skipped inspector history restore because component {} cannot be deserialized",
                key.component
            );
            return false;
        }
        if key.component == ComponentTransform::type_uuid() {
            scene.clear_transform_cache();
        }
        true
    }

    pub fn open_scene_file(&mut self, file: PathBuf) {
        if !self.scene_autosave.prepare_scene_change(&self.game) {
            return;
        }
        let scene = match self
            .game
            .assets
            .registries
            .assets
            .read()
            .reload_by_path(file.as_path())
        {
            Ok(scene) => scene,
            Err(error) => {
                let message = format!("Failed to open scene {}: {}", file.display(), error);
                log::error!("{message}");
                return;
            }
        };
        self.game.scenes.load_scene(scene.readonly());
        if self.game.scenes.current_scene_meta().file.is_none() {
            self.game.scenes.set_current_scene_file(Some(file.clone()));
        }
        self.scene_autosave.scene_loaded(&self.game);
        self.clear_scene_history();
        let object_count = self.game.scenes.current_scene().objects().count();
        let message = format!("Opened scene {} ({} objects)", file.display(), object_count);
        log::info!("{message}");
    }

    pub fn open_scene_asset(&mut self, asset_id: uuid::Uuid) {
        if !self.scene_autosave.prepare_scene_change(&self.game) {
            return;
        }
        let (scene, label) = {
            let registry = self.game.assets.registries.assets.read();
            let label = registry
                .asset_meta_from_id(asset_id)
                .and_then(|meta| meta.path.map(|path| path.display().to_string()))
                .unwrap_or_else(|| asset_id.to_string());
            (registry.reload_by_id::<Scene>(asset_id), label)
        };

        match scene {
            Ok(scene) => {
                self.game.scenes.load_scene(scene.readonly());
                self.scene_autosave.scene_loaded(&self.game);
                self.clear_scene_history();
                let object_count = self.game.scenes.current_scene().objects().count();
                let message = format!("Opened scene {label} ({object_count} objects)");
                log::info!("{message}");
            }
            Err(error) => {
                let message = format!("Failed to open scene {label}: {error}");
                log::error!("{message}");
            }
        }
    }

    fn new_scene_from_default(&mut self) {
        if !self.scene_autosave.prepare_scene_change(&self.game) {
            return;
        }
        self.game.scenes.load_default_scene();
        self.scene_autosave
            .new_scene_loaded(self.game.scenes.current_scene());
        self.clear_scene_history();
        log::info!("Created new scene from default scene");
    }

    fn save_current_scene(&mut self, file: PathBuf) -> bool {
        let previous_file = self.game.scenes.current_scene_meta().file.clone();
        if !Self::save_scene_to_file(file.clone(), self.game.scenes.current_scene()) {
            return false;
        }

        self.game.scenes.set_current_scene_file(Some(file.clone()));
        self.scene_autosave.scene_saved(
            previous_file.as_deref(),
            &file,
            self.game.scenes.current_scene(),
        );
        true
    }

    fn update_scene_autosave(&mut self) {
        self.scene_autosave.update(&self.game);
    }

    fn show_scene_recovery_prompt(&mut self, ctx: &egui::Context) {
        if self
            .scene_autosave
            .show_recovery_prompt(ctx, &mut self.game)
        {
            self.clear_scene_history();
        }
    }

    fn shutdown_scene_autosave(&mut self) {
        self.flush_pending_inspector_value_edits();
        self.scene_autosave.shutdown(&self.game);
    }

    fn save_scene_to_file(file: PathBuf, scene: &Scene) -> bool {
        let object_count = scene.objects().count();
        let display_path = file.display().to_string();
        match write_scene_file(&file, scene) {
            Ok(()) => {
                log::info!("Saved scene {display_path} ({object_count} objects)");
                true
            }
            Err(error) => {
                log::error!("Failed to save scene {display_path}: {error}");
                false
            }
        }
    }
}

fn set_json_value(
    target: &mut serde_json::Value,
    path: &[String],
    value: serde_json::Value,
) -> bool {
    let Some((segment, remaining)) = path.split_first() else {
        *target = value;
        return true;
    };

    match target {
        serde_json::Value::Object(values) => values
            .get_mut(segment)
            .is_some_and(|target| set_json_value(target, remaining, value)),
        serde_json::Value::Array(values) => segment
            .parse::<usize>()
            .ok()
            .and_then(|index| values.get_mut(index))
            .is_some_and(|target| set_json_value(target, remaining, value)),
        _ => false,
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
        if !project_manager.write().load_existing_assemblies() {
            project_manager.read().build_assemblies();
        }
        let panels = Panels::new(content_browser_root);
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
            state: EditorAppState::new(
                game,
                Self::initial_render_size(&cc.egui_ctx),
                project_path.clone(),
            ),
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
        self.state.game.resources.time_mut().update_time();
        self.state.game_response = None;
        self.render_views(ctx, frame);
        self.process_thumbnail_jobs(frame);

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
        self.edit_shortcuts(ctx);
        self.state.flush_due_inspector_value_edits();

        self.status_bar(ctx);

        self.update_game(ctx);
        self.render_view_outline(frame);
        self.update_window_title(ctx);
        self.state.show_scene_recovery_prompt(ctx);

        self.fps_counter += 1;
        if self.state.game.resources.time().timer("fps") >= 1.0 {
            self.fps = self.fps_counter;
            self.fps_counter = 0;
            self.state.game.resources.time_mut().reset_timer("fps");
        }

        self.state.game.scenes.current_scene_mut().flush_deletes();
        self.state.update_scene_autosave();
        self.state
            .game
            .assets
            .registries
            .assets
            .read()
            .reload_assets();

        ctx.request_repaint();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.state.shutdown_scene_autosave();
    }
}

impl EditorApp {
    fn process_thumbnail_jobs(&mut self, frame: &mut eframe::Frame) {
        let Some(render_state) = frame.wgpu_render_state() else {
            return;
        };
        let asset_context = self.state.game.assets.lock_read();
        self.state.thumbnails.process(&asset_context, render_state);
    }

    fn update_game(&mut self, ctx: &egui::Context) {
        self.state.game.scenes.prepare();
        let input = Input::from_ctx(
            ctx,
            self.state.game_response.as_ref(),
            InputState {
                is_active: self.is_game_focused() && self.state.game_response.is_some(),
                last_cursor_pos: None,
                ..Default::default()
            },
        );
        let assets = self.state.game.assets.lock_read();
        let GameContext {
            scenes, resources, ..
        } = &mut self.state.game;
        scenes.update(&assets.registries, resources, &input);
    }

    fn update_window_title(&mut self, ctx: &egui::Context) {
        let scene_meta = self.state.game.scenes.current_scene_meta();
        self.state
            .active_scene
            .set_scene(scene_meta.asset_name.clone(), scene_meta.file.clone());

        let project_name = self.project_manager.read().current_project().name().clone();
        let title = self
            .state
            .active_scene
            .title(project_name.as_str(), self.state.is_scene_dirty());
        if self.state.window_title != title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.state.window_title = title;
        }
    }

    fn render_view_outline(&mut self, frame: &mut eframe::Frame) {
        self.state
            .scene_renderer
            .set_hovered_game_object(self.state.hovered_game_object);
        self.state
            .scene_renderer
            .set_selected_game_object(self.state.selection.first(SelectionType::GameObject));
        let render_state = frame.wgpu_render_state().unwrap();
        self.state.scene_renderer.finalize_scene(render_state);
    }

    fn render_views(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Self {
            physics_debug_pipeline,
            state:
                EditorAppState {
                    game:
                        GameContext {
                            scenes,
                            resources: _,
                            ..
                        },
                    scene_renderer,
                    game_renderer,
                    game_size,
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
        scene_renderer.set_hovered_game_object(None);
        scene_renderer.set_selected_game_object(None);

        let scene = scenes.simulation_scene();
        scene_renderer.render_scene_base(
            render_state,
            camera,
            transform,
            scene,
            Some(physics_debug_pipeline),
        );
        if let Some((node, c)) = scene.main_camera() {
            game_renderer.options_mut().clear_color = c.clear_color;
            let (width, height) = Self::get_physical_size(ctx, *game_size);
            if width == 0 || height == 0 {
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

            game_renderer.resize_textures(width, height);
            let transform = scene.world_transform(node);
            let camera = Camera::new(
                width as f32 / height as f32,
                c.fov,
                c.near_plane,
                c.far_plane,
            );
            game_renderer.render_scene_base(render_state, &camera, &transform, scene, None);
            game_renderer.finalize_scene(render_state);
        } else {
            let device = &render_state.device;
            let queue = &render_state.queue;
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            encoder.clear_texture(&game_renderer.scene_texture().texture, &Default::default());
            queue.submit(Some(encoder.finish()));
        }
    }

    fn initial_render_size(ctx: &egui::Context) -> (u32, u32) {
        let Some(window_size) = ctx.input(|i| i.viewport().inner_rect) else {
            return (0, 0);
        };
        let pixels_per_point = ctx.pixels_per_point();
        (
            (window_size.width() * pixels_per_point) as u32,
            (window_size.height() * pixels_per_point) as u32,
        )
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
}

impl EditorApp {
    fn file_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("File", |ui| {
            if ui.button("New").clicked() {
                self.new_interaction();
                ui.close_menu();
            }
            let open_response = ui
                .add_enabled(
                    self.project_manager.read().assemblies_loaded(),
                    Button::new("Open"),
                )
                .on_disabled_hover_text("Build project assemblies before opening scenes");
            if open_response.clicked() {
                self.open_interaction();
                ui.close_menu();
            }
            if ui.button("Save").clicked() {
                self.save_interaction(false);
                ui.close_menu();
            }
            if ui.button("Save As").clicked() {
                self.save_interaction(true);
                ui.close_menu();
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
        try_all!(
            None => return;
            let file = self.scene_save_file(save_as);
        );
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
                if Self::icon_button(
                    ui,
                    if is_simulating {
                        include_image!("../../resources/icons/pause_dark.png")
                    } else {
                        include_image!("../../resources/icons/execute_dark.png")
                    },
                )
                .clicked()
                {
                    if is_simulating {
                        log::info!(
                            "User paused scene simulation; objects={}",
                            self.state.game.scenes.simulation_scene().objects().count()
                        );
                        self.state.game.scenes.pause_simulation();
                    } else {
                        self.state.flush_pending_inspector_value_edits();
                        log::info!(
                            "User started scene simulation; objects={}",
                            self.state.game.scenes.current_scene().objects().count()
                        );
                        self.state.game.scenes.start_simulation();
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
                            let background_ref = self.state.game.resources.background().clone();
                            let background = background_ref.read();
                            if !background.task_list().is_empty() {
                                ui.add(egui::Spinner::new().size(15.0));
                            }
                            match background.task_list().len() {
                                0 => {}
                                1 => {
                                    let id = background.task_list().iter().next().unwrap();
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

    fn is_simulating(&self) -> bool {
        self.state.game.scenes.is_simulating()
    }
}

#[cfg(test)]
mod tests {
    use super::{set_json_value, ActiveSceneState};
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn active_scene_title_uses_untitled_fallback() {
        let mut state = ActiveSceneState::default();

        state.set_scene(None, None);
        assert_eq!(
            state.title("Sandbox", false),
            "Calyx - Sandbox - Untitled Scene"
        );
    }

    #[test]
    fn active_scene_title_uses_asset_name() {
        let mut state = ActiveSceneState::default();
        let file = PathBuf::from("assets/scene.cxscene");

        state.set_scene(Some("scene".into()), Some(file));
        assert_eq!(state.title("Sandbox", false), "Calyx - Sandbox - scene");
    }

    #[test]
    fn active_scene_title_uses_file_name_fallback() {
        let mut state = ActiveSceneState::default();
        let file = PathBuf::from("assets/scene.cxscene");

        state.set_scene(None, Some(file));
        assert_eq!(
            state.title("Sandbox", false),
            "Calyx - Sandbox - scene.cxscene"
        );
    }

    #[test]
    fn active_scene_title_marks_dirty_scene() {
        let mut state = ActiveSceneState::default();

        state.set_scene(Some("scene".into()), None);
        assert_eq!(state.title("Sandbox", true), "Calyx - Sandbox - scene *");
    }

    #[test]
    fn json_value_patch_preserves_other_component_values() {
        let mut component = json!({
            "position": [1.0, 2.0, 3.0],
            "enabled": true
        });

        assert!(set_json_value(
            &mut component,
            &["position".into(), "1".into()],
            json!(9.0)
        ));

        assert_eq!(component["position"], json!([1.0, 9.0, 3.0]));
        assert_eq!(component["enabled"], json!(true));
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

        let Some(project_path) = env::args().nth(1).map(|arg| PathBuf::from(arg)) else {
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
            Box::new(move |cc| Ok(Box::new(EditorApp::new(cc, project_path, log)?))),
        )
    }
}
