use egui::Ui;
use egui_gizmo::{GizmoMode, GizmoVisuals};

use engine::*;
use engine::egui::{Color32, Image, Key, Margin, Sense};
use engine::egui_dock::TabStyle;
use engine::glm::Mat4;
use engine::render::CameraLike;

use crate::EditorAppState;
use crate::panel::Panel;

#[derive(Default)]
pub struct PanelViewport;

const GIZMO_VISUALS: GizmoVisuals = GizmoVisuals {
    x_color: Color32::from_rgb(255, 0,  148),
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
            inner_margin: Margin {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 0.0,
            },
            ..*global_style
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
                    .clamp_range(30..=160)
            );
            ui.label("FOV");
            if degrees != radians.to_degrees() {
                *radians = degrees.to_radians();
            }
        });
    }

    fn viewport(&self, ui: &mut Ui, app_state: &mut EditorAppState) -> egui::Response {
        let res = ui.add(Image::new(
            app_state.scene_renderer.as_ref()
                .unwrap()
                .read()
                .unwrap().scene_texture_handle().id(),
            egui::Vec2::new(ui.available_width(), ui.available_height()),
        ).sense(Sense::drag()));
        app_state.camera.update(ui, &res);
        let screen_rect = ui.ctx().screen_rect();
        app_state.viewport_width = res.rect.width() / screen_rect.width();
        app_state.viewport_height = res.rect.height() / screen_rect.height();
        res
    }

    fn gizmo(&self, ui: &mut Ui, app_state: &mut EditorAppState, viewport: &egui::Rect) {
        if let Some(selection) = app_state.selection.clone() {
            if let Some(node_id) = selection.first_entity() {
                let transform = app_state.scene.get_world_transform(node_id);
                let gizmo = egui_gizmo::Gizmo::new("test")
                    .projection_matrix(app_state.camera.camera.projection)
                    .view_matrix(app_state.camera.transform.get_inverse_matrix())
                    .model_matrix(transform.matrix)
                    .mode(app_state.gizmo_mode)
                    .viewport(*viewport)
                    .visuals(GIZMO_VISUALS);
                if let Some(gizmo_response) = gizmo.interact(ui) {
                    let transform = gizmo_response.transform();
                    app_state.scene.set_world_transform(
                        node_id,
                        Mat4::from(transform.to_cols_array_2d())
                    );
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
}