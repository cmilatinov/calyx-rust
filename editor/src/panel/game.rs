use super::Panel;
use crate::{icons, EditorAppState};
use egui::{
    load::SizedTexture, Image, ImageSource, Key, Modifiers, PointerButton, Response, Sense, Ui,
    Vec2,
};
use engine::math::fit_aspect;
use re_ui::Icon;
use std::any::Any;

#[derive(Default, Debug)]
pub struct PanelGame {
    pub is_cursor_grabbed: bool,
}

impl Panel for PanelGame {
    fn name() -> &'static str {
        "Game"
    }

    fn icon(&self) -> Option<&'static Icon> {
        Some(&icons::GAMEPAD)
    }

    fn ui(&mut self, ui: &mut Ui, state: &mut EditorAppState) {
        egui::Frame {
            fill: ui.style().visuals.panel_fill,
            ..Default::default()
        }
        .show(ui, |ui| {
            self.action_bar(ui, state);
            let (size, res) = self.viewport(ui, state);
            self.viewport_input(ui, state, size, res);
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
                let Some(texture_id) = app_state
                    .game_renderer
                    .scene_texture_handle()
                    .map(|handle| handle.id())
                else {
                    return ui.response();
                };
                ui.add(
                    Image::new(ImageSource::Texture(SizedTexture {
                        id: texture_id,
                        size,
                    }))
                    .sense(Sense::click_and_drag()),
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
        let primary_pressed =
            ui.input(|input| input.pointer.button_pressed(PointerButton::Primary));
        let primary_pressed_on_viewport = res.hovered() && primary_pressed;
        let escape_pressed = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
        self.is_cursor_grabbed = cursor_grabbed_after_input(
            self.is_cursor_grabbed,
            primary_pressed_on_viewport,
            primary_pressed,
            escape_pressed,
        );
        if primary_pressed_on_viewport {
            res.request_focus();
        }
        if self.is_cursor_grabbed && !res.has_focus() {
            res.request_focus();
        }
        let screen_rect = ui.ctx().screen_rect();
        app_state.game_size = (size.x / screen_rect.width(), size.y / screen_rect.height());
        app_state.game_response = Some(res);
    }
}

fn cursor_grabbed_after_input(
    was_grabbed: bool,
    primary_pressed_on_viewport: bool,
    primary_pressed: bool,
    escape_pressed: bool,
) -> bool {
    if primary_pressed_on_viewport {
        return true;
    }
    if primary_pressed || escape_pressed {
        return false;
    }
    was_grabbed
}

#[cfg(test)]
mod tests {
    use super::cursor_grabbed_after_input;

    #[test]
    fn clicking_the_game_viewport_grabs_input() {
        assert!(cursor_grabbed_after_input(false, true, true, false));
    }

    #[test]
    fn clicking_outside_the_game_viewport_releases_input() {
        assert!(!cursor_grabbed_after_input(true, false, true, false));
    }

    #[test]
    fn escape_releases_game_viewport_input() {
        assert!(!cursor_grabbed_after_input(true, false, false, true));
    }
}
