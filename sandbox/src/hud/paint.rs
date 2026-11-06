//! Small vector vocabulary shared by the concept HUD panels and symbols.
use calyx_engine::ui::{
    CornerRadius, PaintCommand, TextAnchor, TextLabel, TextOutline, UiColor, UiFont, UiPoint,
    UiRect, UiSize,
};

pub(super) const PANEL: UiColor = UiColor::rgba(18, 25, 33, 245);
pub(super) const STEEL: UiColor = UiColor::rgba(89, 103, 117, 255);
pub(super) const LIGHT: UiColor = UiColor::rgba(235, 242, 247, 255);
pub(super) const MUTED: UiColor = UiColor::rgba(143, 157, 170, 255);
pub(super) const CYAN: UiColor = UiColor::rgba(0, 184, 238, 255);
pub(super) const RED: UiColor = UiColor::rgba(236, 55, 62, 255);
pub(super) const BLACK: UiColor = UiColor::rgba(6, 11, 17, 255);

pub(super) struct Painter {
    pub commands: Vec<PaintCommand>,
    pub scale: f32,
    pub origin: UiPoint,
}

impl Painter {
    pub fn new(viewport: UiRect) -> Self {
        Self {
            commands: vec![PaintCommand::PushClip(viewport)],
            scale: 1.0,
            origin: viewport.min,
        }
    }

    pub fn point(&self, x: f32, y: f32) -> UiPoint {
        UiPoint::new(
            self.origin.x + x * self.scale,
            self.origin.y + y * self.scale,
        )
    }

    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> UiRect {
        UiRect::from_min_size(
            self.point(x, y),
            UiSize::new(w * self.scale, h * self.scale),
        )
    }

