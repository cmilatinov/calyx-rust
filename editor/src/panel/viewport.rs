use crate::panel::Panel;
use crate::EditorAppState;
use egui::Ui;
use engine::egui::load::SizedTexture;
use engine::egui::{
    Align2, Color32, Image, ImageSource, Key, PointerButton, Pos2, Sense, TextStyle,
};
use engine::glm::{DMat4, Mat4};
use engine::input::{Input, InputState};
use engine::math::Transform;
use engine::mint::ColumnMatrix4;
use engine::nalgebra;
use engine::render::CameraLike;
use engine::scene::SceneManager;
use engine::*;
use re_ui::Icon;
use std::any::Any;
use transform_gizmo_egui::config::DEFAULT_SNAP_ANGLE;
use transform_gizmo_egui::mint::RowMatrix4;
use transform_gizmo_egui::GizmoExt;
use transform_gizmo_egui::{
    Gizmo, GizmoConfig, GizmoMode, GizmoOrientation, GizmoResult, GizmoVisuals,
};

pub struct PanelViewport {
    gizmo: Gizmo,
}

impl Default for PanelViewport {
    fn default() -> Self {
        Self {
            gizmo: Gizmo::new(GizmoConfig::default()),
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
        Some(&re_ui::icons::SPACE_VIEW_3D)
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get_mut();
        egui::Frame {
            fill: ui.style().visuals.panel_fill,
            ..Default::default()
        }
            .show(ui, |ui| {
                self.action_bar(ui, &mut app_state);
                let res = self.viewport(ui, &mut app_state);
                self.gizmo(ui, &mut app_state, &res.rect);
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
                    .suffix("°")
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
        let res = ui.add(
            Image::new(ImageSource::Texture(SizedTexture {
                id: app_state
                    .scene_renderer
                    .as_ref()
                    .unwrap()
                    .read()
                    .scene_texture_handle()
                    .id(),
                size: ui.available_size() - egui::Vec2::new(0.0, 1.0),
            }))
                .sense(Sense::drag()),
        );
        let state = InputState {
            is_active: res.dragged_by(PointerButton::Secondary),
            last_cursor_pos: None,
        };
        app_state
            .camera
            .update(&Input::from_ctx(ui.ctx(), Some(&res), state));
        let screen_rect = ui.ctx().screen_rect();
        app_state.viewport_size = (
            res.rect.width() / screen_rect.width(),
            res.rect.height() / screen_rect.height(),
        );
        res
    }

    fn gizmo(&mut self, ui: &mut Ui, app_state: &mut EditorAppState, viewport: &egui::Rect) {
        ui.set_clip_rect(*viewport);
        let snap = ui.input(|input| input.modifiers.ctrl);
        let snap_coarse = ui.input(|input| input.modifiers.shift);
        let snap_distance = if snap_coarse { 10.0 } else { 1.0 };
        let snap_angle = if snap_coarse {
            DEFAULT_SNAP_ANGLE
        } else {
            DEFAULT_SNAP_ANGLE / 2.0
        };

        if let Some(game_object) = app_state.selection.first_game_object().and_then(|id| {
            SceneManager::get()
                .simulation_scene()
                .get_game_object_by_uuid(id)
        }) {
            let view_matrix = RowMatrix4::from(<DMat4 as Into<ColumnMatrix4<f64>>>::into(
                nalgebra::convert::<Mat4, DMat4>(app_state.camera.transform.inverse_matrix),
            ));
            let projection_matrix = RowMatrix4::from(<DMat4 as Into<ColumnMatrix4<f64>>>::into(
                nalgebra::convert::<Mat4, DMat4>(app_state.camera.camera.projection),
            ));
            self.gizmo.update_config(GizmoConfig {
                view_matrix,
                projection_matrix,
                viewport: *viewport,
                modes: app_state.gizmo_modes,
                mode_override: None,
                orientation: GizmoOrientation::Global,
                pivot_point: Default::default(),
                snapping: snap,
                snap_angle,
                snap_distance,
                snap_scale: snap_distance,
                visuals: GIZMO_VISUALS,
                pixels_per_point: 0.0,
            });
            let transform = SceneManager::get()
                .simulation_scene()
                .get_world_transform(game_object);
            if let Some((result, transforms)) = self.gizmo.interact(ui, &[transform.into()]) {
                let res: Transform = transforms[0].into();
                SceneManager::get_mut()
                    .simulation_scene_mut()
                    .set_world_transform(game_object, res.matrix);
                self.gizmo_status(ui, &result);
            }
        }
        if ui.input(|input| input.key_pressed(Key::Q)) {
            app_state.gizmo_modes = GizmoMode::all_translate();
        }
        if ui.input(|input| input.key_pressed(Key::E)) {
            app_state.gizmo_modes = GizmoMode::all_rotate();
        }
        if ui.input(|input| input.key_pressed(Key::R)) {
            app_state.gizmo_modes = GizmoMode::all_scale();
        }
        if ui.input(|input| input.key_pressed(Key::Z)) {
            app_state.gizmo_orientation = if app_state.gizmo_orientation == GizmoOrientation::Global
            {
                GizmoOrientation::Local
            } else {
                GizmoOrientation::Global
            };
        }
    }

    fn gizmo_status(&self, ui: &Ui, response: &GizmoResult) {
        let text = match response {
            GizmoResult::Rotation { total, .. } => {
                format!("{:.1}°, {:.2} rad", total.to_degrees(), total)
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
}
