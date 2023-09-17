use egui::Ui;
use egui_gizmo::GizmoMode;

use engine::*;
use engine::egui::{Image, Margin, Sense};
use engine::egui_dock::TabStyle;
use engine::glm::Mat4;
use engine::render::CameraLike;

use crate::EditorAppState;
use crate::panel::Panel;

#[derive(Default)]
pub struct PanelViewport;

impl Panel for PanelViewport {
    fn name() -> &'static str {
        "Viewport"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get_mut();
        self.action_bar(ui, &mut app_state);
        let res = self.viewport(ui, &mut app_state);
        self.gizmo(ui, &app_state, &res.rect);
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

    fn gizmo(&self, ui: &mut Ui, app_state: &EditorAppState, viewport: &egui::Rect) {
        // app_state.scene

        let gizmo = egui_gizmo::Gizmo::new("test")
            .projection_matrix(app_state.camera.camera.projection)
            .view_matrix(app_state.camera.transform.get_inverse_matrix())
            .model_matrix(Mat4::identity())
            .mode(app_state.gizmo_mode.unwrap_or(GizmoMode::Translate))
            .viewport(*viewport);
        if let Some(gizmo_response) = gizmo.interact(ui) {
            // gizmo_response.transform()
        }
    }
}