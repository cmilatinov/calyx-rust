use crate::EditorAppState;
use engine::egui::{Key, Modifiers, Response, Sense, Vec2, ViewportCommand};
use engine::{
    egui::{self, load::SizedTexture, Image, ImageSource, Ui},
    math::fit_aspect,
};
use std::any::Any;

use super::Panel;

#[derive(Default, Debug)]
pub struct PanelGame {
    pub is_cursor_grabbed: bool,
}

impl Panel for PanelGame {
    fn name() -> &'static str {
        "Game"
    }

    fn ui(&mut self, ui: &mut Ui) {
        egui::Frame {
            fill: ui.style().visuals.panel_fill,
            ..Default::default()
        }
        .show(ui, |ui| {
            let mut app_state = EditorAppState::get_mut();
            self.action_bar(ui, &mut app_state);
            let (size, res) = self.viewport(ui, &mut app_state);
            self.viewport_input(ui, &mut app_state, size, res);
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl PanelGame {
    fn action_bar(&self, ui: &mut Ui, app_state: &mut EditorAppState) {
        let padding = 5.0;
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
        ui.add_space(-ui.style().spacing.item_spacing.y + padding);
    }

    fn viewport(&mut self, ui: &mut Ui, app_state: &mut EditorAppState) -> (Vec2, Response) {
        let available = ui.available_size();
        let size = if let Some((n, d)) = app_state.game_aspect {
            fit_aspect(n as f32 / d as f32, available).into()
        } else {
            available
        };
        let res = ui.with_layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                ui.add(
                    Image::new(ImageSource::Texture(SizedTexture {
                        id: app_state
                            .game_renderer
                            .as_ref()
                            .unwrap()
                            .read()
                            .scene_texture_handle()
                            .id(),
                        size,
                    }))
                    .sense(Sense::click()),
                )
            },
        );
        (size, res.inner)
    }

    fn viewport_input(
        &mut self,
        ui: &mut Ui,
        app_state: &mut EditorAppState,
        size: Vec2,
        res: Response,
    ) {
        if res.clicked() {
            self.is_cursor_grabbed = true;
        } else if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.is_cursor_grabbed = false;
        }
        if self.is_cursor_grabbed {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::CursorVisible(false));
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::CursorPosition(res.rect.center()));
        } else {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::CursorVisible(true));
        }
        let screen_rect = ui.ctx().screen_rect();
        app_state.game_size = (size.x / screen_rect.width(), size.y / screen_rect.height());
        app_state.game_response = Some(res);
    }
}
