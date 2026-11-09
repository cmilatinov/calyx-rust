use crate::context::{AssetContext, GameContext, UpdatedScene};
use crate::error::DynError;
use crate::input::Input;
use crate::logging::{DefaultLogger, Log};
use crate::profiler;
use crate::reflect::type_registry::TypeRegistry;
use crate::render::{SceneRenderer, SceneRendererOptions};
use crate::runtime::{
    initial_render_size, wgpu_configuration, FrameCounter, HostCommand, ProjectRuntimeSettings,
    WindowSettings,
};
use crate::scene::{Scene, SceneHud};
use crate::ui::{UiInput, UiPoint};
use eframe::{Frame, NativeOptions};
use egui::load::SizedTexture;
use egui::{Context, Direction, Image, ImageSource, InnerResponse, Layout, Rect, Sense};
use egui_wgpu::wgpu;
use std::path::PathBuf;
#[cfg(windows)]
use winit::platform::windows::EventLoopBuilderExtWindows;
// winit's default features enable X11 on Linux. Dependency features are not
// exposed as cfg(feature = "...") values in this crate.
#[cfg(target_os = "linux")]
use winit::platform::x11::EventLoopBuilderExtX11;

struct GameApp {
    game: GameContext,
    renderer: SceneRenderer,
    hud: SceneHud,
    frame_counter: FrameCounter,
    #[allow(unused)]
    log: Log<DefaultLogger>,
}

impl GameApp {
    fn new(
        cc: &eframe::CreationContext,
        settings: &ProjectRuntimeSettings,
        plugin_main: extern "C" fn(&mut TypeRegistry),
    ) -> Result<Self, Box<DynError>> {
        let startup_scene = player_scene(settings, std::env::var_os(SCENE_ENV).as_deref())
            .ok_or("the project does not configure a startup scene")?;
        if let Some(render_state) = &cc.wgpu_render_state {
            profiler::gpu::install(&render_state.device, &render_state.queue);
        }
        if std::env::var_os(PROFILE_ENV).is_some() {
            profiler::set_capture(true);
        }
        let assets = AssetContext::new(cc, settings.assets_directory())?;
        {
            let mut type_registry = assets.registries.types.write();
            plugin_main(&mut type_registry);
            assets
                .registries
                .components
                .write()
                .refresh_class_lists(&type_registry);
        }
        let mut game = GameContext::new(assets.clone());
        let scene = assets
            .registries
            .assets
            .read()
            .load_by_path::<Scene>(&startup_scene)
            .map_err(|error| {
                format!(
                    "Failed to load startup scene '{}': {error}",
                    startup_scene.display()
                )
            })?;
        game.scenes.load_scene(scene.readonly());
        game.scenes.current_scene_mut().start();
        Ok(Self {
            game,
            renderer: SceneRenderer::new(
                &assets.lock_read(),
                renderer_options(),
                initial_render_size(&cc.egui_ctx),
            )?,
            hud: SceneHud::default(),
            frame_counter: FrameCounter::default(),
            log: Log::new(
                DefaultLogger::builder()
                    .app_vendor("Calyx")
                    .app_name("Player")
                    .build(),
            ),
        })
    }

    fn physical_size(ctx: &Context, rect: &Rect) -> (u32, u32) {
        let pixels_per_point = ctx.pixels_per_point();
        (
            (pixels_per_point * rect.width()) as u32,
            (pixels_per_point * rect.height()) as u32,
        )
    }

    fn pointer_input(ctx: &Context) -> UiInput {
        ctx.input(|input| UiInput {
            pointer_position: input
                .pointer
                .latest_pos()
                .map(|position| UiPoint::new(position.x, position.y)),
            pointer_down: input.pointer.primary_down(),
            delta_time: input.stable_dt,
        })
    }
}

