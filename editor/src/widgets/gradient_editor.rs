//! An editor for a [`Gradient`], usable in any `Ui`.

use egui::color_picker::{color_edit_button_hsva, Alpha};
use egui::ecolor::{gamma_from_linear, linear_from_gamma, Hsva};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    pos2, vec2, Color32, Id, Key, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    Widget, WidgetInfo, WidgetType,
};
use engine::math::{Gradient, GradientKey};
use nalgebra_glm::{vec4, Vec4};

/// Default height of the gradient strip, in points.
const STRIP_HEIGHT: f32 = 20.0;
/// Height of the row of key markers under the strip, in points.
const MARKER_HEIGHT: f32 = 12.0;
/// Width of a key marker, in points.
const MARKER_WIDTH: f32 = 10.0;
/// Side of a checkerboard square behind the strip, in points.
const CHECKER_SIZE: f32 = 5.0;

/// Edits a [`Gradient`] on a strip of its colors over its normalized
/// timeline, drawn over a checkerboard so alpha shows, with a marker under
/// the strip for each key:
///
/// - drag a marker to move its key in time, between its neighbours;
/// - click the strip to add a key there, of the gradient's color there;
/// - click a marker to select its key, then edit its color, alpha included,
///   and its time in the row under the markers;
/// - delete a key from its marker's context menu, or press Delete while its
///   marker is selected and focused. A gradient keeps at least one key.
///
/// The strip takes the `Ui`'s available width, however narrow, so it never
/// widens its parent. The returned [`Response`] reports
/// [`Response::changed`] on a frame that changed the gradient, and the drag
/// start and stop of a marker. Selection, drag and picker state live in egui
/// memory under the widget's id, so several editors can share a `Ui`: give
/// each its own [`GradientEditor::id_salt`].
///
/// ```ignore
/// if ui.add(GradientEditor::new(&mut gradient).id_salt("color")).changed() {
///     // The gradient changed.
/// }
/// ```
pub struct GradientEditor<'a> {
    gradient: &'a mut Gradient,
    id_salt: Option<Id>,
    height: f32,
}

impl<'a> GradientEditor<'a> {
    pub fn new(gradient: &'a mut Gradient) -> Self {
        Self {
            gradient,
            id_salt: None,
            height: STRIP_HEIGHT,
        }
    }

    /// Keeps this editor's state apart from other editors in the same `Ui`.
    pub fn id_salt(mut self, id_salt: impl std::hash::Hash) -> Self {
        self.id_salt = Some(Id::new(id_salt));
        self
    }

    /// Height of the strip, in points.
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Paints `gradient` across `rect` over a checkerboard, as a read-only
    /// sketch, such as for a row that shows the gradient's field.
    pub fn paint_preview(ui: &Ui, rect: Rect, gradient: &Gradient) {
        if ui.is_rect_visible(rect) {
            paint_strip(ui, rect, gradient);
        }
    }
}

