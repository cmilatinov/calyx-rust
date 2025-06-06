use eframe::egui_wgpu::{SurfaceErrorAction, WgpuSetup, WgpuSetupCreateNew};
use eframe::wgpu::PowerPreference;
use eframe::{egui_wgpu, wgpu, Frame, NativeOptions};
use egui::load::SizedTexture;
use egui::{
    Align2, Color32, Context, Direction, FontId, Image, ImageSource, InnerResponse, Layout, Pos2,
    Rect, Sense,
};
use engine::context::{AssetContext, GameContext};
use engine::core::Time;
use engine::error::DynError;
use engine::ext::egui::EguiContextExt;
use engine::input::{Input, InputState};
use engine::net::Network;
use engine::render::{Camera, SceneRenderer, SceneRendererOptions};
use engine::scene::Scene;
use sandbox::plugin_main;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(unix)]
#[cfg(feature = "wayland")]
use winit::platform::wayland::EventLoopBuilderExtWayland;
#[cfg(windows)]
use winit::platform::windows::EventLoopBuilderExtWindows;
#[cfg(unix)]
#[cfg(feature = "x11")]
use winit::platform::x11::EventLoopBuilderExtX11;

struct GameApp {
    game: GameContext,
    renderer: SceneRenderer,
    fps_counter: usize,
    fps: usize,
}

impl GameApp {
    fn new(cc: &eframe::CreationContext) -> Result<Self, Box<DynError>> {
        let assets =
            AssetContext::new(cc, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"))?;
        {
            let mut type_registry = assets.type_registry.write();
            plugin_main(&mut type_registry);
            assets
                .component_registry
                .write()
                .refresh_class_lists(&mut type_registry);
        }
        let mut game = GameContext::new(assets.clone());
        let scene = assets.asset_registry.read().load::<Scene>("scene").unwrap();
        game.scenes.load_scene(scene.readonly());
        Ok(Self {
            game,
            renderer: SceneRenderer::new(
                &assets.lock_read(),
                SceneRendererOptions {
                    samples: 8,
                    ..Default::default()
                },
            ),
            fps_counter: 0,
            fps: 0,
        })
    }

    fn physical_size(ctx: &Context, rect: &Rect) -> (u32, u32) {
        let pixels_per_point = ctx.pixels_per_point();
        (
            (pixels_per_point * rect.width()) as u32,
            (pixels_per_point * rect.height()) as u32,
        )
    }
}

impl eframe::App for GameApp {
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {
        self.game.resources.time_mut().update_time();

        {
            let GameContext {
                scenes, resources, ..
            } = &mut self.game;
            if let Some((network, time)) = resources.resource2_mut::<Network, Time>() {
                // TODO: Pass &Time resource directly maybe?
                network.update(scenes.current_scene_mut(), time.static_duration());
            }
        }

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
                let state = InputState {
                    is_active: true,
                    last_cursor_pos: None,
                };
                rect = response.rect;
                let input = Input::from_ctx(ui.ctx(), Some(&response), state);
                let GameContext {
                    scenes, resources, ..
                } = &mut self.game;
                let scene = scenes.current_scene_mut();
                scene.prepare();
                scene.update(resources, &input);
                ui.painter().text(
                    Pos2::new(0.0, ui.max_rect().height()),
                    Align2::LEFT_BOTTOM,
                    format!("{}", self.fps),
                    FontId::proportional(18.0),
                    Color32::GREEN,
                );
            });

        {
            let GameApp { game, renderer, .. } = self;
            let render_state = frame.wgpu_render_state().unwrap();
            let scene = game.scenes.current_scene();
            let (width, height) = Self::physical_size(ctx, &rect);
            if width != 0 && height != 0 {
                renderer.resize_textures(width, height);
            }
            if let Some((game_object, c_camera)) = scene.get_main_camera() {
                let transform = scene.get_world_transform(game_object);

                let camera = Camera::new(
                    rect.aspect_ratio(),
                    c_camera.fov,
                    c_camera.near_plane,
                    c_camera.far_plane,
                );
                renderer.render_scene(render_state, &camera, &transform, scene, None);
            }
        }

        self.fps_counter += 1;
        if self.game.resources.time().timer("fps") >= 1.0 {
            self.fps = self.fps_counter;
            self.fps_counter = 0;
            self.game.resources.time_mut().reset_timer("fps");
        }

        // ctx.grab_cursor(ctx.input(|input| input.focused));
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result<()> {
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
    eframe::run_native(
        "Calyx",
        options,
        Box::new(|cc| Ok(GameApp::new(cc).map(Box::new)?)),
    )
}
