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
    render_commands, Border, CornerRadius, EdgeInsets, EguiUiBackend, JustifyContent,
    PointerEvents, StylePatch, Theme, UiArena, UiColor, UiInput, UiLength, UiNodeHandle, UiPoint,
    UiRect, UiRuntime, UiSize, Widget,
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

    fn sandbox_ui(ui: &mut UiArena, viewport: UiRect) -> UiNodeHandle {
        let effects = [
            StatusEffectHud {
                name: "Armor",
                stacks: 2,
                progress: 0.64,
            },
            StatusEffectHud {
                name: "Reload",
                stacks: 1,
                progress: 0.38,
            },
        ];
        SandboxHud {
            viewport,
            style: HudStyle::default(),
            health: 82.0,
            max_health: 100.0,
            weapon_name: "Cannon",
            ammo_loaded: 5,
            ammo_capacity: 6,
            ammo_reserve: 24,
            effects: &effects,
        }
        .build(ui)
    }
}

#[derive(Clone, Copy)]
struct HudStyle {
    screen_margin: f32,
    panel: UiColor,
    panel_strong: UiColor,
    outline: UiColor,
    text: UiColor,
    muted: UiColor,
    health: UiColor,
    ammo: UiColor,
    effect: UiColor,
    bar_background: UiColor,
}

impl Default for HudStyle {
    fn default() -> Self {
        Self {
            screen_margin: 28.0,
            panel: UiColor::rgba(10, 12, 14, 205),
            panel_strong: UiColor::rgba(8, 10, 12, 235),
            outline: UiColor::rgba(132, 142, 146, 185),
            text: UiColor::rgba(244, 244, 238, 245),
            muted: UiColor::rgba(170, 174, 170, 210),
            health: UiColor::rgba(92, 205, 110, 245),
            ammo: UiColor::rgba(238, 217, 132, 245),
            effect: UiColor::rgba(118, 178, 245, 235),
            bar_background: UiColor::rgba(34, 36, 38, 220),
        }
    }
}

impl HudStyle {
    fn margin(self, viewport: UiRect) -> f32 {
        self.screen_margin.min(viewport.width() * 0.04).max(18.0)
    }

    fn panel_style(self) -> StylePatch {
        StylePatch::default()
            .background(self.panel)
            .border(Border::solid(self.outline, 1.0))
            .radius(CornerRadius::all(3.0))
            .padding(EdgeInsets::all(10.0))
            .gap(7.0)
            .pointer_events(PointerEvents::None)
    }

    fn label_style(self, color: UiColor) -> StylePatch {
        StylePatch::default()
            .text_color(color)
            .font_size(11.0)
            .pointer_events(PointerEvents::None)
    }

    fn value_style(self, color: UiColor, size: f32) -> StylePatch {
        StylePatch::default()
            .text_color(color)
            .font_size(size)
            .pointer_events(PointerEvents::None)
    }
}

struct SandboxHud<'a> {
    viewport: UiRect,
    style: HudStyle,
    health: f32,
    max_health: f32,
    weapon_name: &'a str,
    ammo_loaded: u32,
    ammo_capacity: u32,
    ammo_reserve: u32,
    effects: &'a [StatusEffectHud<'a>],
}

impl Widget for SandboxHud<'_> {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let margin = self.style.margin(self.viewport);
        let health_size = UiSize::new((self.viewport.width() * 0.28).clamp(250.0, 340.0), 76.0);
        let ammo_size = UiSize::new((self.viewport.width() * 0.22).clamp(220.0, 300.0), 80.0);
        let effects_size = UiSize::new((self.viewport.width() * 0.24).clamp(230.0, 310.0), 124.0);

        let health = HealthHud {
            id: "hud-health",
            style: self.style,
            current: self.health,
            max: self.max_health,
            size: health_size,
            margin: EdgeInsets {
                top: self.viewport.height() - health_size.height - margin,
                right: 0.0,
                bottom: 0.0,
                left: margin,
            },
        }
        .build(ui);
        let ammo = AmmoHud {
            id: "hud-ammo",
            style: self.style,
            weapon_name: self.weapon_name,
            loaded: self.ammo_loaded,
            capacity: self.ammo_capacity,
            reserve: self.ammo_reserve,
            size: ammo_size,
            margin: EdgeInsets {
                top: self.viewport.height() - ammo_size.height - margin,
                right: 0.0,
                bottom: 0.0,
                left: (self.viewport.width() - ammo_size.width - margin).max(margin),
            },
        }
        .build(ui);
        let effects = StatusEffectsHud {
            id: "hud-effects",
            style: self.style,
            effects: self.effects,
            size: effects_size,
            margin: EdgeInsets {
                top: margin,
                right: 0.0,
                bottom: 0.0,
                left: (self.viewport.width() - effects_size.width - margin).max(margin),
            },
        }
        .build(ui);

        ui.stack()
            .id(ui, "sandbox-ui")
            .width(ui, UiLength::Px(self.viewport.width()))
            .height(ui, UiLength::Px(self.viewport.height()))
            .pointer_events(ui, PointerEvents::None)
            .children(ui, [health, ammo, effects])
    }
}

