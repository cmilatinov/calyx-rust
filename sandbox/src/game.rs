use eframe::egui_wgpu::{SurfaceErrorAction, WgpuSetup, WgpuSetupCreateNew};
use eframe::wgpu::PowerPreference;
use eframe::{egui_wgpu, wgpu, Frame, NativeOptions};
use egui::load::SizedTexture;
use egui::{Context, Direction, Image, ImageSource, InnerResponse, Layout, Rect, Sense};
use engine::context::{AssetContext, GameContext};
use engine::error::DynError;
use engine::input::{Input, InputState};
use engine::logging::{DefaultLogger, Log};
use engine::render::{Camera, SceneRenderer, SceneRendererOptions};
use engine::scene::Scene;
use engine::ui::{
    render_commands, Border, CornerRadius, EdgeInsets, EguiUiBackend, PaintCommand, PointerEvents,
    ScreenClass, StylePatch, Theme, UiArena, UiColor, UiInput, UiLength, UiNodeHandle, UiPoint,
    UiRect, UiRuntime, UiSize, UiTransform,
};
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
    ui_runtime: UiRuntime,
    ui_arena: UiArena,
    ui_theme: Theme,
    fps_counter: usize,
    fps: usize,
    #[allow(unused)]
    log: Log<DefaultLogger>,
}

