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
    button, center, column, container, crosshair, custom_paint, image, progress_bar,
    render_commands, row, sized_box, spacer, stack, text, Border, CornerRadius, EdgeInsets,
    EguiUiBackend, PaintCommand, PointerEvents, ScreenClass, StylePatch, Theme, UiColor, UiInput,
    UiLength, UiPoint, UiRect, UiRuntime, UiSize,
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
        })
    }

    fn sandbox_ui(viewport: UiRect, fps: usize) -> engine::ui::UiNode {
        let panel = UiColor::rgba(11, 15, 18, 220);
        let panel_hover = UiColor::rgba(30, 42, 48, 235);
        let outline = UiColor::rgba(116, 136, 145, 180);
        let text_primary = UiColor::rgba(238, 244, 247, 255);
        let text_muted = UiColor::rgba(158, 176, 184, 255);
        let health = UiColor::rgba(87, 201, 132, 255);
        let ammo = UiColor::rgba(86, 162, 255, 255);
        let warning = UiColor::rgba(255, 192, 94, 255);

        let panel_style = StylePatch::default()
            .background(panel)
            .border(Border::solid(outline, 1.0))
            .radius(CornerRadius::all(8.0))
            .padding(EdgeInsets::all(12.0))
            .gap(8.0);

        let hover_style = StylePatch::default().background(panel_hover);

        stack()
            .id("sandbox-ui")
            .width(UiLength::Px(viewport.width()))
            .height(UiLength::Px(viewport.height()))
            .pointer_events(PointerEvents::None)
            .child(
                column()
                    .id("status-panel")
                    .style(panel_style.clone())
                    .hover_style(hover_style.clone())
                    .pointer_events(PointerEvents::Auto)
                    .width(UiLength::Px(320.0))
                    .height(UiLength::Px(196.0))
                    .when(
                        ScreenClass::Compact,
                        StylePatch::default().width(UiLength::Px(260.0)),
                    )
                    .child(
                        row()
                            .id("status-row")
                            .gap(10.0)
                            .height(UiLength::Px(28.0))
                            .child(
                                text("Calyx Runtime UI")
                                    .id("title")
                                    .style(
                                        StylePatch::default()
                                            .text_color(text_primary)
                                            .font_size(18.0),
                                    )
                                    .width(UiLength::Px(190.0)),
                            )
                            .child(spacer().id("title-spacer"))
                            .child(
                                container()
                                    .id("fps-pill")
                                    .background(UiColor::rgba(33, 45, 50, 255))
                                    .radius(CornerRadius::all(12.0))
                                    .padding(EdgeInsets::symmetric(8.0, 3.0))
                                    .width(UiLength::Px(70.0))
                                    .height(UiLength::Px(24.0))
                                    .child(
                                        text(format!("{fps} fps"))
                                            .style(
                                                StylePatch::default()
                                                    .text_color(text_muted)
                                                    .font_size(13.0),
                                            )
                                            .id("fps-text"),
                                    ),
                            ),
                    )
                    .child(
                        column()
                            .id("meters")
                            .gap(6.0)
                            .height(UiLength::Px(56.0))
                            .child(
                                progress_bar(0.76, health)
                                    .id("health-bar")
                                    .background(UiColor::rgba(42, 50, 45, 255))
                                    .border(Border::solid(UiColor::rgba(98, 128, 108, 180), 1.0))
                                    .radius(CornerRadius::all(5.0))
                                    .width(UiLength::Fill)
                                    .height(UiLength::Px(16.0)),
                            )
                            .child(
                                progress_bar(0.42, ammo)
                                    .id("ammo-bar")
                                    .background(UiColor::rgba(37, 44, 54, 255))
                                    .border(Border::solid(UiColor::rgba(86, 128, 184, 180), 1.0))
                                    .radius(CornerRadius::all(5.0))
                                    .width(UiLength::Fill)
                                    .height(UiLength::Px(16.0)),
                            ),
                    )
                    .child(
                        row()
                            .id("button-row")
                            .gap(8.0)
                            .height(UiLength::Px(36.0))
                            .child(
                                button("Hover")
                                    .id("hover-button")
                                    .background(UiColor::rgba(35, 47, 53, 255))
                                    .border(Border::solid(outline, 1.0))
                                    .radius(CornerRadius::all(6.0))
                                    .padding(EdgeInsets::symmetric(10.0, 6.0))
                                    .hover_style(
                                        StylePatch::default()
                                            .background(UiColor::rgba(50, 72, 81, 255)),
                                    )
                                    .pressed_style(
                                        StylePatch::default()
                                            .background(UiColor::rgba(67, 96, 108, 255)),
                                    ),
                            )
                            .child(
                                button("Pressed")
                                    .id("pressed-button")
                                    .background(UiColor::rgba(58, 43, 36, 255))
                                    .border(Border::solid(warning, 1.0))
                                    .radius(CornerRadius::all(6.0))
                                    .padding(EdgeInsets::symmetric(10.0, 6.0))
                                    .hover_style(
                                        StylePatch::default()
                                            .background(UiColor::rgba(75, 56, 42, 255)),
                                    )
                                    .pressed_style(
                                        StylePatch::default()
                                            .background(UiColor::rgba(95, 66, 42, 255)),
                                    ),
                            ),
                    )
                    .child(
                        container()
                            .id("clip-demo")
                            .background(UiColor::rgba(20, 25, 28, 255))
                            .border(Border::solid(UiColor::rgba(84, 96, 102, 255), 1.0))
                            .radius(CornerRadius::all(6.0))
                            .style(StylePatch::default().clip(true))
                            .width(UiLength::Fill)
                            .height(UiLength::Px(32.0))
                            .child(text("clipped text + rounded paint").id("clip-label").style(
                                StylePatch::default().text_color(text_muted).font_size(13.0),
                            )),
                    ),
            )
            .child(
                center(
                    crosshair(UiColor::rgba(238, 244, 247, 210), 28.0)
                        .id("crosshair")
                        .width(UiLength::Px(28.0))
                        .height(UiLength::Px(28.0)),
                )
                .id("crosshair-center")
                .width(UiLength::Px(viewport.width()))
                .height(UiLength::Px(viewport.height()))
                .pointer_events(PointerEvents::None),
            )
            .child(
                row()
                    .id("bottom-card")
                    .style(panel_style)
                    .hover_style(hover_style)
                    .pointer_events(PointerEvents::Auto)
                    .width(UiLength::Px(360.0))
                    .height(UiLength::Px(96.0))
                    .margin(EdgeInsets {
                        top: viewport.height() - 116.0,
                        right: 0.0,
                        bottom: 0.0,
                        left: viewport.width() - 380.0,
                    })
                    .gap(12.0)
                    .child(
                        image("scene-preview")
                            .id("image-preview")
                            .background(UiColor::rgba(28, 34, 38, 255))
                            .border(Border::solid(outline, 1.0))
                            .radius(CornerRadius::all(6.0))
                            .width(UiLength::Px(72.0))
                            .height(UiLength::Px(72.0)),
                    )
                    .child(
                        column()
                            .id("custom-column")
                            .gap(5.0)
                            .child(
                                text("Image + custom paint").id("image-title").style(
                                    StylePatch::default()
                                        .text_color(text_primary)
                                        .font_size(15.0),
                                ),
                            )
                            .child(
                                sized_box()
                                    .id("divider-box")
                                    .width(UiLength::Px(210.0))
                                    .height(UiLength::Px(8.0))
                                    .child(
                                        custom_paint(vec![PaintCommand::Line {
                                            start: UiPoint::new(
                                                viewport.width() - 276.0,
                                                viewport.height() - 62.0,
                                            ),
                                            end: UiPoint::new(
                                                viewport.width() - 66.0,
                                                viewport.height() - 62.0,
                                            ),
                                            color: warning,
                                            width: 2.0,
                                        }])
                                        .id("custom-line"),
                                    ),
                            )
                            .child(
                                text("Backend commands over egui")
                                    .id("image-caption")
                                    .style(
                                        StylePatch::default()
                                            .text_color(text_muted)
                                            .font_size(13.0),
                                    ),
                            ),
                    ),
            )
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
                let sandbox_ui = Self::sandbox_ui(viewport, self.fps);
                let frame = self.ui_runtime.frame(
                    &sandbox_ui,
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