struct HealthHud {
    id: &'static str,
    style: HudStyle,
    current: f32,
    max: f32,
    size: UiSize,
    margin: EdgeInsets,
}

impl Widget for HealthHud {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let ratio = if self.max > 0.0 {
            self.current / self.max
        } else {
            0.0
        }
        .clamp(0.0, 1.0);
        let label = hud_text(
            ui,
            "hud-health-label",
            "HULL INTEGRITY",
            self.style.label_style(self.style.muted),
        );
        let value = hud_text(
            ui,
            "hud-health-value",
            format!(
                "{:03}/{:03}",
                self.current.max(0.0) as u32,
                self.max.max(0.0) as u32
            ),
            self.style.value_style(self.style.text, 16.0),
        );
        let header = ui
            .row()
            .id(ui, "hud-health-header")
            .justify_content(ui, JustifyContent::SpaceBetween)
            .pointer_events(ui, PointerEvents::None)
            .child(ui, label)
            .child(ui, value);
        let meter = progress_meter(
            ui,
            "hud-health-meter",
            ratio,
            self.style.health,
            self.style.bar_background,
            self.style.outline,
        );

        ui.column()
            .id(ui, self.id)
            .style(ui, self.style.panel_style())
            .width(ui, UiLength::Px(self.size.width))
            .height(ui, UiLength::Px(self.size.height))
            .margin(ui, self.margin)
            .child(ui, header)
            .child(ui, meter)
    }
}

struct AmmoHud<'a> {
    id: &'static str,
    style: HudStyle,
    weapon_name: &'a str,
    loaded: u32,
    capacity: u32,
    reserve: u32,
    size: UiSize,
    margin: EdgeInsets,
}

impl Widget for AmmoHud<'_> {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let label = hud_text(
            ui,
            "hud-ammo-label",
            self.weapon_name.to_uppercase(),
            self.style.label_style(self.style.muted),
        );
        let ammo = hud_text(
            ui,
            "hud-ammo-value",
            format!("{}/{}", self.loaded, self.capacity),
            self.style.value_style(self.style.ammo, 24.0),
        );
        let reserve = hud_text(
            ui,
            "hud-ammo-reserve",
            format!("RESERVE {}", self.reserve),
            self.style.label_style(self.style.text),
        );
        let loaded_ratio = if self.capacity > 0 {
            self.loaded as f32 / self.capacity as f32
        } else {
            0.0
        };
        let meter = progress_meter(
            ui,
            "hud-ammo-meter",
            loaded_ratio,
            self.style.ammo,
            self.style.bar_background,
            self.style.outline,
        );

        ui.column()
            .id(ui, self.id)
            .style(
                ui,
                self.style.panel_style().background(self.style.panel_strong),
            )
            .width(ui, UiLength::Px(self.size.width))
            .height(ui, UiLength::Px(self.size.height))
            .margin(ui, self.margin)
            .child(ui, label)
            .child(ui, ammo)
            .child(ui, reserve)
            .child(ui, meter)
    }
}

struct StatusEffectHud<'a> {
    name: &'a str,
    stacks: u8,
    progress: f32,
}

struct StatusEffectsHud<'a> {
    id: &'static str,
    style: HudStyle,
    effects: &'a [StatusEffectHud<'a>],
    size: UiSize,
    margin: EdgeInsets,
}

