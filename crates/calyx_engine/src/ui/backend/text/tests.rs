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
                .get_temp_mut_or_default::<OutlineCache>(egui::Id::new("calyx-ui-outline-textures"))
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
