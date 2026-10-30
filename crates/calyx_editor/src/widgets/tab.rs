use egui::{
    Align2, Color32, CursorIcon, Galley, Id, InnerResponse, Pos2, Rect, Response, Sense, Stroke,
    StrokeKind, TextStyle, TextWrapMode, Ui, Vec2, WidgetText,
};
use egui_tiles::{Behavior, TabState, TileId, Tiles};
use re_ui::{DesignTokens, Icon};
use std::sync::Arc;

pub struct TabWidget {
    pub galley: Arc<Galley>,
    pub rect: Rect,
    pub galley_rect: Rect,
    pub close_rect: Option<Rect>,
    pub icon: Option<&'static Icon>,
    pub icon_size: Vec2,
    pub icon_rect: Rect,
    pub bg_color: Color32,
    pub text_color: Color32,
}

pub struct TabDesc {
    pub label: WidgetText,
    pub icon: Option<&'static Icon>,
    pub selected: bool,
    pub hovered: bool,
    pub closeable: bool,
}

impl TabWidget {
    pub fn new<'a, Pane>(
        tab_viewer: &mut impl Behavior<Pane>,
        ui: &'a mut Ui,
        tiles: &'a Tiles<Pane>,
        tile_id: TileId,
        tab_state: &TabState,
        tab_desc: TabDesc,
        gamma: f32,
    ) -> Self {
        // tab icon
        let icon_size;
        let icon_width_plus_padding;
        if tab_desc.icon.is_some() {
            icon_size = DesignTokens::small_icon_size();
            icon_width_plus_padding = icon_size.x + DesignTokens::text_to_icon_padding();
        } else {
            icon_size = Vec2::ZERO;
            icon_width_plus_padding = 0.0;
        }

        // tab title
        let text = tab_desc.label;

        let font_id = TextStyle::Button.resolve(ui.style());
        let galley = text.into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, font_id);

        // close icon
        let close_size;
        let close_width_plus_padding;
        if tab_desc.closeable {
            close_size = DesignTokens::small_icon_size();
            close_width_plus_padding = close_size.x + DesignTokens::text_to_icon_padding();
        } else {
            close_size = Vec2::ZERO;
            close_width_plus_padding = 0.0;
        }

        let x_margin = tab_viewer.tab_title_spacing(ui.visuals());
        let (_, rect) = ui.allocate_space(Vec2::new(
            galley.size().x + 2.0 * x_margin + icon_width_plus_padding + close_width_plus_padding,
            DesignTokens::title_bar_height(),
        ));
        let galley_rect = Rect::from_two_pos(
            rect.min + Vec2::new(icon_width_plus_padding, 0.0),
            rect.max - Vec2::new(close_width_plus_padding, 0.0),
        );
        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.left() + x_margin + icon_size.x / 2.0, rect.center().y),
            icon_size,
        );
        let close_rect = Rect::from_center_size(
            rect.max - Vec2::new(close_size.x, rect.height() / 2.0),
            close_size,
        );

        let bg_color = if tab_desc.selected {
            ui.visuals().selection.bg_fill
        } else if tab_desc.hovered {
            ui.visuals().widgets.hovered.bg_fill
        } else {
            re_ui::design_tokens().tab_bar_color
        };
        let bg_color = bg_color.gamma_multiply(gamma);
        let text_color = tab_viewer
            .tab_text_color(ui.visuals(), tiles, tile_id, tab_state)
            .gamma_multiply(gamma);

        Self {
            galley,
            rect,
            galley_rect,
            close_rect: if tab_desc.closeable {
                Some(close_rect)
            } else {
                None
            },
            icon: tab_desc.icon,
            icon_size,
            icon_rect,
            bg_color,
            text_color,
        }
    }

    pub fn show(
        mut self,
        ui: &mut Ui,
        id: Id,
        state: &TabState,
    ) -> InnerResponse<Option<Response>> {
        let response = ui
            .interact(self.rect, id, Sense::click_and_drag())
            .on_hover_cursor(CursorIcon::Grab);

        if response.hovered() {
            self.bg_color = ui.visuals().widgets.hovered.bg_fill;
        }

        if !ui.is_rect_visible(self.rect) || state.is_being_dragged {
            return InnerResponse {
                inner: None,
                response,
            };
        }

        let close_response = self.close_rect.map(|rect| {
            ui.interact(rect, id.with("close"), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand)
        });

        ui.painter().rect(
            self.rect,
            0.0,
            self.bg_color,
            Stroke::NONE,
            StrokeKind::Middle,
        );

        if let Some(icon) = self.icon {
            let icon_image = icon
                .as_image()
                .fit_to_exact_size(self.icon_size)
                .tint(self.text_color);
            icon_image.paint_at(ui, self.icon_rect);
        }

        if let Some(close_rect) = self.close_rect {
            if let Some(close_response) = &close_response {
                let color = if close_response.hovered() {
                    ui.visuals().widgets.hovered.text_color()
                } else {
                    ui.visuals().widgets.noninteractive.text_color()
                };
                let icon_image = re_ui::icons::CLOSE
                    .as_image()
                    .fit_to_exact_size(close_rect.size())
                    .tint(color);
                icon_image.paint_at(ui, close_rect);
            }
        }

        let label_color = self.text_color;
        ui.painter().galley(
            Align2::CENTER_CENTER
                .align_size_within_rect(self.galley.size(), self.galley_rect)
                .min,
            self.galley,
            label_color,
        );

        InnerResponse {
            inner: close_response,
            response,
        }
    }
}
