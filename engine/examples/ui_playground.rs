use eframe::{egui, NativeOptions};
use engine::ui::{
    render_commands, Border, CornerRadius, EdgeInsets, EguiUiBackend, JustifyContent,
    PointerEvents, StylePatch, Theme, UiArena, UiColor, UiInput, UiLength, UiNodeHandle, UiPoint,
    UiRect, UiRuntime, UiSize, Widget,
};

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([900.0, 620.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Calyx UI Playground",
        options,
        Box::new(|_cc| Ok(Box::<PlaygroundApp>::default())),
    )
}

#[derive(Default)]
struct PlaygroundApp {
    runtime: UiRuntime,
    arena: UiArena,
    theme: Theme,
    click_count: u32,
    armed: bool,
}

impl eframe::App for PlaygroundApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let viewport = UiRect::from_min_size(
                    UiPoint::new(rect.min.x, rect.min.y),
                    UiSize::new(rect.width(), rect.height()),
                );
                let time = ctx.input(|input| input.time as f32);

                self.arena.clear();
                let root = self.arena.build(PlaygroundUi {
                    viewport,
                    click_count: self.click_count,
                    armed: self.armed,
                    pulse: time.sin() * 0.5 + 0.5,
                });
                let frame = self.runtime.frame(
                    &self.arena,
                    root,
                    viewport,
                    pointer_input(ctx),
                    &self.theme,
                );

                if frame.clicked("primary-button") {
                    self.click_count = self.click_count.saturating_add(1);
                }
                if frame.clicked("toggle-button") {
                    self.armed = !self.armed;
                }

                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_rgb(9, 11, 14));
                let mut backend = EguiUiBackend::new(ui.painter());
                render_commands(
                    &mut backend,
                    viewport,
                    ctx.pixels_per_point(),
                    &frame.paint_commands,
                    |_| unreachable!("the UI playground does not use image widgets"),
                );
            });

        ctx.request_repaint();
    }
}

fn pointer_input(ctx: &egui::Context) -> UiInput {
    ctx.input(|input| UiInput {
        pointer_position: input
            .pointer
            .latest_pos()
            .map(|position| UiPoint::new(position.x, position.y)),
        pointer_down: input.pointer.primary_down(),
        delta_time: input.stable_dt,
    })
}

struct PlaygroundUi {
    viewport: UiRect,
    click_count: u32,
    armed: bool,
    pulse: f32,
}

impl Widget for PlaygroundUi {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let palette = Palette::default();
        let margin = 28.0;
        let title = TitlePanel {
            palette,
            margin: EdgeInsets {
                top: margin,
                right: 0.0,
                bottom: 0.0,
                left: margin,
            },
        }
        .build(ui);
        let controls = ControlsPanel {
            palette,
            click_count: self.click_count,
            armed: self.armed,
            margin: EdgeInsets {
                top: 150.0,
                right: 0.0,
                bottom: 0.0,
                left: margin,
            },
        }
        .build(ui);
        let meters = MeterPanel {
            palette,
            pulse: self.pulse,
            margin: EdgeInsets {
                top: margin,
                right: 0.0,
                bottom: 0.0,
                left: (self.viewport.width() - 392.0 - margin).max(margin),
            },
        }
        .build(ui);
        let composition = CompositionPanel {
            palette,
            margin: EdgeInsets {
                top: (self.viewport.height() - 210.0 - margin).max(380.0),
                right: 0.0,
                bottom: 0.0,
                left: margin,
            },
        }
        .build(ui);
        let tokens = TokensPanel {
            palette,
            margin: EdgeInsets {
                top: (self.viewport.height() - 210.0 - margin).max(380.0),
                right: 0.0,
                bottom: 0.0,
                left: (self.viewport.width() - 392.0 - margin).max(margin),
            },
        }
        .build(ui);

        ui.stack()
            .id(ui, "playground-root")
            .width(ui, UiLength::Px(self.viewport.width()))
            .height(ui, UiLength::Px(self.viewport.height()))
            .pointer_events(ui, PointerEvents::None)
            .children(ui, [title, controls, meters, composition, tokens])
    }
}

#[derive(Clone, Copy)]
struct Palette {
    panel: UiColor,
    panel_alt: UiColor,
    line: UiColor,
    text: UiColor,
    muted: UiColor,
    blue: UiColor,
    green: UiColor,
    amber: UiColor,
    red: UiColor,
    empty_bar: UiColor,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            panel: UiColor::rgba(18, 22, 27, 232),
            panel_alt: UiColor::rgba(25, 29, 35, 238),
            line: UiColor::rgba(92, 108, 122, 190),
            text: UiColor::rgba(238, 242, 246, 255),
            muted: UiColor::rgba(155, 166, 176, 230),
            blue: UiColor::rgba(94, 166, 255, 245),
            green: UiColor::rgba(84, 214, 142, 245),
            amber: UiColor::rgba(238, 190, 92, 245),
            red: UiColor::rgba(239, 95, 95, 245),
            empty_bar: UiColor::rgba(38, 43, 49, 245),
        }
    }
}