impl eframe::App for GameApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        profiler::new_frame();
        log_profile();
        crate::profile_scope!("Player update");
        profiler::time_ui_pass(ctx);
        self.game.update();

        let texture_id = self
            .renderer
            .scene_texture_handle()
            .map(|handle| handle.id())
            .expect("invalid or missing scene texture");
        let mut rect = Rect::ZERO;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let InnerResponse {
                    inner: response, ..
                } = ui.with_layout(
                    Layout::centered_and_justified(Direction::LeftToRight),
                    |ui| {
                        ui.add(
                            Image::new(ImageSource::Texture(SizedTexture {
                                id: texture_id,
                                size: ui.available_size(),
                            }))
                            .sense(Sense::click_and_drag()),
                        )
                    },
                );
                rect = response.rect;

                // The HUD is non-interactive, so update gameplay before taking the
                // snapshot that will be painted this frame.
                let input = Input::from_ctx(
                    ui.ctx(),
                    Some(&response),
                    self.game.resources.input_mut(),
                    true,
                );
                self.game.prepare_scene(UpdatedScene::Current);
                for command in self.game.update_scene(UpdatedScene::Current, &input) {
                    match command {
                        HostCommand::Quit => {
                            log::info!("Game requested quit; closing the player");
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
                self.hud.paint(
                    self.game.scenes.current_scene_mut(),
                    ui.painter(),
                    rect,
                    Self::pointer_input(ui.ctx()),
                );
            });

        {
            let GameApp { game, renderer, .. } = self;
            let scene = game.scenes.current_scene();
            let (width, height) = Self::physical_size(ctx, &rect);
            if width != 0 && height != 0 {
                renderer.resize_textures(width, height);
            }
            if let Some((game_object, c_camera)) = scene.main_camera() {
                let transform = scene.world_transform(game_object);

                let camera = c_camera.camera(rect.aspect_ratio());
                renderer.render_scene_base(&camera, &transform, scene, Some(&game.particles), None);
                renderer.finalize_scene();
            }
        }

        self.frame_counter.tick(self.game.resources.time());

        // ctx.grab_cursor(ctx.input(|input| input.focused));
        ctx.request_repaint();
    }
}

/// Environment variable that turns profiling on in the player, which then
/// logs a profile of the last [`PROFILE_LOG_FRAMES`] frames every as many
/// frames.
const PROFILE_ENV: &str = "CALYX_PROFILE";
const PROFILE_LOG_FRAMES: u64 = 600;

fn log_profile() {
    let frame = profiler::current_frame();
    if !profiler::is_capturing() || frame == 0 || !frame.is_multiple_of(PROFILE_LOG_FRAMES) {
        return;
    }
    let report = profiler::report(PROFILE_LOG_FRAMES as usize);
    match serde_json::to_string(&report) {
        Ok(json) => log::info!("Profile of the last {} frames: {json}", report.frames),
        Err(error) => log::warn!("Failed to serialize the profile: {error}"),
    }
}

/// Environment variable naming a scene the player opens in place of the
/// project's startup scene, relative to the project directory or absolute,
/// so a scene other than the startup one can be measured in the player.
const SCENE_ENV: &str = "CALYX_SCENE";

/// Environment variable that, set to `0`, presents frames without waiting
/// for vertical sync, so a measured frame time is the player's own rather
/// than the display's refresh interval.
const VSYNC_ENV: &str = "CALYX_VSYNC";

/// The scene the player opens: `scene`, the value of [`SCENE_ENV`], when it
/// is set and not empty, else the project's startup scene.
fn player_scene(
    settings: &ProjectRuntimeSettings,
    scene: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    match scene.filter(|scene| !scene.is_empty()) {
        Some(scene) => Some(settings.project_path(scene)),
        None => settings.startup_scene_path(),
    }
}

/// How the player presents frames: without vertical sync when `vsync`, the
/// value of [`VSYNC_ENV`], is `0`, else waiting for it as the editor does.
fn present_mode(vsync: Option<&std::ffi::OsStr>) -> wgpu::PresentMode {
    if vsync.is_some_and(|vsync| vsync == "0") {
        wgpu::PresentMode::AutoNoVsync
    } else {
        wgpu::PresentMode::AutoVsync
    }
}

/// How the player renders the scene: like the editor's Game panel, without
/// the editor grid.
fn renderer_options() -> SceneRendererOptions {
    SceneRendererOptions {
        samples: 8,
        // Lights that cast shadows keep them in the player, as in the
        // editor's game view.
        shadows: true,
        profile_name: "Game",
        ..Default::default()
    }
}

/// Storage id the player's window and egui state persist under.
const PLAYER_APP_ID: &str = "calyx-player";