impl Widget for StatusEffectsHud<'_> {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let title = hud_text(
            ui,
            "hud-effects-title",
            "STATUS",
            self.style.label_style(self.style.muted),
        );
        let mut panel = ui
            .column()
            .id(ui, self.id)
            .style(ui, self.style.panel_style())
            .width(ui, UiLength::Px(self.size.width))
            .height(ui, UiLength::Px(self.size.height))
            .margin(ui, self.margin)
            .child(ui, title);

        if self.effects.is_empty() {
            let clear = hud_text(
                ui,
                "hud-effects-clear",
                "CLEAR",
                self.style.value_style(self.style.text, 14.0),
            );
            panel = panel.child(ui, clear);
        } else {
            for (index, effect) in self.effects.iter().enumerate() {
                let row = effect_row(ui, self.style, index, effect);
                panel = panel.child(ui, row);
            }
        }

        panel
    }
}

fn hud_text(
    ui: &mut UiArena,
    id: impl Into<String>,
    value: impl Into<String>,
    style: StylePatch,
) -> UiNodeHandle {
    ui.text(value)
        .id(ui, id.into())
        .style(ui, style)
        .pointer_events(ui, PointerEvents::None)
}

fn progress_meter(
    ui: &mut UiArena,
    id: impl Into<String>,
    value: f32,
    fill: UiColor,
    background: UiColor,
    outline: UiColor,
) -> UiNodeHandle {
    ui.progress_bar(value, fill)
        .id(ui, id.into())
        .background(ui, background)
        .border(ui, Border::solid(outline, 1.0))
        .radius(ui, CornerRadius::all(2.0))
        .width(ui, UiLength::Fill)
        .height(ui, UiLength::Px(12.0))
        .pointer_events(ui, PointerEvents::None)
}

fn effect_row(
    ui: &mut UiArena,
    style: HudStyle,
    index: usize,
    effect: &StatusEffectHud<'_>,
) -> UiNodeHandle {
    let name = hud_text(
        ui,
        format!("hud-effect-{index}-name"),
        effect.name.to_uppercase(),
        style.value_style(style.text, 12.0),
    );
    let stacks = hud_text(
        ui,
        format!("hud-effect-{index}-stacks"),
        format!("x{}", effect.stacks.max(1)),
        style.label_style(style.effect),
    );
    let header = ui
        .row()
        .id(ui, format!("hud-effect-{index}-header"))
        .justify_content(ui, JustifyContent::SpaceBetween)
        .pointer_events(ui, PointerEvents::None)
        .child(ui, name)
        .child(ui, stacks);
    let timer = progress_meter(
        ui,
        format!("hud-effect-{index}-timer"),
        effect.progress,
        style.effect,
        style.bar_background,
        style.outline,
    );

    ui.column()
        .id(ui, format!("hud-effect-{index}"))
        .gap(ui, 3.0)
        .pointer_events(ui, PointerEvents::None)
        .child(ui, header)
        .child(ui, timer)
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
                rect = response.rect;

                let viewport = UiRect::from_min_size(
                    UiPoint::new(rect.min.x, rect.min.y),
                    UiSize::new(rect.width(), rect.height()),
                );
                self.ui_arena.clear();
                let sandbox_ui = Self::sandbox_ui(&mut self.ui_arena, viewport);
                let ui_frame = self.ui_runtime.frame(
                    &self.ui_arena,
                    sandbox_ui,
                    viewport,
                    Self::pointer_input(ui.ctx()),
                    &self.ui_theme,
                );

                let state = InputState {
                    is_active: !ui_frame.consumed_pointer,
                    last_cursor_pos: None,
                    ..Default::default()
                };
                let input = Input::from_ctx(ui.ctx(), Some(&response), state);
                let assets = self.game.assets.lock_read();
                let GameContext {
                    scenes, resources, ..
                } = &mut self.game;
                let scene = scenes.current_scene_mut();
                scene.prepare();
                scene.update(&assets.registries, resources, &input);

                let mut backend = EguiUiBackend::new(ui.painter());
                render_commands(
                    &mut backend,
                    viewport,
                    ui.ctx().pixels_per_point(),
                    &ui_frame.paint_commands,
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