impl Palette {
    fn panel_style(self) -> StylePatch {
        StylePatch::default()
            .background(self.panel)
            .border(Border::solid(self.line, 1.0))
            .radius(CornerRadius::all(6.0))
            .padding(EdgeInsets::all(14.0))
            .gap(10.0)
    }

    fn title_text(self) -> StylePatch {
        StylePatch::default()
            .text_color(self.text)
            .font_size(26.0)
            .pointer_events(PointerEvents::None)
    }

    fn label_text(self) -> StylePatch {
        StylePatch::default()
            .text_color(self.muted)
            .font_size(12.0)
            .pointer_events(PointerEvents::None)
    }

    fn body_text(self) -> StylePatch {
        StylePatch::default()
            .text_color(self.text)
            .font_size(14.0)
            .pointer_events(PointerEvents::None)
    }
}

struct TitlePanel {
    palette: Palette,
    margin: EdgeInsets,
}

impl Widget for TitlePanel {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let title = text(
            ui,
            "title",
            "Engine UI Playground",
            self.palette.title_text(),
        );
        let subtitle = text(
            ui,
            "subtitle",
            "Struct widgets composed from engine primitives",
            self.palette.label_text(),
        );
        let badge = pill(
            ui,
            "backend-pill",
            "egui backend",
            self.palette,
            self.palette.blue,
        );

        ui.column()
            .id(ui, "title-panel")
            .style(ui, self.palette.panel_style())
            .width(ui, UiLength::Px(430.0))
            .height(ui, UiLength::Px(96.0))
            .margin(ui, self.margin)
            .pointer_events(ui, PointerEvents::None)
            .child(ui, title)
            .child(ui, subtitle)
            .child(ui, badge)
    }
}

struct ControlsPanel {
    palette: Palette,
    click_count: u32,
    armed: bool,
    margin: EdgeInsets,
}

impl Widget for ControlsPanel {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let title = text(
            ui,
            "controls-title",
            "Interactive Controls",
            self.palette.body_text(),
        );
        let primary = button(
            ui,
            "primary-button",
            "Click me",
            self.palette,
            self.palette.blue,
        );
        let toggle_label = if self.armed { "Armed" } else { "Disarmed" };
        let toggle_color = if self.armed {
            self.palette.green
        } else {
            self.palette.red
        };
        let toggle = button(
            ui,
            "toggle-button",
            toggle_label,
            self.palette,
            toggle_color,
        );
        let count = text(
            ui,
            "click-count",
            format!("Clicks recorded: {}", self.click_count),
            self.palette.label_text(),
        );

        ui.column()
            .id(ui, "controls-panel")
            .style(ui, self.palette.panel_style())
            .width(ui, UiLength::Px(300.0))
            .height(ui, UiLength::Px(196.0))
            .margin(ui, self.margin)
            .child(ui, title)
            .child(ui, primary)
            .child(ui, toggle)
            .child(ui, count)
    }
}

struct MeterPanel {
    palette: Palette,
    pulse: f32,
    margin: EdgeInsets,
}

impl Widget for MeterPanel {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let title = text(
            ui,
            "meters-title",
            "Progress Bars",
            self.palette.body_text(),
        );
        let first = meter_row(
            ui,
            "meter-a",
            "Power",
            0.78,
            self.palette.green,
            self.palette,
        );
        let second = meter_row(
            ui,
            "meter-b",
            "Cooldown",
            self.pulse,
            self.palette.blue,
            self.palette,
        );
        let third = meter_row(
            ui,
            "meter-c",
            "Warning",
            0.32,
            self.palette.amber,
            self.palette,
        );

        ui.column()
            .id(ui, "meters-panel")
            .style(ui, self.palette.panel_style())
            .width(ui, UiLength::Px(392.0))
            .height(ui, UiLength::Px(214.0))
            .margin(ui, self.margin)
            .pointer_events(ui, PointerEvents::None)
            .children(ui, [title, first, second, third])
    }
}

struct CompositionPanel {
    palette: Palette,
    margin: EdgeInsets,
}

impl Widget for CompositionPanel {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let title = text(
            ui,
            "composition-title",
            "Layout Composition",
            self.palette.body_text(),
        );
        let blue = swatch(ui, "swatch-blue", self.palette.blue);
        let green = swatch(ui, "swatch-green", self.palette.green);
        let amber = swatch(ui, "swatch-amber", self.palette.amber);
        let red = swatch(ui, "swatch-red", self.palette.red);
        let row = ui
            .row()
            .id(ui, "composition-row")
            .gap(ui, 8.0)
            .pointer_events(ui, PointerEvents::None)
            .children(ui, [blue, green, amber, red]);
        let copy = text(
            ui,
            "composition-copy",
            "Rows, columns, stack positioning, margins, padding, borders, and radius.",
            self.palette.label_text(),
        );