impl GameApp {
    fn new(cc: &eframe::CreationContext) -> Result<Self, Box<DynError>> {
        let assets =
            AssetContext::new(cc, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"))?;
        {
            let mut type_registry = assets.registries.types.write();
            plugin_main(&mut type_registry);
            assets
                .registries
                .components
                .write()
                .refresh_class_lists(&mut type_registry);
        }
        let mut game = GameContext::new(assets.clone());
        let scene = assets
            .registries
            .assets
            .read()
            .load::<Scene>("scene")
            .unwrap();
        game.scenes.load_scene(scene.readonly());
        Ok(Self {
            game,
            renderer: SceneRenderer::new(
                &assets.lock_read(),
                SceneRendererOptions {
                    samples: 8,
                    grid: true,
                    ..Default::default()
                },
                Self::initial_render_size(&cc.egui_ctx),
            ),
            ui_runtime: UiRuntime::default(),
            ui_arena: UiArena::default(),
            ui_theme: Theme::default(),
            fps_counter: 0,
            fps: 0,
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

    fn initial_render_size(ctx: &Context) -> (u32, u32) {
        let Some(window_size) = ctx.input(|i| i.viewport().inner_rect) else {
            return (0, 0);
        };
        let pixels_per_point = ctx.pixels_per_point();
        (
            (pixels_per_point * window_size.width()) as u32,
            (pixels_per_point * window_size.height()) as u32,
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

    fn hud_text(
        ui: &mut UiArena,
        id: &'static str,
        value: impl Into<String>,
        color: UiColor,
        size: f32,
    ) -> UiNodeHandle {
        ui.text(value)
            .id(ui, id)
            .style(ui, StylePatch::default().text_color(color).font_size(size))
    }

    fn meter(
        ui: &mut UiArena,
        id: &'static str,
        value: f32,
        fill: UiColor,
        background: UiColor,
        outline: UiColor,
    ) -> UiNodeHandle {
        ui.progress_bar(value, fill)
            .id(ui, id)
            .background(ui, background)
            .border(ui, Border::solid(outline, 1.0))
            .radius(ui, CornerRadius::all(4.0))
            .width(ui, UiLength::Fill)
            .height(ui, UiLength::Px(14.0))
    }

    fn sandbox_ui(ui: &mut UiArena, viewport: UiRect, fps: usize) -> UiNodeHandle {
        let panel = UiColor::rgba(6, 13, 18, 218);
        let panel_hover = UiColor::rgba(13, 31, 40, 236);
        let cyan = UiColor::rgba(65, 231, 255, 235);
        let cyan_dim = UiColor::rgba(45, 121, 137, 190);
        let red = UiColor::rgba(239, 76, 84, 235);
        let amber = UiColor::rgba(255, 195, 81, 240);
        let green = UiColor::rgba(86, 228, 142, 235);
        let text = UiColor::rgba(229, 247, 252, 255);
        let muted = UiColor::rgba(124, 168, 181, 255);

        let panel_style = StylePatch::default()
            .background(panel)
            .border(Border::solid(cyan_dim, 1.0))
            .radius(CornerRadius::all(7.0))
            .padding(EdgeInsets::all(12.0))
            .gap(8.0)
            .transition_duration(0.18);
        let hover_tilt = StylePatch::default()
            .background(panel_hover)
            .transform(UiTransform::tilt_degrees(3.0, -7.0));

        let title = Self::hud_text(ui, "status-title", "MANTICORE // STATUS", cyan, 16.0)
            .width(ui, UiLength::Px(210.0));
        let fps_text = Self::hud_text(ui, "fps-text", format!("{fps} FPS"), muted, 12.0);
        let fps_pill = ui
            .container()
            .id(ui, "fps-pill")
            .background(ui, UiColor::rgba(11, 28, 36, 245))
            .border(ui, Border::solid(cyan_dim, 1.0))
            .radius(ui, CornerRadius::all(12.0))
            .padding(ui, EdgeInsets::symmetric(9.0, 3.0))
            .width(ui, UiLength::Px(76.0))
            .height(ui, UiLength::Px(24.0))
            .child(ui, fps_text);
        let status_spacer = ui.spacer().id(ui, "status-header-spacer");
        let status_header = ui
            .row()
            .id(ui, "status-header")
            .height(ui, UiLength::Px(28.0))
            .gap(ui, 8.0)
            .child(ui, title)
            .child(ui, status_spacer)
            .child(ui, fps_pill);

        let hull_label = Self::hud_text(ui, "hull-label", "HULL  76%", text, 13.0);
        let hull = Self::meter(
            ui,
            "hull-meter",
            0.76,
            green,
            UiColor::rgba(18, 48, 33, 230),
            UiColor::rgba(86, 228, 142, 160),
        );
        let shield_label = Self::hud_text(ui, "shield-label", "SHIELD  42%", text, 13.0);
        let shield = Self::meter(
            ui,
            "shield-meter",
            0.42,
            cyan,
            UiColor::rgba(18, 45, 54, 230),
            UiColor::rgba(65, 231, 255, 150),
        );
        let status_panel = ui
            .column()
            .id(ui, "status-panel")
            .style(ui, panel_style.clone())
            .hover_style(ui, hover_tilt.clone())
            .pointer_events(ui, PointerEvents::Auto)
            .width(ui, UiLength::Px(340.0))
            .height(ui, UiLength::Px(164.0))
            .when(
                ui,
                ScreenClass::Compact,
                StylePatch::default().width(UiLength::Px(292.0)),
            )
            .child(ui, status_header)
            .child(ui, hull_label)
            .child(ui, hull)
            .child(ui, shield_label)
            .child(ui, shield);

        let weapon_title = Self::hud_text(ui, "weapon-title", "RAIL CANNON", cyan, 15.0);
        let ammo_big = Self::hud_text(ui, "ammo-count", "042", text, 34.0)
            .width(ui, UiLength::Px(86.0))
            .height(ui, UiLength::Px(46.0));
        let reserve_ammo = Self::hud_text(ui, "reserve-ammo", "/ 120", muted, 15.0);
        let fire_mode = Self::hud_text(ui, "fire-mode", "BURST ARM", amber, 12.0);
        let ammo_meta = ui
            .column()
            .id(ui, "ammo-meta")
            .gap(ui, 3.0)
            .child(ui, reserve_ammo)
            .child(ui, fire_mode);
        let ammo_row = ui
            .row()
            .id(ui, "ammo-row")
            .height(ui, UiLength::Px(50.0))
            .child(ui, ammo_big)
            .child(ui, ammo_meta);
        let heat = Self::meter(
            ui,
            "heat-meter",
            0.31,
            amber,
            UiColor::rgba(58, 42, 18, 230),
            UiColor::rgba(255, 195, 81, 150),
        );
        let heat_label = Self::hud_text(ui, "heat-label", "HEAT", muted, 12.0);
        let weapon_panel = ui
            .column()
            .id(ui, "weapon-panel")
            .style(ui, panel_style.clone())
            .hover_style(
                ui,
                StylePatch::default()
                    .background(panel_hover)
                    .transform(UiTransform::tilt_degrees(-3.0, 8.0)),
            )
            .pointer_events(ui, PointerEvents::Auto)
            .width(ui, UiLength::Px(278.0))
            .height(ui, UiLength::Px(150.0))
            .margin(
                ui,
                EdgeInsets {
                    top: viewport.height() - 174.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 24.0,
                },
            )
            .child(ui, weapon_title)
            .child(ui, ammo_row)
            .child(ui, heat_label)
            .child(ui, heat);

        let target_title = Self::hud_text(ui, "target-title", "TARGET LOCK", red, 14.0);
        let target_name = Self::hud_text(ui, "target-name", "SIMULATED ARMOR // 284m", text, 18.0);
        let target_armor = Self::meter(
            ui,
            "target-armor",
            0.58,
            red,
            UiColor::rgba(58, 24, 28, 230),
            UiColor::rgba(239, 76, 84, 150),
        );
        let target_readout = ui
            .column()
            .id(ui, "target-panel")
            .style(ui, panel_style)
            .hover_style(ui, hover_tilt)
            .pointer_events(ui, PointerEvents::Auto)
            .width(ui, UiLength::Px(312.0))
            .height(ui, UiLength::Px(118.0))
            .margin(
                ui,
                EdgeInsets {
                    top: viewport.height() - 142.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: viewport.width() - 336.0,
                },
            )
            .child(ui, target_title)
            .child(ui, target_name)
            .child(ui, target_armor);

        let crosshair = ui
            .crosshair(cyan, 34.0)
            .id(ui, "crosshair")
            .width(ui, UiLength::Px(34.0))
            .height(ui, UiLength::Px(34.0));
        let crosshair_center = ui
            .center(crosshair)
            .id(ui, "crosshair-center")
            .width(ui, UiLength::Px(viewport.width()))
            .height(ui, UiLength::Px(viewport.height()))
            .pointer_events(ui, PointerEvents::None);

        let center_x = viewport.min.x + viewport.width() * 0.5;
        let center_y = viewport.min.y + viewport.height() * 0.5;
        let brackets = ui
            .custom_paint(vec![
                PaintCommand::Line {
                    start: UiPoint::new(center_x - 74.0, center_y - 44.0),
                    end: UiPoint::new(center_x - 38.0, center_y - 44.0),
                    color: cyan,
                    width: 2.0,
                },
                PaintCommand::Line {
                    start: UiPoint::new(center_x + 38.0, center_y - 44.0),
                    end: UiPoint::new(center_x + 74.0, center_y - 44.0),
                    color: cyan,
                    width: 2.0,
                },
                PaintCommand::Line {
                    start: UiPoint::new(center_x - 74.0, center_y + 44.0),
                    end: UiPoint::new(center_x - 38.0, center_y + 44.0),
                    color: cyan,
                    width: 2.0,
                },
                PaintCommand::Line {
                    start: UiPoint::new(center_x + 38.0, center_y + 44.0),
                    end: UiPoint::new(center_x + 74.0, center_y + 44.0),
                    color: cyan,
                    width: 2.0,
                },
            ])
            .id(ui, "target-brackets")
            .pointer_events(ui, PointerEvents::None);

        ui.stack()
            .id(ui, "sandbox-ui")
            .width(ui, UiLength::Px(viewport.width()))
            .height(ui, UiLength::Px(viewport.height()))
            .pointer_events(ui, PointerEvents::None)
            .child(ui, status_panel)
            .child(ui, weapon_panel)
            .child(ui, target_readout)
            .child(ui, crosshair_center)
            .child(ui, brackets)
    }
}

impl eframe::App for GameApp {
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {
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
                let state = InputState {
                    is_active: true,
                    last_cursor_pos: None,
                    ..Default::default()
                };
                rect = response.rect;
                let input = Input::from_ctx(ui.ctx(), Some(&response), state);
                let assets = self.game.assets.lock_read();
                let GameContext {
                    scenes, resources, ..
                } = &mut self.game;
                let scene = scenes.current_scene_mut();
                scene.prepare();
                scene.update(&assets.registries, resources, &input);

                let viewport = UiRect::from_min_size(
                    UiPoint::new(rect.min.x, rect.min.y),
                    UiSize::new(rect.width(), rect.height()),
                );
                self.ui_arena.clear();
                let sandbox_ui = Self::sandbox_ui(&mut self.ui_arena, viewport, self.fps);
                let frame = self.ui_runtime.frame(
                    &self.ui_arena,
                    sandbox_ui,
                    viewport,
                    Self::pointer_input(ui.ctx()),
                    &self.ui_theme,
                );
                let mut backend = EguiUiBackend::new(ui.painter());
                render_commands(
                    &mut backend,
                    viewport,
                    ui.ctx().pixels_per_point(),
                    &frame.paint_commands,
                    |_| texture_id,
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
            if let Some((game_object, c_camera)) = scene.main_camera() {
                let transform = scene.world_transform(game_object);

                let camera = Camera::new(
                    rect.aspect_ratio(),
                    c_camera.fov,
                    c_camera.near_plane,
                    c_camera.far_plane,
                );
                renderer.render_scene_base(render_state, &camera, &transform, scene, None);
                renderer.finalize_scene(render_state);
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
    eframe::run_native(
        "Calyx",
        options,
        Box::new(|cc| Ok(GameApp::new(cc).map(Box::new)?)),
    )
}
