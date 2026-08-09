use crate::panel::Panel;
use crate::selection::SelectionType;
use crate::{icons, EditorAppState, SceneEditSnapshot};
use egui::epaint::Vertex;
use egui::load::SizedTexture;
use egui::Ui;
use egui::{
    Align2, Color32, Image, ImageSource, Key, Mesh, Modifiers, PointerButton, Pos2, Response, Rgba,
    Sense, TextStyle,
};
use engine::input::{Input, InputState};
use engine::math::Transform;
use engine::render::CameraLike;
use mint::ColumnMatrix4;
use nalgebra_glm::{DMat4, Mat4};
use re_ui::Icon;
use std::any::Any;
use transform_gizmo_egui::config::DEFAULT_SNAP_ANGLE;
use transform_gizmo_egui::mint::RowMatrix4;
use transform_gizmo_egui::{
    Gizmo, GizmoConfig, GizmoInteraction, GizmoMode, GizmoOrientation, GizmoResult, GizmoVisuals,
};

pub struct PanelViewport {
    gizmo: Gizmo,
    transform_edit_before: Option<SceneEditSnapshot>,
    last_viewport_pass: Option<u64>,
}

impl Default for PanelViewport {
    fn default() -> Self {
        Self {
            gizmo: Gizmo::new(GizmoConfig::default()),
            transform_edit_before: None,
            last_viewport_pass: None,
        }
    }
}

const GIZMO_VISUALS: GizmoVisuals = GizmoVisuals {
    x_color: Color32::from_rgb(255, 0, 148),
    y_color: Color32::from_rgb(148, 255, 0),
    z_color: Color32::from_rgb(0, 148, 255),
    s_color: Color32::from_rgb(255, 255, 255),
    inactive_alpha: 0.4,
    highlight_alpha: 1.0,
    highlight_color: Some(Color32::from_rgb(255, 215, 0)),
    stroke_width: 3.5,
    gizmo_size: 75.0,
};

