use crate::panel::Panel;
use crate::scene_document::SceneDocument;
use crate::scene_edit::SceneEdit;
use crate::selection::SelectionType;
use crate::{icons, EditorAppState};
use egui::epaint::Vertex;
use egui::load::SizedTexture;
use egui::Ui;
use egui::{
    Align2, Color32, Image, ImageSource, Key, Mesh, Modifiers, PointerButton, Pos2, Response, Rgba,
    Sense, TextStyle,
};
use engine::context::GameContext;
use engine::input::Input;
use engine::math::Transform;
use engine::render::CameraLike;
use engine::scene::GameObject;
use mint::ColumnMatrix4;
use nalgebra_glm::{DMat4, Mat4};
use re_ui::Icon;
use std::any::Any;
use transform_gizmo_egui::config::DEFAULT_SNAP_ANGLE;
use transform_gizmo_egui::mint::RowMatrix4;
use transform_gizmo_egui::{
    Gizmo, GizmoConfig, GizmoInteraction, GizmoMode, GizmoOrientation, GizmoResult, GizmoVisuals,
};
use uuid::Uuid;

pub struct PanelViewport {
    gizmo: Gizmo,
    /// The gizmo drag in progress, from its first move to the pointer's
    /// release.
    transform_edit: Option<GizmoDrag>,
    last_viewport_pass: Option<u64>,
}

impl Default for PanelViewport {
    fn default() -> Self {
        Self {
            gizmo: Gizmo::new(GizmoConfig::default()),
            transform_edit: None,
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
        let is_active = res.dragged_by(PointerButton::Secondary);
        let EditorAppState { camera, game, .. } = app_state;
        let input = Input::from_ctx(ui.ctx(), Some(&res), &mut camera.input, is_active);
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
                let res: Transform = transforms[0].into();
                let game = &mut app_state.game;
                self.transform_edit
                    .get_or_insert_with(|| GizmoDrag::begin(game, game_object))
                    .move_to(game, res.matrix());
                self.gizmo_status(ui, &result);
            }
            gizmo_focused = self.gizmo.is_focused();
        }

