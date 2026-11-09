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
    /// The frame after which the player closes itself, from
    /// [`EXIT_AFTER_FRAMES_ENV`].
    exit_after_frames: Option<u64>,
    #[allow(unused)]
    log: Log<DefaultLogger>,
}

impl GameApp {
    fn new(
        cc: &eframe::CreationContext,
        settings: &ProjectRuntimeSettings,
        plugin_main: extern "C" fn(&mut TypeRegistry),
    ) -> Result<Self, Box<DynError>> {
        let startup_scene = startup_scene_override()
            .or_else(|| settings.startup_scene_path())
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
            exit_after_frames: std::env::var(EXIT_AFTER_FRAMES_ENV)
                .ok()
                .and_then(|frames| frames.trim().parse().ok()),
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
        if self
            .exit_after_frames
            .is_some_and(|frames| profiler::current_frame() >= frames)
        {
            log::info!(
                "Closing the player after {} frames",
                profiler::current_frame()
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
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

/// Environment variable naming a scene file the player starts in place of
/// the project's startup scene, as a measurement run does.
const STARTUP_SCENE_ENV: &str = "CALYX_STARTUP_SCENE";
/// Environment variable that, set to `off`, presents frames without waiting
/// for vertical sync, so a measurement sees the frame time rather than the
/// display's refresh interval.
const VSYNC_ENV: &str = "CALYX_VSYNC";
/// Environment variable giving a frame count after which the player closes
/// itself, so a measurement run ends on its own and its log is complete.
const EXIT_AFTER_FRAMES_ENV: &str = "CALYX_EXIT_AFTER_FRAMES";
/// Environment variable giving the window's size as `WIDTHxHEIGHT` in
/// logical points, in place of the project's window size.
const WINDOW_SIZE_ENV: &str = "CALYX_WINDOW_SIZE";

fn startup_scene_override() -> Option<PathBuf> {
    std::env::var_os(STARTUP_SCENE_ENV).map(PathBuf::from)
}

fn vsync_off(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("off") || value == "0")
}

fn parse_window_size(value: &str) -> Option<WindowSettings> {
    let (width, height) = value.split_once(['x', 'X'])?;
    let window = WindowSettings {
        width: width.trim().parse().ok()?,
        height: height.trim().parse().ok()?,
    };
    window.non_zero_size().map(|_| window)
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
        // Nothing picks objects in the player.
        object_ids: false,
        ..Default::default()
    }
}

/// Storage id the player's window and egui state persist under.
const PLAYER_APP_ID: &str = "calyx-player";

fn native_options(window: &WindowSettings) -> NativeOptions {
    let window_override = std::env::var(WINDOW_SIZE_ENV)
        .ok()
        .and_then(|value| parse_window_size(&value));
    let window = window_override.as_ref().unwrap_or(window);
    let mut wgpu_options = wgpu_configuration();
    if vsync_off(std::env::var(VSYNC_ENV).ok().as_deref()) {
        wgpu_options.present_mode = wgpu::PresentMode::AutoNoVsync;
    }
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
        wgpu_options,
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
        native_options, parse_window_size, renderer_options, vsync_off, ProjectRuntimeSettings,
    };
    use std::path::PathBuf;

    fn project_directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sandbox")
    }

    #[test]
    fn player_renders_without_the_editor_grid_or_gizmos() {
        let options = renderer_options();
        assert!(!options.grid);
        assert!(options.gizmos.is_none());
    }

    #[test]
    fn measurement_overrides_parse_vsync_and_window_size() {
        assert!(vsync_off(Some("off")));
        assert!(vsync_off(Some("OFF")));
        assert!(vsync_off(Some("0")));
        assert!(!vsync_off(Some("on")));
        assert!(!vsync_off(None));
        let window = parse_window_size("1536x864").unwrap();
        assert_eq!((window.width, window.height), (1536, 864));
        assert!(parse_window_size("0x864").is_none());
        assert!(parse_window_size("wide").is_none());
    }

    #[test]
    fn player_draws_no_object_ids() {
        assert!(!renderer_options().object_ids);
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
