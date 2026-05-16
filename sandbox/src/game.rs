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
            .radius(ui, CornerRadius::none())
            .width(ui, UiLength::Fill)
            .height(ui, UiLength::Px(14.0))
    }

    fn line(start: UiPoint, end: UiPoint, color: UiColor, width: f32) -> PaintCommand {
        PaintCommand::Line {
            start,
            end,
            color,
            width,
        }
    }

    fn push_panel_frame(
        commands: &mut Vec<PaintCommand>,
        rect: UiRect,
        primary: UiColor,
        danger: UiColor,
        dim: UiColor,
    ) {
        let x = rect.min.x;
        let y = rect.min.y;
        let w = rect.width();
        let h = rect.height();
        let right = x + w;
        let bottom = y + h;

        commands.extend([
            Self::line(UiPoint::new(x, y + 22.0), UiPoint::new(x, y), primary, 2.0),
            Self::line(UiPoint::new(x, y), UiPoint::new(x + 42.0, y), primary, 2.0),
            Self::line(
                UiPoint::new(right - 86.0, y),
                UiPoint::new(right - 18.0, y),
                primary,
                2.0,
            ),
            Self::line(
                UiPoint::new(right - 18.0, y),
                UiPoint::new(right, y + 18.0),
                primary,
                2.0,
            ),
            Self::line(
                UiPoint::new(right, y + 18.0),
                UiPoint::new(right, y + 46.0),
                primary,
                2.0,
            ),
            Self::line(
                UiPoint::new(x, bottom - 36.0),
                UiPoint::new(x, bottom),
                dim,
                1.0,
            ),
            Self::line(
                UiPoint::new(x, bottom),
                UiPoint::new(x + 70.0, bottom),
                dim,
                1.0,
            ),
            Self::line(
                UiPoint::new(right - 56.0, bottom),
                UiPoint::new(right, bottom),
                danger,
                2.0,
            ),
            Self::line(
                UiPoint::new(right, bottom - 24.0),
                UiPoint::new(right, bottom),
                danger,
                2.0,
            ),
            Self::line(
                UiPoint::new(x + 18.0, y + 7.0),
                UiPoint::new(x + 118.0, y + 7.0),
                dim,
                1.0,
            ),
            Self::line(
                UiPoint::new(x + 20.0, bottom - 9.0),
                UiPoint::new(right - 90.0, bottom - 9.0),
                dim,
                1.0,
            ),
        ]);
    }

    fn panel_frame_node(
        ui: &mut UiArena,
        id: &'static str,
        viewport: UiRect,
        rect: UiRect,
        primary: UiColor,
        danger: UiColor,
        dim: UiColor,
        hover_style: StylePatch,
    ) -> UiNodeHandle {
        let mut commands = Vec::new();
        Self::push_panel_frame(&mut commands, rect, primary, danger, dim);
        ui.custom_paint(commands)
            .id(ui, id)
            .style(
                ui,
                StylePatch::default()
                    .width(UiLength::Px(rect.width()))
                    .height(UiLength::Px(rect.height()))
                    .margin(EdgeInsets {
                        top: rect.min.y - viewport.min.y,
                        right: 0.0,
                        bottom: 0.0,
                        left: rect.min.x - viewport.min.x,
                    })
                    .transition_duration(0.18),
            )
            .hover_style(ui, hover_style)
            .pointer_events(ui, PointerEvents::None)
    }

    fn hud_overview_overlay(
        viewport: UiRect,
        primary: UiColor,
        bright: UiColor,
        dim: UiColor,
    ) -> Vec<PaintCommand> {
        let mut commands = Vec::new();
        let center_x = viewport.min.x + viewport.width() * 0.5;
        let center_y = viewport.min.y + viewport.height() * 0.5;
        commands.extend([
            Self::line(
                UiPoint::new(center_x - 122.0, center_y),
                UiPoint::new(center_x - 48.0, center_y),
                bright,
                2.0,
            ),
            Self::line(
                UiPoint::new(center_x + 48.0, center_y),
                UiPoint::new(center_x + 122.0, center_y),
                bright,
                2.0,
            ),
            Self::line(
                UiPoint::new(center_x, center_y - 92.0),
                UiPoint::new(center_x, center_y - 48.0),
                primary,
                2.0,
            ),
            Self::line(
                UiPoint::new(center_x, center_y + 48.0),
                UiPoint::new(center_x, center_y + 92.0),
                primary,
                2.0,
            ),
            Self::line(
                UiPoint::new(center_x - 24.0, center_y - 24.0),
                UiPoint::new(center_x - 9.0, center_y - 36.0),
                dim,
                1.0,
            ),
            Self::line(
                UiPoint::new(center_x + 24.0, center_y + 24.0),
                UiPoint::new(center_x + 9.0, center_y + 36.0),
                dim,
                1.0,
            ),
            Self::line(
                UiPoint::new(viewport.min.x + 26.0, viewport.min.y + 206.0),
                UiPoint::new(viewport.min.x + 278.0, viewport.min.y + 206.0),
                dim,
                1.0,
            ),
            Self::line(
                UiPoint::new(viewport.min.x + 26.0, viewport.min.y + 214.0),
                UiPoint::new(viewport.min.x + 174.0, viewport.min.y + 214.0),
                primary,
                1.0,
            ),
            Self::line(
                UiPoint::new(viewport.max.x - 286.0, viewport.min.y + 58.0),
                UiPoint::new(viewport.max.x - 94.0, viewport.min.y + 58.0),
                bright,
                1.0,
            ),
            Self::line(
                UiPoint::new(viewport.max.x - 94.0, viewport.min.y + 58.0),
                UiPoint::new(viewport.max.x - 64.0, viewport.min.y + 88.0),
                dim,
                1.0,
            ),
        ]);
        commands
    }

    fn sandbox_ui(ui: &mut UiArena, viewport: UiRect, fps: usize) -> UiNodeHandle {
        let panel = UiColor::rgba(10, 10, 10, 190);
        let panel_strong = UiColor::rgba(8, 8, 8, 226);
        let primary = UiColor::rgba(232, 232, 228, 236);
        let bright = UiColor::rgba(255, 255, 252, 255);
        let dim = UiColor::rgba(138, 138, 136, 172);
        let muted = UiColor::rgba(170, 170, 168, 232);
        let black = UiColor::rgba(6, 6, 6, 244);
        let green = UiColor::rgba(83, 191, 84, 238);
        let compact = ScreenClass::from_width(viewport.width()) == ScreenClass::Compact;
        let status_size = UiSize::new(if compact { 210.0 } else { 240.0 }, 88.0);
        let prompt_size = UiSize::new(if compact { 232.0 } else { 272.0 }, 42.0);
        let vitals_size = UiSize::new(if compact { 276.0 } else { 318.0 }, 116.0);
        let progress_size = UiSize::new(292.0, 54.0);
        let speed_size = UiSize::new(156.0, 112.0);
        let radio_size = UiSize::new(if compact { 264.0 } else { 296.0 }, 154.0);

        let status_rect = UiRect::from_min_size(
            UiPoint::new(viewport.min.x + 18.0, viewport.min.y + 20.0),
            status_size,
        );
        let prompt_rect = UiRect::from_min_size(
            UiPoint::new(
                viewport.min.x + viewport.width() * 0.5 - prompt_size.width * 0.5,
                viewport.min.y + 24.0,
            ),
            prompt_size,
        );
        let vitals_rect = UiRect::from_min_size(
            UiPoint::new(
                viewport.min.x + 24.0,
                viewport.min.y + viewport.height() - vitals_size.height - 28.0,
            ),
            vitals_size,
        );
        let progress_rect = UiRect::from_min_size(
            UiPoint::new(
                viewport.min.x + viewport.width() * 0.5 - progress_size.width * 0.5,
                viewport.min.y + viewport.height() - progress_size.height - 26.0,
            ),
            progress_size,
        );
        let speed_rect = UiRect::from_min_size(
            UiPoint::new(
                viewport.min.x + viewport.width() - speed_size.width - 34.0,
                viewport.min.y + viewport.height() - speed_size.height - 36.0,
            ),
            speed_size,
        );
        let radio_rect = UiRect::from_min_size(
            UiPoint::new(
                viewport.min.x + viewport.width() - radio_size.width - 34.0,
                viewport.min.y + 92.0,
            ),
            radio_size,
        );

        let panel_style = StylePatch::default()
            .background(panel)
            .border(Border::solid(dim, 1.0))
            .radius(CornerRadius::none())
            .padding(EdgeInsets::all(10.0))
            .gap(6.0)
            .transition_duration(0.18);
        let hover_tilt = StylePatch::default().transform(UiTransform::tilt_degrees(3.0, -7.0));
        let reverse_hover_tilt =
            StylePatch::default().transform(UiTransform::tilt_degrees(-3.0, 8.0));

        let brand = Self::hud_text(ui, "hud-brand", "CALYX", bright, 18.0);
        let id_line = Self::hud_text(ui, "hud-id", "ID 0323    20:23", muted, 11.0);
        let fps_line = Self::hud_text(ui, "hud-fps", format!("{fps:03} FPS"), dim, 11.0);
        let status_panel = ui
            .column()
            .id(ui, "status-panel")
            .style(ui, panel_style.clone())
            .hover_style(ui, hover_tilt.clone())
            .pointer_events(ui, PointerEvents::Auto)
            .width(ui, UiLength::Px(status_size.width))
            .height(ui, UiLength::Px(status_size.height))
            .margin(
                ui,
                EdgeInsets {
                    top: status_rect.min.y - viewport.min.y,
                    right: 0.0,
                    bottom: 0.0,
                    left: status_rect.min.x - viewport.min.x,
                },
            )
            .child(ui, brand)
            .child(ui, id_line)
            .child(ui, fps_line);

        let key_text = Self::hud_text(ui, "prompt-key-text", "E", bright, 15.0);
        let key_box = ui
            .container()
            .id(ui, "prompt-key")
            .background(ui, black)
            .border(ui, Border::solid(bright, 1.0))
            .radius(ui, CornerRadius::none())
            .padding(ui, EdgeInsets::symmetric(9.0, 4.0))
            .width(ui, UiLength::Px(32.0))
            .height(ui, UiLength::Px(30.0))
            .child(ui, key_text);
        let prompt_title = Self::hud_text(ui, "prompt-title", "GARAGE", bright, 12.0);
        let prompt_hint =
            Self::hud_text(ui, "prompt-hint", "Press [E] to open garage", muted, 10.0);
        let prompt_copy = ui
            .column()
            .id(ui, "prompt-copy")
            .gap(ui, 0.0)
            .child(ui, prompt_title)
            .child(ui, prompt_hint);
        let prompt_panel = ui
            .row()
            .id(ui, "prompt-panel")
            .style(
                ui,
                StylePatch::default()
                    .background(panel_strong)
                    .border(Border::solid(dim, 1.0))
                    .radius(CornerRadius::none())
                    .padding(EdgeInsets::symmetric(8.0, 6.0))
                    .transition_duration(0.18),
            )
            .hover_style(ui, reverse_hover_tilt.clone())
            .pointer_events(ui, PointerEvents::Auto)
            .width(ui, UiLength::Px(prompt_size.width))
            .height(ui, UiLength::Px(prompt_size.height))
            .margin(
                ui,
                EdgeInsets {
                    top: prompt_rect.min.y - viewport.min.y,
                    right: 0.0,
                    bottom: 0.0,
                    left: prompt_rect.min.x - viewport.min.x,
                },
            )
            .gap(ui, 8.0)
            .child(ui, key_box)
            .child(ui, prompt_copy);

        let hp_label = Self::hud_text(ui, "hp-label", "HP", bright, 12.0);
        let hp_meter = Self::meter(
            ui,
            "hp-meter",
            1.0,
            bright,
            UiColor::rgba(32, 32, 32, 210),
            dim,
        );
        let shield_label = Self::hud_text(ui, "shield-label", "SHIELD", muted, 12.0);
        let shield_meter = Self::meter(
            ui,
            "shield-meter",
            0.58,
            bright,
            UiColor::rgba(32, 32, 32, 210),
            dim,
        );
        let cash_label = Self::hud_text(ui, "cash-label", "CASH  58", green, 12.0);
        let vitals_panel = ui
            .column()
            .id(ui, "vitals-panel")
            .style(ui, panel_style.clone())
            .hover_style(ui, hover_tilt.clone())
            .pointer_events(ui, PointerEvents::Auto)
            .width(ui, UiLength::Px(vitals_size.width))
            .height(ui, UiLength::Px(vitals_size.height))
            .margin(
                ui,
                EdgeInsets {
                    top: vitals_rect.min.y - viewport.min.y,
                    right: 0.0,
                    bottom: 0.0,
                    left: vitals_rect.min.x - viewport.min.x,
                },
            )
            .child(ui, hp_label)
            .child(ui, hp_meter)
            .child(ui, shield_label)
            .child(ui, shield_meter)
            .child(ui, cash_label);

        let notification_title =
            Self::hud_text(ui, "notification-title", "NOTIFICATION", bright, 12.0);
        let notification_copy = Self::hud_text(
            ui,
            "notification-copy",
            "You have been hired as a driver",
            muted,
            10.0,
        );
        let radio_title = Self::hud_text(ui, "radio-title", "Voice channel #1", bright, 11.0);
        let radio_one = Self::hud_text(ui, "radio-one", "Victoria Adams", muted, 10.0);
        let radio_two = Self::hud_text(ui, "radio-two", "Aiden Smith", muted, 10.0);
        let radio_three = Self::hud_text(ui, "radio-three", "Carl John", muted, 10.0);
        let radio_panel = ui
            .column()
            .id(ui, "radio-panel")
            .style(ui, panel_style.clone())
            .hover_style(ui, reverse_hover_tilt.clone())
            .pointer_events(ui, PointerEvents::Auto)
            .width(ui, UiLength::Px(radio_size.width))
            .height(ui, UiLength::Px(radio_size.height))
            .margin(
                ui,
                EdgeInsets {
                    top: radio_rect.min.y - viewport.min.y,
                    right: 0.0,
                    bottom: 0.0,
                    left: radio_rect.min.x - viewport.min.x,
                },
            )
            .child(ui, notification_title)
            .child(ui, notification_copy)
            .child(ui, radio_title)
            .child(ui, radio_one)
            .child(ui, radio_two)
            .child(ui, radio_three);

        let street_key = Self::hud_text(ui, "street-key", "NE", bright, 13.0);
        let street_name = Self::hud_text(ui, "street-name", "Burton", bright, 12.0);
        let street_sub = Self::hud_text(ui, "street-sub", "Abe Milton Parkway", muted, 10.0);
        let street_copy = ui
            .column()
            .id(ui, "street-copy")
            .gap(ui, 0.0)
            .child(ui, street_name)
            .child(ui, street_sub);
        let street_card = ui
            .row()
            .id(ui, "street-card")
            .style(
                ui,
                StylePatch::default()
                    .background(panel_strong)
                    .border(Border::solid(dim, 1.0))
                    .radius(CornerRadius::none())
                    .padding(EdgeInsets::symmetric(8.0, 6.0)),
            )
            .gap(ui, 8.0)
            .width(ui, UiLength::Px(150.0))
            .height(ui, UiLength::Px(42.0))
            .margin(
                ui,
                EdgeInsets {
                    top: viewport.height() - 172.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: viewport.width() * 0.5 - 75.0,
                },
            )
            .child(ui, street_key)
            .child(ui, street_copy);

        let progress_label = Self::hud_text(ui, "progress-label", "LOADING...", muted, 10.0);
        let loading_bar = Self::meter(
            ui,
            "loading-meter",
            0.72,
            bright,
            UiColor::rgba(34, 34, 34, 220),
            dim,
        );
        let progress_panel = ui
            .column()
            .id(ui, "progress-panel")
            .style(ui, panel_style.clone())
            .hover_style(ui, reverse_hover_tilt.clone())
            .pointer_events(ui, PointerEvents::Auto)
            .width(ui, UiLength::Px(progress_size.width))
            .height(ui, UiLength::Px(progress_size.height))
            .margin(
                ui,
                EdgeInsets {
                    top: progress_rect.min.y - viewport.min.y,
                    right: 0.0,
                    bottom: 0.0,
                    left: progress_rect.min.x - viewport.min.x,
                },
            )
            .child(ui, progress_label)
            .child(ui, loading_bar);

        let speed_value = Self::hud_text(ui, "speed-value", "173", bright, 36.0)
            .width(ui, UiLength::Px(78.0))
            .height(ui, UiLength::Px(44.0));
        let speed_unit = Self::hud_text(ui, "speed-unit", "km/h", muted, 13.0);
        let speed_meta = Self::hud_text(ui, "speed-meta", "0.7.3.5.12", dim, 10.0);
        let speed_panel = ui
            .column()
            .id(ui, "speed-panel")
            .style(ui, panel_style)
            .hover_style(ui, hover_tilt.clone())
            .pointer_events(ui, PointerEvents::Auto)
            .width(ui, UiLength::Px(speed_size.width))
            .height(ui, UiLength::Px(speed_size.height))
            .margin(
                ui,
                EdgeInsets {
                    top: speed_rect.min.y - viewport.min.y,
                    right: 0.0,
                    bottom: 0.0,
                    left: speed_rect.min.x - viewport.min.x,
                },
            )
            .child(ui, speed_value)
            .child(ui, speed_unit)
            .child(ui, speed_meta);

        let crosshair = ui
            .crosshair(bright, 28.0)
            .id(ui, "crosshair")
            .width(ui, UiLength::Px(28.0))
            .height(ui, UiLength::Px(28.0));
        let crosshair_center = ui
            .center(crosshair)
            .id(ui, "crosshair-center")
            .width(ui, UiLength::Px(viewport.width()))
            .height(ui, UiLength::Px(viewport.height()))
            .pointer_events(ui, PointerEvents::None);

        let status_frame = Self::panel_frame_node(
            ui,
            "status-panel-frame",
            viewport,
            status_rect,
            bright,
            primary,
            dim,
            hover_tilt.clone(),
        );
        let prompt_frame = Self::panel_frame_node(
            ui,
            "prompt-panel-frame",
            viewport,
            prompt_rect,
            bright,
            primary,
            dim,
            reverse_hover_tilt.clone(),
        );
        let vitals_frame = Self::panel_frame_node(
            ui,
            "vitals-panel-frame",
            viewport,
            vitals_rect,
            bright,
            primary,
            dim,
            hover_tilt.clone(),
        );
        let radio_frame = Self::panel_frame_node(
            ui,
            "radio-panel-frame",
            viewport,
            radio_rect,
            bright,
            primary,
            dim,
            reverse_hover_tilt.clone(),
        );
        let progress_frame = Self::panel_frame_node(
            ui,
            "progress-panel-frame",
            viewport,
            progress_rect,
            bright,
            primary,
            dim,
            reverse_hover_tilt,
        );
        let speed_frame = Self::panel_frame_node(
            ui,
            "speed-panel-frame",
            viewport,
            speed_rect,
            bright,
            primary,
            dim,
            hover_tilt,
        );
        let overlay = ui
            .custom_paint(Self::hud_overview_overlay(viewport, primary, bright, dim))
            .id(ui, "hud-overview-overlay")
            .pointer_events(ui, PointerEvents::None);

        ui.stack()
            .id(ui, "sandbox-ui")
            .width(ui, UiLength::Px(viewport.width()))
            .height(ui, UiLength::Px(viewport.height()))
            .pointer_events(ui, PointerEvents::None)
            .child(ui, status_panel)
            .child(ui, prompt_panel)
            .child(ui, vitals_panel)
            .child(ui, radio_panel)
            .child(ui, street_card)
            .child(ui, progress_panel)
            .child(ui, speed_panel)
            .child(ui, status_frame)
            .child(ui, prompt_frame)
            .child(ui, vitals_frame)
            .child(ui, radio_frame)
            .child(ui, progress_frame)
            .child(ui, speed_frame)
            .child(ui, overlay)
            .child(ui, crosshair_center)
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