impl Panel for PanelViewport {
    fn name() -> &'static str {
        "Viewport"
    }

    fn icon(&self) -> Option<&'static Icon> {
        Some(&icons::VIEWPORT_3D)
    }

    fn ui(&mut self, ui: &mut Ui, state: &mut EditorAppState) {
        egui::Frame {
            fill: ui.style().visuals.panel_fill,
            ..Default::default()
        }
        .show(ui, |ui| {
            self.action_bar(ui, state);
            let res = self.viewport(ui, state);
            self.gizmo(ui, state, &res);
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl PanelViewport {
    fn action_bar(&self, ui: &mut Ui, app_state: &mut EditorAppState) {
        let padding = 5.0;
        ui.add_space(padding);
        ui.horizontal(|ui| {
            ui.add_space(padding);
            let radians = &mut app_state.camera.camera.fov_x;
            let mut degrees = radians.to_degrees();
            ui.add(
                egui::DragValue::new(&mut degrees)
                    .speed(1.0)
                    .suffix(" deg")
                    .range(30..=160),
            );
            ui.label("FOV");
            if degrees != radians.to_degrees() {
                *radians = degrees.to_radians();
            }
        });
        ui.add_space(-ui.style().spacing.item_spacing.y + padding);
    }

    fn viewport(&self, ui: &mut Ui, app_state: &mut EditorAppState) -> egui::Response {
        let Some(texture_id) = app_state
            .scene_renderer
            .scene_texture_handle()
            .map(|handle| handle.id())
        else {
            return ui.response();
        };
        let res = ui.add(
            Image::new(ImageSource::Texture(SizedTexture {
                id: texture_id,
                size: ui.available_size() - egui::Vec2::new(0.0, 1.0),
            }))
            .sense(Sense::click_and_drag()),
        );
        if ui.input(|input| input.pointer.any_pressed()) {
            if ui.rect_contains_pointer(res.rect) {
                res.request_focus();
            } else {
                res.surrender_focus();
            }
        }
        let state = InputState {
            is_active: res.dragged_by(PointerButton::Secondary),
            last_cursor_pos: None,
            ..Default::default()
        };
        let EditorAppState { camera, game, .. } = app_state;
        let input = Input::from_ctx(ui.ctx(), Some(&res), state);
        camera.update(game.resources.time(), &input);
        let screen_rect = ui.ctx().screen_rect();
        app_state.viewport_size = (
            res.rect.width() / screen_rect.width(),
            res.rect.height() / screen_rect.height(),
        );
        app_state.viewport_response = Some(res.clone());
        res
    }

    fn gizmo(&mut self, ui: &mut Ui, app_state: &mut EditorAppState, viewport_response: &Response) {
        let pass = ui.ctx().cumulative_pass_nr();
        let viewport_is_continuous =
            Self::viewport_pass_is_continuous(self.last_viewport_pass, pass);
        self.last_viewport_pass = Some(pass);
        ui.set_clip_rect(viewport_response.rect);
        let snap = ui.input(|input| input.modifiers.ctrl);
        let snap_coarse = ui.input(|input| input.modifiers.shift);
        let snap_distance = if snap_coarse { 10.0 } else { 1.0 };
        let snap_angle = if snap_coarse {
            DEFAULT_SNAP_ANGLE
        } else {
            DEFAULT_SNAP_ANGLE / 2.0
        };

        let mut gizmo_focused = false;
        let mut selected_game_object = false;
        let pointer_in_viewport = ui.rect_contains_pointer(viewport_response.rect);
        let hovered_game_object = self.hovered_game_object(ui, viewport_response, app_state);
        app_state.hovered_game_object = hovered_game_object;

        if let Some(game_object) = app_state
            .selection
            .first(SelectionType::GameObject)
            .and_then(|id| app_state.game.scenes.simulation_scene().find(id))
        {
            selected_game_object = true;
            let view_matrix = RowMatrix4::from(<DMat4 as Into<ColumnMatrix4<f64>>>::into(
                nalgebra::convert::<Mat4, DMat4>(app_state.camera.transform.inverse_matrix()),
            ));
            let projection_matrix = RowMatrix4::from(<DMat4 as Into<ColumnMatrix4<f64>>>::into(
                nalgebra::convert::<Mat4, DMat4>(app_state.camera.camera.projection),
            ));
            self.gizmo.update_config(GizmoConfig {
                view_matrix,
                projection_matrix,
                viewport: viewport_response.rect,
                modes: app_state.gizmo_modes,
                mode_override: None,
                orientation: app_state.gizmo_orientation,
                pivot_point: Default::default(),
                snapping: snap,
                snap_angle,
                snap_distance,
                snap_scale: snap_distance,
                visuals: GIZMO_VISUALS,
                pixels_per_point: ui.ctx().pixels_per_point(),
            });
            let transform = app_state
                .game
                .scenes
                .simulation_scene()
                .world_transform(game_object);
            if let Some((result, transforms)) = self.interact_gizmo(
                ui,
                viewport_response,
                pointer_in_viewport,
                &[transform.into()],
            ) {
                if self.transform_edit_before.is_none() {
                    self.transform_edit_before = app_state.scene_edit_snapshot();
                }
                let res: Transform = transforms[0].into();
                app_state
                    .game
                    .scenes
                    .simulation_scene_mut()
                    .set_world_transform(game_object, res.matrix());
                self.gizmo_status(ui, &result);
            }
            gizmo_focused = self.gizmo.is_focused();
        }

        if self.transform_edit_before.is_some()
            && !ui.input(|input| input.pointer.button_down(PointerButton::Primary))
        {
            let edit_before = self.transform_edit_before.take();
            if viewport_is_continuous && selected_game_object {
                app_state.commit_scene_edit("Transform game object", edit_before);
            }
        }

        if viewport_response.clicked_by(PointerButton::Primary) && !gizmo_focused {
            let clicked_game_object = self
                .viewport_pixel(ui, viewport_response, app_state)
                .and_then(|(pixel_x, pixel_y)| {
                    app_state.scene_renderer.pick_game_object(pixel_x, pixel_y)
                });
            if let Some(game_object_id) = clicked_game_object {
                app_state.selection =
                    crate::selection::Selection::from_id(SelectionType::GameObject, game_object_id);
            }
        }
        if !Self::transform_shortcuts_enabled(
            viewport_response.has_focus(),
            viewport_response.dragged_by(PointerButton::Secondary),
        ) {
            return;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Q)) {
            app_state.gizmo_modes = GizmoMode::all_translate();
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::E)) {
            app_state.gizmo_modes = GizmoMode::all_rotate();
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::R)) {
            app_state.gizmo_modes = GizmoMode::all_scale();
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Z)) {
            app_state.gizmo_orientation = if app_state.gizmo_orientation == GizmoOrientation::Global
            {
                GizmoOrientation::Local
            } else {
                GizmoOrientation::Global
            };
        }
    }

    fn hovered_game_object(
        &self,
        ui: &Ui,
        viewport_response: &Response,
        app_state: &mut EditorAppState,
    ) -> Option<uuid::Uuid> {
        let (pixel_x, pixel_y) = self.viewport_pixel(ui, viewport_response, app_state)?;
        app_state
            .scene_renderer
            .request_pick_game_object(pixel_x, pixel_y)
    }

    fn viewport_pixel(
        &self,
        ui: &Ui,
        viewport_response: &Response,
        app_state: &EditorAppState,
    ) -> Option<(u32, u32)> {
        if viewport_response.dragged_by(PointerButton::Secondary) {
            return None;
        }
        if !ui.rect_contains_pointer(viewport_response.rect) {
            return None;
        }
        let pointer_pos = ui.ctx().pointer_hover_pos()?;
        let (width, height) = app_state.scene_renderer.scene_texture_size();
        if width == 0
            || height == 0
            || viewport_response.rect.width() <= 0.0
            || viewport_response.rect.height() <= 0.0
        {
            return None;
        }

        let x = ((pointer_pos.x - viewport_response.rect.left()) / viewport_response.rect.width())
            .clamp(0.0, 0.999_999);
        let y = ((pointer_pos.y - viewport_response.rect.top()) / viewport_response.rect.height())
            .clamp(0.0, 0.999_999);
        let pixel_x = (x * width as f32).floor() as u32;
        let pixel_y = (y * height as f32).floor() as u32;
        Some((pixel_x, pixel_y))
    }

    fn interact_gizmo(
        &mut self,
        ui: &Ui,
        viewport_response: &Response,
        pointer_in_viewport: bool,
        transforms: &[transform_gizmo_egui::math::Transform],
    ) -> Option<(GizmoResult, Vec<transform_gizmo_egui::math::Transform>)> {
        let cursor_pos = ui.ctx().pointer_hover_pos().unwrap_or_default();
        let hovered = pointer_in_viewport
            && !ui.input(|input| input.pointer.button_down(PointerButton::Secondary));
        let gizmo_result = self.gizmo.update(
            GizmoInteraction {
                cursor_pos: (cursor_pos.x, cursor_pos.y),
                hovered,
                drag_started: hovered
                    && ui.input(|input| input.pointer.button_pressed(PointerButton::Primary)),
                dragging: hovered
                    && ui.input(|input| input.pointer.button_down(PointerButton::Primary)),
            },
            transforms,
        );

        let draw_data = self.gizmo.draw();
        let mesh = Mesh {
            indices: draw_data.indices,
            vertices: draw_data
                .vertices
                .into_iter()
                .zip(draw_data.colors)
                .map(|(pos, [r, g, b, a])| Vertex {
                    pos: pos.into(),
                    uv: Pos2::default(),
                    color: Rgba::from_rgba_premultiplied(r, g, b, a).into(),
                })
                .collect(),
            ..Default::default()
        };
        ui.painter_at(viewport_response.rect).add(mesh);

        gizmo_result
    }

    fn gizmo_status(&self, ui: &Ui, response: &GizmoResult) {
        let text = match response {
            GizmoResult::Rotation { total, .. } => {
                format!("{:.1} deg, {:.2} rad", total.to_degrees(), total)
            }
            GizmoResult::Translation { total, .. } => {
                format!("dX: {:.2}, dY: {:.2}, dZ: {:.2}", total.x, total.y, total.z)
            }
            GizmoResult::Scale { total } => {
                format!("dX: {:.2}, dY: {:.2}, dZ: {:.2}", total.x, total.y, total.z)
            }
            _ => String::from(""),
        };
        let rect = ui.clip_rect();
        ui.painter().text(
            Pos2::new(rect.left() + 5.0, rect.bottom()),
            Align2::LEFT_BOTTOM,
            text,
            ui.style()
                .text_styles
                .get(&TextStyle::Body)
                .unwrap()
                .clone(),
            Color32::WHITE,
        );
    }

    fn viewport_pass_is_continuous(last_pass: Option<u64>, pass: u64) -> bool {
        last_pass.is_none_or(|last_pass| last_pass.saturating_add(1) >= pass)
    }

    fn transform_shortcuts_enabled(viewport_has_focus: bool, camera_dragging: bool) -> bool {
        viewport_has_focus && !camera_dragging
    }
}

#[cfg(test)]
mod tests {
    use super::PanelViewport;

    #[test]
    fn viewport_pass_continuity_rejects_inactive_tab_gaps() {
        assert!(PanelViewport::viewport_pass_is_continuous(Some(8), 9));
        assert!(PanelViewport::viewport_pass_is_continuous(Some(8), 8));
        assert!(!PanelViewport::viewport_pass_is_continuous(Some(8), 10));
    }

    #[test]
    fn transform_shortcuts_require_viewport_focus() {
        assert!(PanelViewport::transform_shortcuts_enabled(true, false));
        assert!(!PanelViewport::transform_shortcuts_enabled(false, false));
        assert!(!PanelViewport::transform_shortcuts_enabled(true, true));
    }
}
