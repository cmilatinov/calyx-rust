use engine::egui;
use engine::egui::load::TexturePoll;
use engine::egui::{
    pos2, Color32, Image, Rect, Response, Rounding, Stroke, TextStyle, TextWrapMode, Ui, Vec2,
    Widget, WidgetText,
};

pub struct FileButton<'a> {
    pub image: Image<'a>,
    pub image_size: Vec2,
    pub image_spacing: f32,
    pub text: WidgetText,
    pub padding: Vec2,
    pub selected: bool,
}

impl Widget for FileButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self {
            image,
            image_size,
            image_spacing,
            text,
            padding,
            selected,
        } = self;

        let text = text.into_galley(
            ui,
            Some(TextWrapMode::Truncate),
            image_size.x,
            TextStyle::Button,
        );
        let text_size = text.size();
        let desired_size = image_size + 2.0 * padding + Vec2::new(0.0, text_size.y + image_spacing);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);
            let (frame_expansion, frame_rounding, frame_fill, frame_stroke) = if selected {
                let selection = ui.visuals().selection;
                (
                    Vec2::ZERO,
                    Rounding::ZERO,
                    selection.bg_fill,
                    selection.stroke,
                )
            } else if response.hovered() {
                let hovered = ui.visuals().widgets.hovered;
                (
                    Vec2::ZERO,
                    Rounding::ZERO,
                    hovered.bg_fill,
                    Stroke::default(),
                )
            } else {
                Default::default()
            };
            ui.painter().rect(
                rect.expand2(frame_expansion),
                frame_rounding,
                frame_fill,
                frame_stroke,
            );

            let mut image_rect = rect.shrink2(padding);
            image_rect.set_height(image_size.y);
            let tlr = image.load_for_size(ui.ctx(), image_size);
            if let Ok(TexturePoll::Ready { texture }) = tlr {
                ui.painter().image(
                    texture.id,
                    image_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }

            let mut text_rect = rect.shrink2(padding);
            text_rect.set_top(image_rect.bottom() + image_spacing);
            text_rect.set_bottom(rect.bottom());
            let mut text_pos = text_rect.center() - (text_size / 2.0);
            text_pos.y -= padding.y;
            ui.painter().galley(text_pos, text, visuals.text_color());
        }

        response
    }
}
