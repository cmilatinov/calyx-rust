use super::Panel;
use crate::{icons, EditorAppState};
use calyx_engine::math::fit_aspect;
use calyx_engine::scene::SceneHud;
use calyx_engine::ui::{UiInput, UiPoint};
use egui::emath::GuiRounding;
use egui::{Color32, Key, Modifiers, PointerButton, Rect, Response, Sense, TextureId, Ui};
use re_ui::Icon;
use std::any::Any;

#[derive(Default)]
pub struct PanelGame {
    pub is_cursor_grabbed: bool,
    hud: SceneHud,
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
            let res = self.viewport(ui, state);
            self.viewport_input(ui, state, res);
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
                .selected_text(aspect_label(app_state.game_aspect))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut app_state.game_aspect, None, "None");
                    ui.selectable_value(&mut app_state.game_aspect, Some((4, 3)), "4:3");
                    ui.selectable_value(&mut app_state.game_aspect, Some((16, 9)), "16:9");
                    ui.selectable_value(&mut app_state.game_aspect, Some((21, 9)), "21:9");
                });
        });
        ui.add_space(-ui.style().spacing.item_spacing.y + padding);
    }

    fn viewport(&mut self, ui: &mut Ui, app_state: &mut EditorAppState) -> Response {
        let texture_id = app_state
            .game_renderer
            .scene_texture_handle()
            .map(|handle| handle.id());
        let response = game_image(ui, texture_id, app_state.game_aspect);
        // The texture was rendered from the scene before this frame's
        // update, so the HUD reads the same state and occupies the image.
        self.hud.paint(
            app_state.game.scenes.simulation_scene(),
            ui.painter(),
            response.rect,
            hud_input(ui),
        );
        response
    }

    fn viewport_input(&mut self, ui: &mut Ui, app_state: &mut EditorAppState, res: Response) {
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
        app_state.game_size = res.rect.size();
        app_state.game_response = Some(res);
    }
}

fn aspect_label(aspect: Option<(u32, u32)>) -> String {
    match aspect {
        Some((width, height)) => {
            let divisor = num::integer::gcd(width, height).max(1);
            match (width / divisor, height / divisor) {
                (7, 3) => "21:9".into(),
                (width, height) => format!("{width}:{height}"),
            }
        }
        None => "None".into(),
    }
}

/// The image, HUD and gameplay input share this rect, including when an
/// aspect ratio leaves bars around it. A justified `Image` widget's response
/// covers the surrounding cell instead of just the image it paints.
fn game_image(ui: &mut Ui, texture: Option<TextureId>, aspect: Option<(u32, u32)>) -> Response {
    let (id, available) = ui.allocate_space(ui.available_size());
    let size = match aspect {
        Some((width, height)) => fit_aspect(width as f32 / height as f32, available.size()).into(),
        None => available.size(),
    };
    let rect =
        Rect::from_center_size(available.center(), size).round_to_pixels(ui.pixels_per_point());
    if let Some(texture) = texture {
        ui.painter().image(
            texture,
            rect,
            Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    ui.interact(rect, id, Sense::click_and_drag())
}

fn hud_input(ui: &Ui) -> UiInput {
    ui.input(|input| UiInput {
        pointer_position: input
            .pointer
            .latest_pos()
            .map(|position| UiPoint::new(position.x, position.y)),
        pointer_down: input.pointer.primary_down(),
        delta_time: input.stable_dt,
    })
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
mod tests;