fn native_options(window: &WindowSettings) -> NativeOptions {
    NativeOptions {
        viewport: egui::ViewportBuilder {
            // eframe restores a window saved under the app id before applying
            // `inner_size`. The editor saves its window under the default id,
            // the app name, so the player uses its own id and saves nothing.
            app_id: Some(PLAYER_APP_ID.to_string()),
            inner_size: Some(window.inner_size()),
            decorations: Some(true),
            ..Default::default()
        },
        // The project's window size decides the opening size, so the player
        // never saves a size that a later run would restore over it.
        persist_window: false,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            present_mode: present_mode(std::env::var_os(VSYNC_ENV).as_deref()),
            ..wgpu_configuration()
        },
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    }
}

/// Runs a project's startup scene in the standalone game player.
///
/// `project_directory` is the directory containing `project.toml`. The linked
/// project's `plugin_main` registers its component types before the startup
/// scene is loaded. The player owns the window and event loop until it closes.
pub fn run_player(
    project_directory: impl Into<PathBuf>,
    plugin_main: extern "C" fn(&mut TypeRegistry),
) -> eframe::Result<()> {
    let settings =
        ProjectRuntimeSettings::load(project_directory).map_err(eframe::Error::AppCreation)?;
    eframe::run_native(
        "Calyx",
        native_options(settings.window()),
        Box::new(move |cc| Ok(GameApp::new(cc, &settings, plugin_main).map(Box::new)?)),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        native_options, player_scene, present_mode, renderer_options, ProjectRuntimeSettings,
    };
    use egui_wgpu::wgpu;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    fn project_directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sandbox")
    }

    #[test]
    fn player_opens_the_scene_the_environment_names_over_the_startup_scene() {
        let settings = ProjectRuntimeSettings::load(project_directory()).unwrap();
        assert_eq!(
            player_scene(&settings, None),
            settings.startup_scene_path(),
            "without a scene named, the startup scene"
        );
        assert_eq!(
            player_scene(&settings, Some(OsStr::new(""))),
            settings.startup_scene_path()
        );
        assert_eq!(
            player_scene(
                &settings,
                Some(OsStr::new("assets/game/scenes/arena.cxscene"))
            ),
            Some(project_directory().join("assets/game/scenes/arena.cxscene"))
        );
    }

    #[test]
    fn player_waits_for_vsync_unless_the_environment_turns_it_off() {
        assert_eq!(present_mode(None), wgpu::PresentMode::AutoVsync);
        assert_eq!(
            present_mode(Some(OsStr::new("1"))),
            wgpu::PresentMode::AutoVsync
        );
        assert_eq!(
            present_mode(Some(OsStr::new("0"))),
            wgpu::PresentMode::AutoNoVsync
        );
    }

    #[test]
    fn player_renders_without_the_editor_grid_or_gizmos() {
        let options = renderer_options();
        assert!(!options.grid);
        assert!(options.gizmos.is_none());
    }

    #[test]
    fn player_renders_shadows() {
        assert!(renderer_options().shadows);
    }

    #[test]
    fn player_reads_startup_scene_and_window_from_sandbox_project() {
        let settings = ProjectRuntimeSettings::load(project_directory()).unwrap();

        let startup_scene = settings.startup_scene_path().unwrap();
        assert_eq!(
            startup_scene,
            project_directory().join("assets/showcase/main/showcase.cxscene")
        );
        assert!(startup_scene.is_file(), "{}", startup_scene.display());

        let options = native_options(settings.window());
        assert_eq!(
            options.viewport.inner_size,
            Some(egui::vec2(
                settings.window().width as f32,
                settings.window().height as f32
            ))
        );
        assert!(!options.persist_window);
        assert_eq!(
            options.viewport.app_id.as_deref(),
            Some(super::PLAYER_APP_ID)
        );
    }

    #[test]
    fn player_reads_the_demo_projects_own_startup_scene() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demos");
        let settings = ProjectRuntimeSettings::load(&root).unwrap();

        assert_eq!(settings.assets_directory(), root.join("assets"));
        let startup_scene = settings.startup_scene_path().unwrap();
        assert_eq!(startup_scene, root.join("assets/rpg_poly/rpg_poly.cxscene"));
        assert!(startup_scene.is_file(), "{}", startup_scene.display());
        assert_eq!(
            native_options(settings.window()).viewport.inner_size,
            Some(settings.window().inner_size())
        );
    }
}