impl Widget for GradientEditor<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self {
            gradient,
            id_salt,
            height,
        } = self;
        let id = match id_salt {
            Some(salt) => ui.id().with(salt),
            None => ui.next_auto_id(),
        };
        let selected_id = id.with("selected");
        let mut selected = ui
            .data(|data| data.get_temp::<usize>(selected_id))
            .filter(|index| *index < gradient.keys.len());

        let width = ui.available_width().max(0.0);
        let (rect, strip) =
            ui.allocate_exact_size(vec2(width, height + MARKER_HEIGHT), Sense::click());
        strip.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Other, ui.is_enabled(), "Gradient strip")
        });
        let strip_rect = Rect::from_min_size(rect.min, vec2(width, height));
        let span = TimeSpan::new(strip_rect);
        let mut response = strip.clone();
        let mut changed = false;

        let mut remove = None;
        for index in 0..gradient.keys.len() {
            let time = gradient.keys[index].time;
            let marker_rect = Rect::from_min_size(
                pos2(span.to_x(time) - MARKER_WIDTH / 2.0, strip_rect.bottom()),
                vec2(MARKER_WIDTH, MARKER_HEIGHT),
            );
            let marker = ui.interact(
                marker_rect,
                id.with(("key", index)),
                Sense::click_and_drag(),
            );
            marker.widget_info(|| {
                WidgetInfo::labeled(
                    WidgetType::Other,
                    ui.is_enabled(),
                    format!("Gradient key {index}"),
                )
            });
            if marker.clicked() || marker.drag_started() {
                selected = Some(index);
                marker.request_focus();
            }
            if marker.dragged() {
                if let Some(pointer) = marker.interact_pointer_pos() {
                    let (min, max) = time_bounds(gradient, index);
                    let time = span.to_time(pointer.x).clamp(min, max);
                    let key = &mut gradient.keys[index];
                    changed |= key.time != time;
                    key.time = time;
                }
            }
            let can_remove = gradient.keys.len() > 1;
            if can_remove
                && marker.has_focus()
                && selected == Some(index)
                && ui.input(|input| input.key_pressed(Key::Delete))
            {
                remove = Some(index);
            }
            marker.context_menu(|ui| {
                if ui
                    .add_enabled(can_remove, egui::Button::new("Delete Key"))
                    .clicked()
                {
                    remove = Some(index);
                    ui.close_menu();
                }
            });
            response = response.union(marker);
        }

        if let Some(index) = remove {
            gradient.keys.remove(index);
            selected = None;
            changed = true;
        }
        if strip.clicked() {
            if let Some(pointer) = strip.interact_pointer_pos() {
                if strip_rect.contains(pointer) {
                    let time = span.to_time(pointer.x);
                    let color = gradient.evaluate(time);
                    selected = Some(gradient.insert(GradientKey::new(time, color)));
                    changed = true;
                }
            }
        }

        if ui.is_rect_visible(rect) {
            paint_strip(ui, strip_rect, gradient);
            paint_markers(ui, strip_rect, &span, gradient, selected, id);
        }

        if let Some(index) = selected {
            changed |= key_row(ui, id, gradient, index);
        }
        ui.data_mut(|data| match selected {
            Some(index) => data.insert_temp(selected_id, index),
            None => data.remove::<usize>(selected_id),
        });
        if changed {
            response.mark_changed();
        }
        response
    }
}

/// Maps between gradient time and x on the strip.
struct TimeSpan {
    left: f32,
    width: f32,
}

impl TimeSpan {
    /// The span of `strip`, inset by half a marker so the end markers stay
    /// inside it.
    fn new(strip: Rect) -> Self {
        let inset = (MARKER_WIDTH / 2.0).min(strip.width() / 2.0);
        Self {
            left: strip.left() + inset,
            width: (strip.width() - 2.0 * inset).max(f32::EPSILON),
        }
    }

    fn to_x(&self, time: f32) -> f32 {
        self.left + time.clamp(0.0, 1.0) * self.width
    }

    fn to_time(&self, x: f32) -> f32 {
        ((x - self.left) / self.width).clamp(0.0, 1.0)
    }
}

/// The times key `index` may take without passing its neighbours.
fn time_bounds(gradient: &Gradient, index: usize) -> (f32, f32) {
    let min = index
        .checked_sub(1)
        .and_then(|prev| gradient.keys.get(prev))
        .map_or(0.0, |key| key.time);
    let max = gradient.keys.get(index + 1).map_or(1.0, |key| key.time);
    (min.clamp(0.0, 1.0), max.clamp(0.0, 1.0))
}

/// An unmultiplied sRGB RGBA color as egui paints it.
fn color32(color: Vec4) -> Color32 {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(color.x), byte(color.y), byte(color.z), byte(color.w))
}

/// An unmultiplied sRGB RGBA color as the color picker edits it.
fn hsva(color: Vec4) -> Hsva {
    Hsva::from_rgba_unmultiplied(
        linear_from_gamma(color.x),
        linear_from_gamma(color.y),
        linear_from_gamma(color.z),
        color.w,
    )
}

/// The unmultiplied sRGB RGBA color the color picker picked.
fn srgba(hsva: Hsva) -> Vec4 {
    let [r, g, b, a] = hsva.to_rgba_unmultiplied();
    vec4(
        gamma_from_linear(r),
        gamma_from_linear(g),
        gamma_from_linear(b),
        a,
    )
}

/// The row of the selected key: its color, in a picker with alpha, and its
/// time. Returns whether it changed the key.
fn key_row(ui: &mut Ui, id: Id, gradient: &mut Gradient, index: usize) -> bool {
    let mut changed = false;
    let (min, max) = time_bounds(gradient, index);
    let key = &mut gradient.keys[index];
    // Keep the picker's own color while it matches the key, so its hue holds
    // through grays and black.
    let picker_id = id.with("picker");
    let mut picked = ui
        .data(|data| data.get_temp::<(usize, Hsva)>(picker_id))
        .filter(|(picked, hsva)| *picked == index && srgba(*hsva) == key.color)
        .map_or_else(|| hsva(key.color), |(_, hsva)| hsva);
    ui.horizontal(|ui| {
        ui.label("Color");
        let button = color_edit_button_hsva(ui, &mut picked, Alpha::OnlyBlend);
        button.widget_info(|| {
            WidgetInfo::labeled(WidgetType::ColorButton, ui.is_enabled(), "Key color")
        });
        if button.changed() {
            key.color = srgba(picked);
            changed = true;
        }
        ui.label("Time");
        changed |= ui
            .add(
                egui::DragValue::new(&mut key.time)
                    .speed(0.01)
                    .range(min..=max),
            )
            .changed();
    });
    ui.data_mut(|data| data.insert_temp(picker_id, (index, picked)));
    changed
}