        ui.column()
            .id(ui, "composition-panel")
            .style(ui, self.palette.panel_style())
            .width(ui, UiLength::Px(470.0))
            .height(ui, UiLength::Px(170.0))
            .margin(ui, self.margin)
            .pointer_events(ui, PointerEvents::None)
            .child(ui, title)
            .child(ui, row)
            .child(ui, copy)
    }
}

struct TokensPanel {
    palette: Palette,
    margin: EdgeInsets,
}

impl Widget for TokensPanel {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let title = text(ui, "tokens-title", "Style Hooks", self.palette.body_text());
        let patches = text(
            ui,
            "tokens-patches",
            "StylePatch: background, border, radius, font, gaps, hover, pressed",
            self.palette.label_text(),
        );
        let hover = button(
            ui,
            "style-demo-button",
            "Hover state",
            self.palette,
            self.palette.amber,
        );

        ui.column()
            .id(ui, "tokens-panel")
            .style(
                ui,
                self.palette
                    .panel_style()
                    .background(self.palette.panel_alt),
            )
            .width(ui, UiLength::Px(392.0))
            .height(ui, UiLength::Px(170.0))
            .margin(ui, self.margin)
            .child(ui, title)
            .child(ui, patches)
            .child(ui, hover)
    }
}

fn text(
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

fn button(
    ui: &mut UiArena,
    id: &'static str,
    label: &'static str,
    palette: Palette,
    accent: UiColor,
) -> UiNodeHandle {
    ui.button(label)
        .id(ui, id)
        .style(
            ui,
            StylePatch::default()
                .background(UiColor::rgba(accent.r, accent.g, accent.b, 42))
                .border(Border::solid(accent, 1.0))
                .radius(CornerRadius::all(5.0))
                .padding(EdgeInsets::symmetric(12.0, 7.0))
                .text_color(palette.text)
                .font_size(14.0),
        )
        .hover_style(
            ui,
            StylePatch::default().background(UiColor::rgba(accent.r, accent.g, accent.b, 92)),
        )
        .pressed_style(
            ui,
            StylePatch::default().background(UiColor::rgba(accent.r, accent.g, accent.b, 145)),
        )
        .height(ui, UiLength::Px(36.0))
        .width(ui, UiLength::Px(180.0))
}

fn meter_row(
    ui: &mut UiArena,
    id: &'static str,
    label: &'static str,
    value: f32,
    fill: UiColor,
    palette: Palette,
) -> UiNodeHandle {
    let label = text(ui, format!("{id}-label"), label, palette.label_text());
    let value_label = text(
        ui,
        format!("{id}-value"),
        format!("{:03}%", (value.clamp(0.0, 1.0) * 100.0) as u32),
        palette.label_text(),
    );
    let header = ui
        .row()
        .id(ui, format!("{id}-header"))
        .justify_content(ui, JustifyContent::SpaceBetween)
        .pointer_events(ui, PointerEvents::None)
        .child(ui, label)
        .child(ui, value_label);
    let meter = ui
        .progress_bar(value, fill)
        .id(ui, format!("{id}-bar"))
        .background(ui, palette.empty_bar)
        .border(ui, Border::solid(palette.line, 1.0))
        .radius(ui, CornerRadius::all(3.0))
        .height(ui, UiLength::Px(14.0))
        .width(ui, UiLength::Fill)
        .pointer_events(ui, PointerEvents::None);

    ui.column()
        .id(ui, id)
        .gap(ui, 4.0)
        .pointer_events(ui, PointerEvents::None)
        .child(ui, header)
        .child(ui, meter)
}

fn pill(
    ui: &mut UiArena,
    id: &'static str,
    value: &'static str,
    palette: Palette,
    accent: UiColor,
) -> UiNodeHandle {
    let label = text(ui, format!("{id}-text"), value, palette.label_text());
    ui.container()
        .id(ui, id)
        .style(
            ui,
            StylePatch::default()
                .background(UiColor::rgba(accent.r, accent.g, accent.b, 48))
                .border(Border::solid(accent, 1.0))
                .radius(CornerRadius::all(12.0))
                .padding(EdgeInsets::symmetric(10.0, 3.0))
                .pointer_events(PointerEvents::None),
        )
        .width(ui, UiLength::Px(116.0))
        .height(ui, UiLength::Px(24.0))
        .child(ui, label)
}

fn swatch(ui: &mut UiArena, id: &'static str, color: UiColor) -> UiNodeHandle {
    ui.container()
        .id(ui, id)
        .background(ui, color)
        .border(ui, Border::solid(UiColor::rgba(255, 255, 255, 92), 1.0))
        .radius(ui, CornerRadius::all(4.0))
        .width(ui, UiLength::Px(42.0))
        .height(ui, UiLength::Px(42.0))
        .pointer_events(ui, PointerEvents::None)
}
