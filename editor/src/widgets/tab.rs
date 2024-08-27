use engine::egui::Vec2;
use engine::{egui, egui_tiles};
use re_ui::{DesignTokens, Icon};

pub struct TabWidget {
    pub galley: std::sync::Arc<egui::Galley>,
    pub rect: egui::Rect,
    pub galley_rect: egui::Rect,
    pub icon: Option<&'static Icon>,
    pub icon_size: egui::Vec2,
    pub icon_rect: egui::Rect,
    pub bg_color: egui::Color32,
    pub text_color: egui::Color32,
}

pub struct TabDesc {
    pub label: egui::WidgetText,
    pub icon: Option<&'static Icon>,
    pub selected: bool,
    pub hovered: bool,
}

impl TabWidget {
    pub fn new<'a, Pane>(
        tab_viewer: &mut impl egui_tiles::Behavior<Pane>,
        ui: &'a mut egui::Ui,
        tiles: &'a egui_tiles::Tiles<Pane>,
        tile_id: egui_tiles::TileId,
        tab_state: &egui_tiles::TabState,
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

        let font_id = egui::TextStyle::Button.resolve(ui.style());
        let galley = text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font_id);

        let x_margin = tab_viewer.tab_title_spacing(ui.visuals());
        let (_, rect) = ui.allocate_space(egui::vec2(
            galley.size().x + 2.0 * x_margin + icon_width_plus_padding,
            DesignTokens::title_bar_height(),
        ));
        let galley_rect = egui::Rect::from_two_pos(
            rect.min + egui::vec2(icon_width_plus_padding, 0.0),
            rect.max,
        );
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + x_margin + icon_size.x / 2.0, rect.center().y),
            icon_size,
        );

        let bg_color = if tab_desc.selected {
            ui.visuals().selection.bg_fill
        } else if tab_desc.hovered {
            ui.visuals().widgets.hovered.bg_fill
        } else {
            tab_viewer.tab_bar_color(ui.visuals())
        };
        let bg_color = bg_color.gamma_multiply(gamma);
        let text_color = tab_viewer
            .tab_text_color(ui.visuals(), tiles, tile_id, tab_state)
            .gamma_multiply(gamma);

        Self {
            galley,
            rect,
            galley_rect,
            icon: tab_desc.icon,
            icon_size,
            icon_rect,
            bg_color,
            text_color,
        }
    }

    pub fn paint(self, ui: &egui::Ui) {
        ui.painter()
            .rect(self.rect, 0.0, self.bg_color, egui::Stroke::NONE);

        if let Some(icon) = self.icon {
            let icon_image = icon
                .as_image()
                .fit_to_exact_size(self.icon_size)
                .tint(self.text_color);
            icon_image.paint_at(ui, self.icon_rect);
        }

        //TODO(ab): use design tokens
        let label_color = self.text_color;

        ui.painter().galley(
            egui::Align2::CENTER_CENTER
                .align_size_within_rect(self.galley.size(), self.galley_rect)
                .min,
            self.galley,
            label_color,
        );
    }
}