/// The strip: a checkerboard, then the gradient's colors over it.
fn paint_strip(ui: &Ui, rect: Rect, gradient: &Gradient) {
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    let (light, dark) = (Color32::from_gray(204), Color32::from_gray(140));
    let columns = (rect.width() / CHECKER_SIZE).ceil() as usize;
    let rows = (rect.height() / CHECKER_SIZE).ceil() as usize;
    painter.rect_filled(rect, 0.0, light);
    for row in 0..rows {
        for column in (row % 2..columns).step_by(2) {
            let min = rect.min + vec2(column as f32, row as f32) * CHECKER_SIZE;
            painter.rect_filled(
                Rect::from_min_size(min, vec2(CHECKER_SIZE, CHECKER_SIZE)),
                0.0,
                dark,
            );
        }
    }

    let span = TimeSpan::new(rect);
    let samples = (rect.width() / 2.0).ceil().max(1.0) as usize;
    let mut mesh = Mesh::default();
    for step in 0..=samples {
        let x = rect.left() + rect.width() * step as f32 / samples as f32;
        let color = color32(gradient.evaluate(span.to_time(x)));
        let base = mesh.vertices.len() as u32;
        for y in [rect.top(), rect.bottom()] {
            mesh.vertices.push(Vertex {
                pos: pos2(x, y),
                uv: WHITE_UV,
                color,
            });
        }
        if step > 0 {
            mesh.add_triangle(base - 2, base - 1, base);
            mesh.add_triangle(base - 1, base + 1, base);
        }
    }
    painter.add(Shape::mesh(mesh));
    painter.rect_stroke(
        rect,
        0.0,
        visuals.widgets.noninteractive.bg_stroke,
        StrokeKind::Inside,
    );
}