    pub fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, color: UiColor, radius: f32) {
        self.commands.push(PaintCommand::FillRect {
            rect: self.rect(x, y, w, h),
            color,
            radius: CornerRadius::all(radius * self.scale),
        });
    }

    pub fn polygon(&mut self, points: &[(f32, f32)], color: UiColor) {
        self.commands.push(PaintCommand::FillPolygon {
            points: points.iter().map(|&(x, y)| self.point(x, y)).collect(),
            color,
        });
    }

    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), color: UiColor, width: f32) {
        self.commands.push(PaintCommand::Line {
            start: self.point(a.0, a.1),
            end: self.point(b.0, b.1),
            color,
            width: width * self.scale,
        });
    }

    pub fn bevel(&mut self, x: f32, y: f32, w: f32, h: f32, cut: f32, color: UiColor) {
        self.polygon(
            &[
                (x + cut, y),
                (x + w - cut, y),
                (x + w, y + cut),
                (x + w, y + h - cut),
                (x + w - cut, y + h),
                (x + cut, y + h),
                (x, y + h - cut),
                (x, y + cut),
            ],
            color,
        );
    }

    pub fn panel(&mut self, x: f32, y: f32, w: f32, h: f32, cut: f32) {
        self.bevel(x, y, w, h, cut, STEEL);
        self.bevel(
            x + 3.0,
            y + 3.0,
            w - 6.0,
            h - 6.0,
            (cut - 2.0).max(1.0),
            PANEL,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn label(
        &mut self,
        x: f32,
        y: f32,
        text: impl Into<String>,
        size: f32,
        color: UiColor,
        anchor: TextAnchor,
        width: Option<f32>,
        italic: bool,
        outline: Option<f32>,
    ) {
        self.commands.push(PaintCommand::Label(TextLabel {
            position: self.point(x, y),
            text: text.into(),
            font: if italic {
                UiFont::BoldItalic
            } else {
                UiFont::Bold
            },
            font_size: size * self.scale,
            scale: 1.0,
            color,
            anchor,
            max_width: width.map(|w| w * self.scale),
            outline: outline.map(|width| TextOutline {
                width: width * self.scale,
                color: BLACK.with_alpha(color.a as f32 / 255.0),
            }),
        }));
    }

    pub fn middle(
        &mut self,
        x: f32,
        y: f32,
        text: impl Into<String>,
        size: f32,
        color: UiColor,
        width: Option<f32>,
    ) {
        self.label(
            x,
            y,
            text,
            size,
            color,
            TextAnchor::Center,
            width,
            false,
            None,
        );
    }

    pub fn gradient(&mut self, points: &[(f32, f32)], colors: &[UiColor], indices: &[u32]) {
        self.commands.push(PaintCommand::ColorMesh {
            points: points.iter().map(|&(x, y)| self.point(x, y)).collect(),
            colors: colors.to_vec(),
            indices: indices.to_vec(),
        });
    }

    pub fn title_plate(&mut self, x: f32, y: f32, w: f32, h: f32) {
        for (inset, color) in [(0.0, STEEL), (3.0, PANEL)] {
            let x = x + inset;
            let y = y + inset;
            let w = w - inset * 2.0;
            let h = h - inset * 2.0;
            self.polygon(
                &[
                    (x + h * 0.4, y),
                    (x + w - h * 0.4, y),
                    (x + w, y + h * 0.5),
                    (x + w - h * 0.4, y + h),
                    (x + h * 0.4, y + h),
                    (x, y + h * 0.5),
                ],
                color,
            );
        }
    }

    pub fn plate(&mut self, x: f32, y: f32, w: f32, h: f32) {
        for (inset, color) in [(0.0, STEEL), (3.0, PANEL)] {
            let x = x + inset;
            let y = y + inset;
            let w = w - 2.0 * inset;
            let h = h - 2.0 * inset;
            self.polygon(
                &[
                    (x + w * 0.07, y),
                    (x + w * 0.93, y),
                    (x + w, y + h * 0.20),
                    (x + w * 0.88, y + h),
                    (x + w * 0.12, y + h),
                    (x, y + h * 0.20),
                ],
                color,
            );
        }
    }

    pub fn circle(&mut self, x: f32, y: f32, r: f32, color: UiColor) {
        self.fill(x - r, y - r, 2.0 * r, 2.0 * r, color, r);
    }

    pub fn tank(&mut self, x: f32, y: f32, s: f32, color: UiColor) {
        // Broad tread/body silhouette, stepped turret, roof hatch, long cannon.
        self.bevel(x, y + 11.0 * s, 42.0 * s, 13.0 * s, 5.0 * s, color);
        self.polygon(
            &[
                (x + 5.0 * s, y + 12.0 * s),
                (x + 12.0 * s, y + 4.0 * s),
                (x + 29.0 * s, y + 4.0 * s),
                (x + 33.0 * s, y + 12.0 * s),
            ],
            color,
        );
        self.fill(x + 16.0 * s, y, 13.0 * s, 3.0 * s, color, 0.6 * s);
        self.fill(x + 28.0 * s, y + 7.0 * s, 22.0 * s, 4.0 * s, color, 0.8 * s);
        self.fill(x + 47.0 * s, y + 5.0 * s, 4.0 * s, 7.0 * s, color, s);
    }

    pub fn skull(&mut self, x: f32, y: f32, s: f32, color: UiColor) {
        self.bevel(x, y, 23.0 * s, 19.0 * s, 6.0 * s, color);
        self.fill(x + 5.0 * s, y + 15.0 * s, 13.0 * s, 10.0 * s, color, 1.0);
        self.fill(x + 4.0 * s, y + 7.0 * s, 5.0 * s, 5.0 * s, PANEL, 1.0);
        self.fill(x + 14.0 * s, y + 7.0 * s, 5.0 * s, 5.0 * s, PANEL, 1.0);
        for dx in [9.0, 14.0] {
            self.line(
                (x + dx * s, y + 20.0 * s),
                (x + dx * s, y + 25.0 * s),
                PANEL,
                s,
            );
        }
    }

    pub fn assist(&mut self, x: f32, y: f32, s: f32, color: UiColor) {
        let draw = |p: &mut Self, points: &[(f32, f32)], c| {
            p.polygon(
                &points
                    .iter()
                    .map(|&(a, b)| (x + a * s, y + b * s))
                    .collect::<Vec<_>>(),
                c,
            )
        };
        // Two cuffs and palms meeting around a shared thumb, with four fingers.
        draw(
            self,
            &[(0.0, 7.0), (7.0, 0.0), (13.0, 6.0), (6.0, 15.0)],
            color,
        );
        draw(
            self,
            &[(33.0, 7.0), (26.0, 0.0), (20.0, 6.0), (27.0, 15.0)],
            color,
        );
        draw(
            self,
            &[
                (9.0, 7.0),
                (15.0, 4.0),
                (23.0, 8.0),
                (26.0, 15.0),
                (18.0, 23.0),
                (7.0, 14.0),
            ],
            color,
        );
        self.line(
            (x + 15.0 * s, y + 5.0 * s),
            (x + 12.0 * s, y + 11.0 * s),
            PANEL,
            2.0 * s,
        );
        self.line(
            (x + 12.0 * s, y + 11.0 * s),
            (x + 18.0 * s, y + 13.0 * s),
            PANEL,
            2.0 * s,
        );
        for i in 0..4 {
            let d = i as f32 * 3.2;
            self.line(
                (x + (10.0 + d) * s, y + (14.0 + d * 0.5) * s),
                (x + (13.0 + d) * s, y + (17.0 + d * 0.5) * s),
                PANEL,
                1.4 * s,
            );
        }
    }
}

pub(super) fn alpha(color: UiColor, opacity: f32) -> UiColor {
    color.with_alpha(opacity)
}
