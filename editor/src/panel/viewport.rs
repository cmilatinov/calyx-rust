use egui::Ui;
use egui_gizmo::{GizmoMode, GizmoResult, GizmoVisuals, DEFAULT_SNAP_ANGLE};

use engine::egui::load::SizedTexture;
use engine::egui::{Align2, Color32, Image, ImageSource, Key, Margin, Pos2, Sense, TextStyle};
use engine::egui_dock::{TabBodyStyle, TabStyle};
use engine::glm::{Mat4, Vec3};
use engine::render::CameraLike;
use engine::*;
use engine::scene::SceneManager;

use crate::panel::Panel;
use crate::EditorAppState;

#[derive(Default)]
pub struct PanelViewport;

const GIZMO_VISUALS: GizmoVisuals = GizmoVisuals {
    x_color: Color32::from_rgb(255, 0, 148),
    y_color: Color32::from_rgb(148, 255, 0),
    z_color: Color32::from_rgb(0, 148, 255),
    s_color: Color32::from_rgb(255, 255, 255),
    inactive_alpha: 0.5,
    highlight_alpha: 1.0,
    highlight_color: Some(Color32::from_rgb(255, 215, 0)),
    stroke_width: 5.0,
    gizmo_size: 75.0,
};

impl Panel for PanelViewport {
    fn name() -> &'static str {
        "Viewport"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get_mut();
        self.action_bar(ui, &mut app_state);
        let res = self.viewport(ui, &mut app_state);
        self.gizmo(ui, &mut app_state, &res.rect);
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

impl PanelViewport {
    fn action_bar(&self, ui: &mut Ui, app_state: &mut EditorAppState) {
        let padding = 3.0;
        ui.add_space(padding);
        ui.horizontal(|ui| {
            ui.add_space(padding);
            let radians = &mut app_state.camera.camera.fov_x;
            let mut degrees = radians.to_degrees();
            ui.add(
                egui::DragValue::new(&mut degrees)
                    .speed(1.0)
                    .suffix("°")
                    .clamp_range(30..=160),
            );
            ui.label("FOV");
            if degrees != radians.to_degrees() {
                *radians = degrees.to_radians();
            }
        });
    }

    fn viewport(&self, ui: &mut Ui, app_state: &mut EditorAppState) -> egui::Response {
        let res = ui.add(
            Image::new(ImageSource::Texture(SizedTexture {
                id: app_state
                    .scene_renderer
                    .as_ref()
                    .unwrap()
                    .read()
                    .unwrap()
                    .scene_texture_handle()
                    .id(),
                size: egui::Vec2::new(ui.available_width(), ui.available_height()),
            }))
            .sense(Sense::drag()),
        );
        app_state.camera.update(ui, &res);
        let screen_rect = ui.ctx().screen_rect();
        app_state.viewport_width = res.rect.width() / screen_rect.width();
        app_state.viewport_height = res.rect.height() / screen_rect.height();
        res
    }

    fn gizmo(&self, ui: &mut Ui, app_state: &mut EditorAppState, viewport: &egui::Rect) {
        ui.set_clip_rect(*viewport);
        let snap = ui.input(|input| input.modifiers.ctrl);
        let snap_coarse = ui.input(|input| input.modifiers.shift);
        let snap_distance = if snap_coarse { 10.0 } else { 1.0 };
        let snap_angle = if snap_coarse {
            DEFAULT_SNAP_ANGLE
        } else {
            DEFAULT_SNAP_ANGLE / 2.0
        };

        if let Some(selection) = app_state.selection.clone() {
            if let Some(node_id) = selection.first_entity() {
                let transform = SceneManager::get().get_scene().get_world_transform(node_id);
                let gizmo = egui_gizmo::Gizmo::new("test")
                    .projection_matrix(app_state.camera.camera.projection.into())
                    .view_matrix(app_state.camera.transform.get_inverse_matrix().into())
                    .model_matrix(transform.matrix.into())
                    .mode(app_state.gizmo_mode)
                    .viewport(*viewport)
                    .visuals(GIZMO_VISUALS)
                    .snapping(snap)
                    .snap_distance(snap_distance)
                    .snap_angle(snap_angle)
                    .snap_scale(snap_distance);
                if let Some(gizmo_response) = gizmo.interact(ui) {
                    let transform = gizmo_response.transform();
                    SceneManager::get().get_scene()
                        .set_world_transform(node_id, Mat4::from(transform));
                    self.gizmo_status(ui, &gizmo_response);
                }
            }
        }
        if ui.input(|input| input.key_pressed(Key::Q)) {
            app_state.gizmo_mode = GizmoMode::Translate;
        }
        if ui.input(|input| input.key_pressed(Key::E)) {
            app_state.gizmo_mode = GizmoMode::Rotate;
        }
        if ui.input(|input| input.key_pressed(Key::R)) {
            app_state.gizmo_mode = GizmoMode::Scale;
        }
    }

    fn gizmo_status(&self, ui: &Ui, response: &GizmoResult) {
        let length = Vec3::from(response.value).magnitude();
        let text = match response.mode {
            GizmoMode::Rotate => format!("{:.1}°, {:.2} rad", length.to_degrees(), length),
            GizmoMode::Translate | GizmoMode::Scale => format!(
                "dX: {:.2}, dY: {:.2}, dZ: {:.2}",
                response.value[0], response.value[1], response.value[2]
            ),
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