/// A marker under the strip for each key: a notch pointing at its time and
/// a swatch of its color, opaque.
fn paint_markers(
    ui: &Ui,
    strip: Rect,
    span: &TimeSpan,
    gradient: &Gradient,
    selected: Option<usize>,
    id: Id,
) {
    let painter = ui.painter();
    let visuals = ui.visuals();
    for (index, key) in gradient.keys.iter().enumerate() {
        let x = span.to_x(key.time);
        let top = strip.bottom();
        let state = ui.ctx().read_response(id.with(("key", index)));
        let active = state
            .as_ref()
            .is_some_and(|response| response.hovered() || response.dragged());
        let stroke = if selected == Some(index) {
            visuals.selection.stroke
        } else if active {
            visuals.widgets.hovered.fg_stroke
        } else {
            visuals.widgets.inactive.fg_stroke
        };
        let notch: Vec<Pos2> = vec![
            pos2(x, top),
            pos2(x + MARKER_WIDTH / 2.0, top + 4.0),
            pos2(x - MARKER_WIDTH / 2.0, top + 4.0),
        ];
        painter.add(Shape::convex_polygon(notch, stroke.color, Stroke::NONE));
        let swatch = Rect::from_min_max(
            pos2(x - MARKER_WIDTH / 2.0, top + 4.0),
            pos2(x + MARKER_WIDTH / 2.0, top + MARKER_HEIGHT),
        );
        let opaque = Vec4::new(key.color.x, key.color.y, key.color.z, 1.0);
        painter.rect(
            swatch,
            1.0,
            color32(opaque),
            Stroke::new(1.0_f32, stroke.color),
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_driver::WidgetDriver;
    use egui::PointerButton;

    fn gradient() -> Gradient {
        Gradient::new(vec![
            GradientKey::rgba(0.0, [1.0, 0.0, 0.0, 1.0]),
            GradientKey::rgba(0.5, [0.0, 1.0, 0.0, 1.0]),
            GradientKey::rgba(1.0, [0.0, 0.0, 1.0, 0.0]),
        ])
    }

    #[test]
    fn keys_move_only_between_their_neighbours() {
        let gradient = gradient();
        assert_eq!(time_bounds(&gradient, 0), (0.0, 0.5));
        assert_eq!(time_bounds(&gradient, 1), (0.0, 1.0));
        assert_eq!(time_bounds(&gradient, 2), (0.5, 1.0));
    }

    #[test]
    fn time_span_round_trips_times() {
        let span = TimeSpan::new(Rect::from_min_size(pos2(10.0, 0.0), vec2(110.0, 20.0)));
        assert_eq!(span.to_x(0.0), 15.0);
        assert_eq!(span.to_x(1.0), 115.0);
        assert!((span.to_time(span.to_x(0.3)) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn picker_colors_round_trip() {
        let color = vec4(0.8, 0.25, 0.1, 0.4);
        let back = srgba(hsva(color));
        for channel in 0..4 {
            assert!((back[channel] - color[channel]).abs() < 1e-5, "{back:?}");
        }
    }

    /// In a bare `Ui`, with no inspector around it, dragging a marker moves
    /// its key and clicking the strip adds one, each reported as a change.
    #[test]
    fn the_editor_drags_and_adds_keys_in_a_bare_ui() {
        let mut driver = WidgetDriver::new();
        let mut gradient = gradient();
        let mut draw = |ui: &mut Ui| ui.add(GradientEditor::new(&mut gradient).id_salt("color"));

        assert!(!driver.idle(3, &mut draw).iter().any(Response::changed));
        let marker = driver.labelled("Gradient key 1").center();
        let drag = driver.drag(marker, vec2(-100.0, 0.0), &mut draw);
        assert!(drag.iter().any(Response::changed));
        assert!(drag.iter().any(Response::drag_started));
        assert!(drag.iter().any(Response::drag_stopped));

        let strip = driver.labelled("Gradient strip");
        let point = pos2(strip.left() + strip.width() * 0.75, strip.top() + 5.0);
        let add = driver.click(point, &mut draw);
        assert_eq!(add.iter().filter(|response| response.changed()).count(), 1);
        drop(draw);
        assert_eq!(gradient.keys.len(), 4);
        assert!(gradient.keys[1].time < 0.5, "{gradient:?}");
        let added = gradient.keys[2];
        assert!((added.time - 0.75).abs() < 0.05, "{gradient:?}");
        let between = &gradient.keys[3];
        assert!(added.color.z > 0.0 && added.color.z < between.color.z);
    }

    /// Clicking a marker selects its key and shows its color button, which
    /// opens a picker; Delete then removes the key, but never the last one.
    #[test]
    fn a_selected_key_shows_its_picker_and_deletes() {
        let mut driver = WidgetDriver::new();
        let mut gradient = gradient();
        let mut draw = |ui: &mut Ui| ui.add(GradientEditor::new(&mut gradient).id_salt("color"));

        driver.idle(2, &mut draw);
        assert!(!driver
            .labelled_widgets()
            .iter()
            .any(|w| w.label == "Key color"));
        let marker = driver.labelled("Gradient key 2").center();
        driver.click(marker, &mut draw);
        let button = driver.labelled("Key color").center();
        driver.click(button, &mut draw);
        // The picker's popup shows its number format switch.
        driver.labelled("U8");
        driver.press_key(Key::Escape, &mut draw);

        let marker = driver.labelled("Gradient key 2").center();
        driver.click(marker, &mut draw);
        let delete = driver.press_key(Key::Delete, &mut draw);
        assert!(delete.iter().any(Response::changed));
        drop(draw);
        assert_eq!(gradient.keys.len(), 2);

        let mut single = Gradient::new(vec![GradientKey::rgba(0.5, [1.0; 4])]);
        let mut draw = |ui: &mut Ui| ui.add(GradientEditor::new(&mut single).id_salt("one"));
        driver.idle(2, &mut draw);
        let marker = driver.labelled("Gradient key 0").center();
        driver.click(marker, &mut draw);
        driver.press_key(Key::Delete, &mut draw);
        driver.click_with(PointerButton::Secondary, marker, &mut draw);
        drop(draw);
        assert_eq!(single.keys.len(), 1);
    }

    /// The strip takes no more width than its `Ui` has, however narrow.
    #[test]
    fn the_editor_fits_a_narrow_ui() {
        let mut driver = WidgetDriver::new();
        let mut gradient = gradient();
        driver.idle(1, &mut |ui: &mut Ui| {
            ui.allocate_ui(vec2(40.0, 200.0), |ui| {
                ui.add(GradientEditor::new(&mut gradient))
            })
            .inner
        });
        let rect = driver.labelled("Gradient strip");
        assert!(rect.width() <= 40.0, "{rect:?}");
    }
}