        if !ui.input(|input| input.pointer.button_down(PointerButton::Primary)) {
            if let Some(drag) = self.transform_edit.take() {
                // A drag the viewport missed frames of, or whose object went
                // away, may have a capture that no longer matches the scene.
                if viewport_is_continuous && selected_game_object {
                    drag.commit(&mut app_state.scene_document, &app_state.game);
                } else {
                    drag.abandon(&mut app_state.scene_document, &mut app_state.game);
                }
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

/// A gizmo drag of one game object, from its first move to its end.
///
/// A drag that ends normally is one undo step. A drag that cannot be recorded,
/// because the viewport missed frames of it or its object is no longer
/// selected, is abandoned: the object gets back the local transform it had
/// before the drag, so no change is left that undo and the dirty mark do not
/// know about. A transform written since the drag's last move, such as by an
/// undo while the viewport was hidden, is left alone.
struct GizmoDrag {
    edit: SceneEdit,
    /// The dragged object.
    object: Uuid,
    /// The object's local transform before the drag.
    before: Mat4,
    /// The object's local transform after the drag's last move.
    after: Option<Mat4>,
    /// Whether the drag writes a simulation copy of the scene rather than
    /// the authoring scene.
    simulating: bool,
}

impl GizmoDrag {
    fn begin(game: &GameContext, game_object: GameObject) -> Self {
        let scene = game.scenes.simulation_scene();
        Self {
            edit: SceneEdit::begin("Transform game object"),
            object: scene.uuid(game_object),
            before: scene.transform(game_object).matrix(),
            after: None,
            simulating: game.scenes.has_simulation_scene(),
        }
    }

    /// Moves the object to the world transform `matrix`.
    fn move_to(&mut self, game: &mut GameContext, matrix: Mat4) {
        let scene = self.edit.scene_mut(game);
        if let Some(game_object) = scene.find(self.object) {
            scene.set_world_transform(game_object, matrix);
            self.after = Some(scene.transform(game_object).matrix());
        }
    }

    /// Records the drag as one undo step.
    fn commit(self, document: &mut SceneDocument, game: &GameContext) {
        self.edit.commit(document, game);
    }

    /// Ends a drag that cannot be recorded, restoring the object's transform
    /// in the scene the drag wrote unless something wrote it since. A simulation copy that has since been
    /// dropped took the drag's writes with it.
    /// The authoring scene's dirty state then follows the rollback, which
    /// matters when the scene was saved while the drag was pending.
    fn abandon(self, document: &mut SceneDocument, game: &mut GameContext) {
        self.edit.cancel();
        let scene = match (self.simulating, game.scenes.has_simulation_scene()) {
            (false, _) => game.scenes.current_scene_mut(),
            (true, true) => game.scenes.simulation_scene_mut(),
            (true, false) => return,
        };
        let Some(game_object) = scene.find(self.object) else {
            return;
        };
        // Only a transform the drag still owns is rolled back.
        if self.after != Some(scene.transform(game_object).matrix()) {
            return;
        }
        scene.set_transform(game_object, &self.before);
        if !self.simulating {
            document.scene_written_outside_history(game);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GizmoDrag, PanelViewport};
    use crate::scene_document::SceneDocument;
    use engine::context::GameContext;
    use engine::test_support::test_game_context;
    use nalgebra_glm::{translation, vec3, Mat4};
    use std::path::Path;
    use uuid::Uuid;

    fn document() -> SceneDocument {
        SceneDocument::new(Path::new("gizmo-drag-test-project"))
    }

    fn temp_scene_file() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("gizmo-drag-{}.cxscene", Uuid::new_v4()))
    }

    fn game_with_object() -> (GameContext, Uuid, Mat4) {
        let mut game = test_game_context();
        let scene = game.scenes.current_scene_mut();
        let object = scene.create(None, None);
        let start = translation(&vec3(1.0, 2.0, 3.0));
        scene.set_transform(object, &start);
        let id = scene.uuid(object);
        (game, id, start)
    }

    fn local_matrix(game: &GameContext, id: Uuid) -> Mat4 {
        let scene = game.scenes.simulation_scene();
        scene.transform(scene.find(id).unwrap()).matrix()
    }

    fn drag(game: &mut GameContext, id: Uuid) -> GizmoDrag {
        let object = game.scenes.simulation_scene().find(id).unwrap();
        let mut drag = GizmoDrag::begin(game, object);
        drag.move_to(game, translation(&vec3(5.0, 0.0, 0.0)));
        drag.move_to(game, translation(&vec3(9.0, 0.0, 0.0)));
        drag
    }

    #[test]
    fn a_finished_gizmo_drag_is_one_undo_step() {
        let (mut game, id, start) = game_with_object();
        let mut document = SceneDocument::new(Path::new("gizmo-drag-test-project"));

        drag(&mut game, id).commit(&mut document, &game);
        assert_eq!(local_matrix(&game, id), translation(&vec3(9.0, 0.0, 0.0)));
        assert!(document.is_dirty());

        document.undo(&mut game);
        assert_eq!(local_matrix(&game, id), start);
    }

    #[test]
    fn an_abandoned_gizmo_drag_restores_the_transform() {
        let (mut game, id, start) = game_with_object();
        let mut document = document();
        let file = temp_scene_file();
        assert!(document.save(&mut game, file.clone()));

        drag(&mut game, id).abandon(&mut document, &mut game);
        let _ = std::fs::remove_file(&file);

        assert_eq!(local_matrix(&game, id), start, "the drag is rolled back");
        assert!(!document.can_undo(&game));
        assert!(!document.is_dirty());
    }

    #[test]
    fn an_abandoned_simulation_drag_restores_the_simulation_transform() {
        let (mut game, id, start) = game_with_object();
        game.scenes.start_simulation();

        drag(&mut game, id).abandon(&mut document(), &mut game);

        assert_eq!(local_matrix(&game, id), start);
    }

    #[test]
    fn an_abandoned_drag_restores_the_authoring_scene_after_simulation_starts() {
        let (mut game, id, start) = game_with_object();
        let drag = drag(&mut game, id);
        game.scenes.start_simulation();

        drag.abandon(&mut document(), &mut game);

        let scene = game.scenes.current_scene();
        assert_eq!(scene.transform(scene.find(id).unwrap()).matrix(), start);
    }

    #[test]
    fn an_abandoned_drag_keeps_a_transform_written_after_it() {
        // Such as an undo made while the viewport was hidden mid-drag.
        let (mut game, id, _) = game_with_object();
        let drag = drag(&mut game, id);
        let elsewhere = translation(&vec3(0.0, 4.0, 0.0));
        let scene = game.scenes.current_scene_mut();
        let object = scene.find(id).unwrap();
        scene.set_transform(object, &elsewhere);

        drag.abandon(&mut document(), &mut game);

        assert_eq!(local_matrix(&game, id), elsewhere);
    }

    #[test]
    fn an_abandoned_drag_marks_a_scene_saved_during_it_dirty() {
        let (mut game, id, start) = game_with_object();
        let mut document = document();
        let file = temp_scene_file();
        let drag = drag(&mut game, id);
        // Saved while the viewport was hidden, with the drag's transform.
        assert!(document.save(&mut game, file.clone()));
        assert!(!document.is_dirty());

        drag.abandon(&mut document, &mut game);
        let _ = std::fs::remove_file(&file);

        assert_eq!(local_matrix(&game, id), start);
        assert!(
            document.is_dirty(),
            "the file still has the drag's transform"
        );
    }

    #[test]
    fn an_abandoned_drag_of_a_removed_object_does_nothing() {
        let (mut game, id, _) = game_with_object();
        let drag = drag(&mut game, id);
        let scene = game.scenes.current_scene_mut();
        let object = scene.find(id).unwrap();
        scene.delete(object);
        scene.flush_deletes();

        drag.abandon(&mut document(), &mut game);

        assert!(game.scenes.current_scene().find(id).is_none());
    }

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
