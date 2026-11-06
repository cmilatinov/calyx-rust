//! Real font layout and coverage-mask outlines for the egui adapter.
use std::collections::VecDeque;
use std::sync::Arc;

use super::to_egui_color;
use crate::ui::{TextAnchor, TextLabel, UiColor, UiFont};

const BOLD: &str = "calyx-hud-bold";
const ITALIC: &str = "calyx-hud-bold-italic";

pub(super) fn install_fonts(ctx: &egui::Context) {
    for (name, bytes) in [
        (BOLD, include_bytes!("../fonts/Barlow-Bold.ttf").as_slice()),
        (
            ITALIC,
            include_bytes!("../fonts/Barlow-BoldItalic.ttf").as_slice(),
        ),
    ] {
        ctx.add_font(egui::epaint::text::FontInsert::new(
            name,
            egui::FontData::from_static(bytes),
            vec![egui::epaint::text::InsertFontFamily {
                family: egui::FontFamily::Name(name.into()),
                priority: egui::epaint::text::FontPriority::Highest,
            }],
        ));
    }
}

fn layout(painter: &egui::Painter, label: &TextLabel) -> Arc<egui::Galley> {
    let family = egui::FontFamily::Name(
        match label.font {
            UiFont::Bold => BOLD,
            UiFont::BoldItalic => ITALIC,
        }
        .into(),
    );
    // add_font becomes active on the next pass. The first frame still uses real
    // backend metrics; subsequent frames use the requested bundled face.
    let family = painter.ctx().fonts(|fonts| {
        if fonts.families().contains(&family) {
            family
        } else {
            egui::FontFamily::Proportional
        }
    });
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.text.clone(),
        egui::FontId::new(label.font_size, family),
        to_egui_color(label.color),
    );
    if let Some(width) = label.max_width {
        job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    }
    painter.ctx().fonts(|fonts| fonts.layout_job(job))
}

fn anchor_position(label: &TextLabel, size: egui::Vec2) -> egui::Pos2 {
    let (x, y) = match label.anchor {
        TextAnchor::TopLeft => (0.0, 0.0),
        TextAnchor::TopCenter => (0.5, 0.0),
        TextAnchor::Center => (0.5, 0.5),
        TextAnchor::CenterLeft => (0.0, 0.5),
        TextAnchor::CenterRight => (1.0, 0.5),
    };
    egui::pos2(label.position.x - x * size.x, label.position.y - y * size.y)
}

#[derive(Clone, Default)]
struct OutlineCache(VecDeque<(String, egui::TextureHandle, egui::Vec2, egui::Vec2)>);

pub(super) fn paint_label(painter: &egui::Painter, label: &TextLabel) {
    if !label.font_size.is_finite()
        || label.font_size <= 0.0
        || label.text.is_empty()
        || label
            .max_width
            .is_some_and(|width| width <= 0.0 || !width.is_finite())
    {
        return;
    }
    if !label.scale.is_finite() || label.scale <= 0.0 {
        return;
    }
    let mut normalized = label.clone();
    let opacity = if let Some(outline) = label.outline {
        label.color.a.max(outline.color.a)
    } else {
        label.color.a
    };
    if opacity == 0 {
        return;
    }
    if let Some(outline) = &mut normalized.outline {
        normalized.color.a = (u16::from(label.color.a) * 255 / u16::from(opacity)) as u8;
        outline.color.a = (u16::from(outline.color.a) * 255 / u16::from(opacity)) as u8;
    } else {
        normalized.font_size *= label.scale;
    }
    let galley = layout(painter, &normalized);
    let visual_scale = if normalized.outline.is_some() {
        label.scale
    } else {
        1.0
    };
    let position = anchor_position(label, galley.size() * visual_scale);
    let Some(outline) = normalized
        .outline
        .filter(|o| o.width.is_finite() && o.width > 0.0)
    else {
        painter.galley(position, galley, to_egui_color(label.color));
        return;
    };
    if !galley.mesh_bounds.is_finite() || !galley.mesh_bounds.is_positive() {
        return;
    }
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let key = format!(
        "{:?}/{:?}/{}/{:?}/{:?}/{ppp}",
        normalized.font, normalized.color, normalized.font_size, outline, galley.job
    );
    let cache_id = egui::Id::new("calyx-ui-outline-textures");
    let cached = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<OutlineCache>(cache_id)
            .0
            .iter()
            .find(|(k, ..)| k == &key)
            .cloned()
    });
    let (_, texture, offset, size) = cached.unwrap_or_else(|| {
        let atlas = ctx.fonts(|fonts| fonts.image());
        let (image, offset) = outlined_image(
            &galley,
            &atlas,
            ppp,
            outline.width,
            normalized.color,
            outline.color,
        );
        let size = egui::vec2(image.size[0] as f32 / ppp, image.size[1] as f32 / ppp);
        let texture = ctx.load_texture("outlined HUD label", image, egui::TextureOptions::LINEAR);
        let entry = (key, texture, offset, size);
        ctx.data_mut(|data| {
            let cache = data.get_temp_mut_or_default::<OutlineCache>(cache_id);
            if cache.0.len() >= 128 {
                cache.0.pop_front();
            }
            cache.0.push_back(entry.clone());
        });
        entry
    });
    painter.image(
        texture.id(),
        egui::Rect::from_min_size(position + offset * visual_scale, size * visual_scale),
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::from_white_alpha(opacity),
    );
}

