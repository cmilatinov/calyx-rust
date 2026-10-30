use eframe::{Frame, NativeOptions};
use egui::load::SizedTexture;
use egui::{Context, Direction, Image, ImageSource, InnerResponse, Layout, Rect, Sense};
use engine::context::{AssetContext, GameContext, UpdatedScene};
use engine::error::DynError;
use engine::input::Input;
use engine::logging::{DefaultLogger, Log};
use engine::render::{Camera, SceneRenderer, SceneRendererOptions};
use engine::runtime::{
    initial_render_size, wgpu_configuration, FrameCounter, ProjectRuntimeSettings, WindowSettings,
};
use engine::scene::{Scene, SceneHud};
use engine::ui::{UiInput, UiPoint};
use sandbox::plugin_main;
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
    ) -> Result<Self, Box<DynError>> {
        let startup_scene = settings
            .startup_scene_path()
            .ok_or("the project does not configure a startup scene")?;
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
                    .app_name("Sandbox")
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
                self.game.update_scene(UpdatedScene::Current, &input);
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

                let camera = Camera::new(
                    rect.aspect_ratio(),
                    c_camera.fov,
                    c_camera.near_plane,
                    c_camera.far_plane,
                );
                renderer.render_scene_base(&camera, &transform, scene, Some(&game.particles), None);
                renderer.finalize_scene();
            }
        }

        self.frame_counter.tick(self.game.resources.time());

        // ctx.grab_cursor(ctx.input(|input| input.focused));
        ctx.request_repaint();
    }
}

/// How the player renders the scene: like the editor's Game panel, without
/// the editor grid.
fn renderer_options() -> SceneRendererOptions {
    SceneRendererOptions {
        samples: 8,
        ..Default::default()
    }
}

/// Storage id the player's window and egui state persist under.
const PLAYER_APP_ID: &str = "calyx-player";

/// Root directory of the project this player runs.
fn project_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

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
        wgpu_options: wgpu_configuration(),
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    }
}

fn main() -> eframe::Result<()> {
    let settings =
        ProjectRuntimeSettings::load(project_directory()).map_err(eframe::Error::AppCreation)?;
    eframe::run_native(
        "Calyx",
        native_options(settings.window()),
        Box::new(move |cc| Ok(GameApp::new(cc, &settings).map(Box::new)?)),
    )
}

#[cfg(test)]
mod tests {
    use super::{native_options, project_directory, renderer_options, ProjectRuntimeSettings};

    #[test]
    fn player_renders_without_the_editor_grid_or_gizmos() {
        let options = renderer_options();
        assert!(!options.grid);
        assert!(!options.gizmos);
    }

    #[test]
    fn player_reads_startup_scene_and_window_from_sandbox_project() {
        let settings = ProjectRuntimeSettings::load(project_directory()).unwrap();

        let startup_scene = settings.startup_scene_path().unwrap();
        assert_eq!(
            startup_scene,
            project_directory().join("assets").join("showcase.cxscene")
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
}
