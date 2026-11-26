use crate::panel::Panel;
use crate::scene_document::SceneDocument;
use crate::scene_edit::SceneEdit;
use crate::selection::SelectionType;
use crate::{icons, EditorAppState};
use calyx_engine::context::GameContext;
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::render::{CameraLike, GBufferView};
use calyx_engine::scene::GameObject;
use egui::epaint::Vertex;
use egui::load::SizedTexture;
use egui::Ui;
use egui::{
    Align2, Color32, Image, ImageSource, Key, Mesh, Modifiers, PointerButton, Pos2, Response, Rgba,
    Sense, TextStyle,
};
use mint::ColumnMatrix4;
use nalgebra_glm::{DMat4, Mat4};
use re_ui::{Icon, UiExt};
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
    /// Set when a drag was abandoned before the pointer's release, such as
    /// when the selection changed mid-drag: the gizmo's results are ignored
    /// until the release, since they are relative to the abandoned drag.
    drag_suspended: bool,
    last_viewport_pass: Option<u64>,
}

impl Default for PanelViewport {
    fn default() -> Self {
        Self {
            gizmo: Gizmo::new(GizmoConfig::default()),
            transform_edit: None,
            drag_suspended: false,
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
            ui.separator();
            // re_ui's style grows hovered and pressed widgets by 2 points,
            // which pushes a plain checkbox's box into its label;
            // `re_checkbox` keeps the box its resting size.
            ui.re_checkbox(
                &mut app_state.preview_particles,
                "Simulate particles in edit mode",
            )
            .on_hover_text("Run the edit scene's particle systems while play is stopped");
            ui.separator();
            Self::view_mode_combo(ui, &mut app_state.scene_renderer.options_mut().gbuffer_view);
        });
        ui.add_space(-ui.style().spacing.item_spacing.y + padding);
    }

    /// Picks what the scene view shows: the lit image, or one G-buffer
    /// target in its place.
    fn view_mode_combo(ui: &mut Ui, view: &mut Option<GBufferView>) {
        let label = |view: Option<GBufferView>| view.map_or("Lit", GBufferView::label);
        egui::ComboBox::from_id_salt("viewport_view_mode")
            .selected_text(label(*view))
            .show_ui(ui, |ui| {
                ui.selectable_value(view, None, label(None));
                for mode in GBufferView::ALL {
                    ui.selectable_value(view, Some(mode), label(Some(mode)));
                }
            })
            .response
            .on_hover_text("Show the lit scene, or one target of the deferred G-buffer");
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

        let selected = app_state
            .selection
            .first(SelectionType::GameObject)
            .filter(|id| app_state.game.scenes.simulation_scene().find(*id).is_some());
        self.end_mismatched_drag(&mut app_state.scene_document, &mut app_state.game, selected);

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
                if !self.drag_suspended {
                    let res: Transform = transforms[0].into();
                    let game = &mut app_state.game;
                    let document = &app_state.scene_document;
                    self.transform_edit
                        .get_or_insert_with(|| GizmoDrag::begin(document, game, game_object))
                        .move_to(game, res.matrix());
                    self.gizmo_status(ui, &result);
                }
            }
            gizmo_focused = self.gizmo.is_focused();
        }

        if !ui.input(|input| input.pointer.button_down(PointerButton::Primary)) {
            self.drag_suspended = false;
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

    /// Abandons the pending drag when `selected`, the selected object, is no
    /// longer the object it drags, and ignores the gizmo until the pointer's
    /// release.
    fn end_mismatched_drag(
        &mut self,
        document: &mut SceneDocument,
        game: &mut GameContext,
        selected: Option<Uuid>,
    ) {
        if self
            .transform_edit
            .as_ref()
            .is_some_and(|drag| Some(drag.object) != selected)
        {
            if let Some(drag) = self.transform_edit.take() {
                drag.abandon(document, game);
            }
            self.drag_suspended = true;
        }
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
/// know about. That holds only while nothing else has used the drag's writes:
/// once an edit was recorded, an undo or redo made, or the scene saved since
/// the drag began, every snapshot taken since holds the dragged transform, so
/// the drag is kept as part of them instead. A transform written since the
/// drag's last move is also left alone.
struct GizmoDrag {
    edit: SceneEdit,
    /// The dragged object.
    object: Uuid,
    /// The object's local transform before the drag.
    before: Mat4,
    /// The object's local transform after the drag's last move.
    after: Option<Mat4>,
    /// The scene document's revision when the drag began.
    revision: u64,
    /// Whether the drag writes a simulation copy of the scene rather than
    /// the authoring scene.
    simulating: bool,
}

impl GizmoDrag {
    fn begin(document: &SceneDocument, game: &GameContext, game_object: GameObject) -> Self {
        let scene = game.scenes.simulation_scene();
        Self {
            edit: SceneEdit::begin("Transform game object"),
            object: scene.uuid(game_object),
            before: scene.transform(game_object).matrix(),
            after: None,
            revision: document.revision(),
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
    /// in the scene the drag wrote unless something wrote or captured it
    /// since. A simulation copy that has since been dropped took the drag's
    /// writes with it. A rollback of the authoring scene goes to the document
    /// as a write outside its history.
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
        // Only a transform the drag still owns, and that no snapshot or save
        // has taken since, is rolled back.
        if self.after != Some(scene.transform(game_object).matrix())
            || (!self.simulating && document.revision() != self.revision)
        {
            return;
        }
        scene.set_transform(game_object, &self.before);
        if !self.simulating {
            document.scene_written_outside_history(game);
        }
    }
}

#[cfg(test)]
mod tests;
