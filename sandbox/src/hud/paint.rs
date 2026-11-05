//! Small vector vocabulary shared by the concept HUD panels and symbols.
use calyx_engine::ui::{CornerRadius, PaintCommand, UiColor, UiPoint, UiRect, UiSize};

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
            scale: ((viewport.max.x - viewport.min.x) / 1672.0)
                .min((viewport.max.y - viewport.min.y) / 941.0)
                .max(0.2),
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
        self.bevel(x - 2.0, y - 2.0, w + 4.0, h + 4.0, cut + 1.0, BLACK);
        self.bevel(x, y, w, h, cut, STEEL);
        self.bevel(
            x + 2.0,
            y + 2.0,
            w - 4.0,
            h - 4.0,
            cut - 1.0,
            UiColor::rgba(37, 49, 61, 255),
        );
        self.bevel(
            x + 5.0,
            y + 5.0,
            w - 10.0,
            h - 10.0,
            (cut - 3.0).max(1.0),
            BLACK,
        );
        self.bevel(
            x + 6.0,
            y + 6.0,
            w - 12.0,
            h - 12.0,
            (cut - 4.0).max(1.0),
            PANEL,
        );
        self.line(
            (x + cut, y + 3.0),
            (x + w - cut, y + 3.0),
            UiColor::rgba(136, 151, 164, 100),
            1.0,
        );
    }
    pub fn text(&mut self, x: f32, y: f32, text: impl Into<String>, size: f32, color: UiColor) {
        let text = text.into();
        let width = text_width(&text, size) + 8.0;
        self.commands.push(PaintCommand::Text {
            rect: self.rect(x, y, width, size * 1.35),
            text,
            color,
            font_size: size * self.scale,
        });
    }
    pub fn centered(&mut self, x: f32, y: f32, text: &str, size: f32, color: UiColor) {
        self.text(x - text_width(text, size) * 0.5, y, text, size, color);
    }
    pub fn bold_centered(&mut self, x: f32, y: f32, text: &str, size: f32, color: UiColor) {
        self.centered(x, y, text, size, color);
        self.centered(x + 0.65, y, text, size, color);
    }
    pub fn bold_text(&mut self, x: f32, y: f32, text: &str, size: f32, color: UiColor) {
        self.text(x, y, text, size, color);
        self.text(x + 0.65, y, text, size, color);
    }
    pub fn circle(&mut self, x: f32, y: f32, r: f32, color: UiColor) {
        self.fill(x - r, y - r, 2.0 * r, 2.0 * r, color, r);
    }
    pub fn ring(&mut self, x: f32, y: f32, r: f32, color: UiColor, width: f32) {
        for i in 0..96 {
            let a = i as f32 * std::f32::consts::TAU / 96.0;
            let b = (i + 1) as f32 * std::f32::consts::TAU / 96.0;
            self.line(
                (x + r * a.cos(), y + r * a.sin()),
                (x + r * b.cos(), y + r * b.sin()),
                color,
                width,
            );
        }
    }
    pub fn tank(&mut self, x: f32, y: f32, s: f32, color: UiColor) {
        self.bevel(x, y + 5.0 * s, 28.0 * s, 12.0 * s, 3.0 * s, color);
        self.bevel(x + 6.0 * s, y, 14.0 * s, 10.0 * s, 3.0 * s, color);
        self.fill(x + 18.0 * s, y + 3.0 * s, 16.0 * s, 3.0 * s, color, 0.0);
        self.line(
            (x + 3.0 * s, y + 14.0 * s),
            (x + 24.0 * s, y + 14.0 * s),
            PANEL,
            1.5 * s,
        );
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
        self.polygon(
            &[
                (x, y + 7.0 * s),
                (x + 7.0 * s, y),
                (x + 17.0 * s, y + 8.0 * s),
                (x + 12.0 * s, y + 18.0 * s),
            ],
            color,
        );
        self.polygon(
            &[
                (x + 12.0 * s, y + 8.0 * s),
                (x + 22.0 * s, y),
                (x + 31.0 * s, y + 7.0 * s),
                (x + 18.0 * s, y + 20.0 * s),
            ],
            color,
        );
        for i in 0..3 {
            let d = i as f32 * 4.0 * s;
            self.line(
                (x + 10.0 * s + d, y + 12.0 * s),
                (x + 14.0 * s + d, y + 16.0 * s),
                PANEL,
                1.5 * s,
            );
        }
    }
}
pub(super) fn text_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| {
            if c.is_whitespace() {
                0.28
            } else if "ilI1.,:!".contains(c) {
                0.30
            } else if "WM@".contains(c) {
                0.85
            } else if !c.is_ascii() {
                1.0
            } else if c.is_uppercase() {
                0.64
            } else {
                0.53
            }
        })
        .sum::<f32>()
        * size
}
pub(super) fn alpha(color: UiColor, opacity: f32) -> UiColor {
    UiColor::rgba(
        color.r,
        color.g,
        color.b,
        (color.a as f32 * opacity.clamp(0.0, 1.0)) as u8,
    )
}
pub(super) fn clipped_name(name: &str, width: f32, size: f32) -> String {
    if text_width(name, size) <= width {
        return name.to_string();
    }
    let mut result = String::new();
    for character in name.chars() {
        let candidate = format!("{result}{character}\u{2026}");
        if text_width(&candidate, size) > width {
            break;
        }
        result.push(character);
    }
    result.push('\u{2026}');
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_and_unicode_names_fit_their_columns() {
        for name in [
            "WWWWWWWWWWWWWWWWWWWWWW",
            "LONG_UPPERCASE_TANK_NAME",
            "\u{5766}\u{514b}\u{5766}\u{514b}\u{5766}\u{514b}\u{5766}\u{514b}",
        ] {
            let label = clipped_name(name, 180.0, 23.0);
            assert!(text_width(&label, 23.0) <= 180.0);
            assert!(label.ends_with('\u{2026}'));
        }
    }
}
