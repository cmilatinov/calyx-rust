use engine::{
    egui::{self, load::SizedTexture, Image, ImageSource, Margin, Ui},
    egui_dock::{TabBodyStyle, TabStyle},
    math::fit_aspect,
};

use crate::EditorAppState;

use super::Panel;

#[derive(Default)]
pub struct PanelGame;

impl Panel for PanelGame {
    fn name() -> &'static str {
        "Game"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get_mut();
        self.action_bar(ui, &mut app_state);
        self.viewport(ui, &mut app_state);
    }

    fn tab_style_override(&self, global_style: &TabStyle) -> Option<TabStyle> {
        Some(TabStyle {
            tab_body: TabBodyStyle {
                inner_margin: Margin {
                    left: 0.0,
                    right: 0.0,
                    top: 0.0,
                    bottom: 0.0,
                },
                ..global_style.tab_body
            },
            ..global_style.clone()
        })
    }
}

impl PanelGame {
    fn action_bar(&self, ui: &mut Ui, app_state: &mut EditorAppState) {
        let padding = 3.0;
        ui.add_space(padding);
        ui.horizontal(|ui| {
            ui.add_space(padding);
            egui::ComboBox::from_label("Aspect Ratio")
                .selected_text(match app_state.game_aspect {
                    Some(value) => match value {
                        (4, 3) => "4:3",
                        (16, 9) => "16:9",
                        (21, 9) => "21:9",
                        _ => "None",
                    },
                    None => "None",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut app_state.game_aspect, None, "None");
                    ui.selectable_value(&mut app_state.game_aspect, Some((4, 3)), "4:3");
                    ui.selectable_value(&mut app_state.game_aspect, Some((16, 9)), "16:9");
                    ui.selectable_value(&mut app_state.game_aspect, Some((21, 9)), "21:9");
                });
        });
    }

    fn viewport(&self, ui: &mut Ui, app_state: &mut EditorAppState) {
        let available = ui.available_size();
        let size = if let Some((n, d)) = app_state.game_aspect {
            fit_aspect(n as f32 / d as f32, available).into()
        } else {
            available
        };
        let res = ui.with_layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                ui.add(Image::new(ImageSource::Texture(SizedTexture {
                    id: app_state
                        .game_renderer
                        .as_ref()
                        .unwrap()
                        .read()
                        .scene_texture_handle()
                        .id(),
                    size,
                })))
            },
        );
        let screen_rect = ui.ctx().screen_rect();
        app_state.game_size = (size.x / screen_rect.width(), size.y / screen_rect.height());
        app_state.game_response = Some(res.inner);
    }
}