/// Rasterize the *laid-out atlas glyphs*, then dilate their shared coverage mask.
/// Each outlined string becomes one image; no repeated text draws or guessed metrics.
fn outlined_image(
    galley: &egui::Galley,
    atlas: &egui::FontImage,
    ppp: f32,
    width: f32,
    fill: UiColor,
    edge: UiColor,
) -> (egui::ColorImage, egui::Vec2) {
    let radius = (width * ppp).ceil().clamp(1.0, 16.0) as i32;
    let padding = radius + 2;
    let bounds = galley.mesh_bounds;
    let left = (bounds.min.x * ppp).floor() as i32 - padding;
    let top = (bounds.min.y * ppp).floor() as i32 - padding;
    let w = ((bounds.max.x * ppp).ceil() as i32 - left + padding).max(1) as usize;
    let h = ((bounds.max.y * ppp).ceil() as i32 - top + padding).max(1) as usize;
    let mut coverage = vec![0.0f32; w * h];
    for row in &galley.rows {
        for glyph in &row.glyphs {
            let uv = glyph.uv_rect;
            let start = glyph.pos + uv.offset;
            let dx = (start.x * ppp).round() as i32 - left;
            let dy = (start.y * ppp).round() as i32 - top;
            for y in uv.min[1]..uv.max[1] {
                for x in uv.min[0]..uv.max[0] {
                    let px = dx + i32::from(x - uv.min[0]);
                    let py = dy + i32::from(y - uv.min[1]);
                    if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                        let value = atlas.pixels[usize::from(y) * atlas.size[0] + usize::from(x)];
                        let index = py as usize * w + px as usize;
                        coverage[index] = coverage[index].max(value);
                    }
                }
            }
        }
    }
    let mut dilated = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let value = coverage[y * w + x];
            if value <= 0.0 {
                continue;
            }
            for oy in -radius..=radius {
                for ox in -radius..=radius {
                    if ox * ox + oy * oy > radius * radius {
                        continue;
                    }
                    let px = x as i32 + ox;
                    let py = y as i32 + oy;
                    if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                        let index = py as usize * w + px as usize;
                        dilated[index] = dilated[index].max(value);
                    }
                }
            }
        }
    }
    let pixels = coverage
        .iter()
        .zip(dilated)
        .map(|(&inside, outside)| {
            let a = inside * fill.a as f32 / 255.0;
            let b = outside * edge.a as f32 / 255.0 * (1.0 - a);
            let alpha = a + b;
            if alpha <= 0.0 {
                return egui::Color32::TRANSPARENT;
            }
            let channel = |f: u8, e: u8| ((f as f32 * a + e as f32 * b) / alpha).round() as u8;
            egui::Color32::from_rgba_unmultiplied(
                channel(fill.r, edge.r),
                channel(fill.g, edge.g),
                channel(fill.b, edge.b),
                (alpha * 255.0).round() as u8,
            )
        })
        .collect();
    (
        egui::ColorImage {
            size: [w, h],
            pixels,
        },
        egui::vec2(left as f32 / ppp, top as f32 / ppp),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{TextOutline, UiPoint};
    #[test]
    fn real_faces_measure_elide_align_and_outline_without_duplicate_text_shapes() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let _ = ctx.run(Default::default(), |_| {});
        let output = ctx.run(Default::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let mut label = TextLabel {
                position: UiPoint::new(100.0, 70.0),
                text: "WWWWWWWWWWWW".into(),
                font: UiFont::Bold,
                font_size: 28.0,
                scale: 1.0,
                color: UiColor::WHITE,
                anchor: TextAnchor::Center,
                max_width: Some(90.0),
                outline: None,
            };
            let galley = layout(&painter, &label);
            assert!(galley.elided);
            assert!(galley.size().x <= 90.0);
            assert_eq!(galley.rows[0].glyphs.last().unwrap().chr, '…');
            assert_eq!(
                anchor_position(&label, galley.size()).x + galley.size().x / 2.0,
                100.0
            );
            label.text = "-142!".into();
            label.font = UiFont::BoldItalic;
            label.outline = Some(TextOutline {
                width: 3.0,
                color: UiColor::BLACK,
            });
            let g = layout(&painter, &label);
            let atlas = ctx.fonts(|f| f.image());
            let (image, _) = outlined_image(&g, &atlas, 1.0, 3.0, UiColor::WHITE, UiColor::BLACK);
            assert!(image.pixels.iter().any(|p| p.a() > 0 && p.r() == 0));
            assert!(image.pixels.iter().any(|p| p.r() > 200));
            paint_label(&painter, &label);
            label.color.a = 127;
            label.outline.as_mut().unwrap().color.a = 127;
            label.scale = 1.2;
            paint_label(&painter, &label);
            assert_eq!(
                ctx.data_mut(|data| data
                    .get_temp_mut_or_default::<OutlineCache>(egui::Id::new(
                        "calyx-ui-outline-textures"
                    ))
                    .0
                    .len()),
                1,
                "fade and pop reuse one glyph mask"
            );
            label.text = "   ".into();
            paint_label(&painter, &label);
            label.max_width = Some(0.0);
            paint_label(&painter, &label);
        });
        assert_eq!(
            output
                .shapes
                .iter()
                .filter(|s| matches!(s.shape, egui::Shape::Mesh(_)))
                .count(),
            2
        );
        assert!(!output
            .shapes
            .iter()
            .any(|s| matches!(s.shape, egui::Shape::Text(_))));
    }
}
