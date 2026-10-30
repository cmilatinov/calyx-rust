//! An editor for a [`Curve`], usable in any `Ui`.

use calyx_engine::math::{Curve, CurveInterpolation, CurveKey};
use egui::{
    pos2, vec2, Color32, Id, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Widget,
    WidgetInfo, WidgetType,
};

/// Default height of the curve graph, in points.
const GRAPH_HEIGHT: f32 = 72.0;
/// Radius of a key's handle, in points.
const HANDLE_RADIUS: f32 = 4.0;
/// Share of the value range left empty above and below the keys.
const VALUE_PADDING: f32 = 0.1;

/// Edits a [`Curve`] on a graph of its normalized timeline: drag a key to
/// move it between its neighbours, double-click the graph to add a key, and
/// right-click a key to type its time and value, pick its interpolation or
/// delete it.
///
/// The graph takes the `Ui`'s available width, however narrow, so it never
/// widens its parent. The returned [`Response`] reports
/// [`Response::changed`] on a frame that changed the curve, and the drag
/// start and stop of a key. A drag keeps its state in egui memory under the
/// widget's id, so several editors can share a `Ui`: give each its own
/// [`CurveEditor::id_salt`].
///
/// ```ignore
/// if ui.add(CurveEditor::new(&mut curve).id_salt("size")).changed() {
///     // The curve changed.
/// }
/// ```
pub struct CurveEditor<'a> {
    curve: &'a mut Curve,
    id_salt: Option<Id>,
    height: f32,
}

impl<'a> CurveEditor<'a> {
    pub fn new(curve: &'a mut Curve) -> Self {
        Self {
            curve,
            id_salt: None,
            height: GRAPH_HEIGHT,
        }
    }

    /// Keeps this editor's state apart from other editors in the same `Ui`.
    pub fn id_salt(mut self, id_salt: impl std::hash::Hash) -> Self {
        self.id_salt = Some(Id::new(id_salt));
        self
    }

    /// Height of the graph, in points.
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Paints `curve` as a line across `rect`, as a read-only sketch, such as
    /// for a row that shows the curve's field.
    pub fn paint_preview(ui: &Ui, rect: Rect, curve: &Curve) {
        if ui.is_rect_visible(rect) {
            let transform = GraphTransform {
                rect: rect.shrink(2.0),
                range: ValueRange::fit(curve),
            };
            ui.painter_at(rect).add(curve_line(ui, &transform, curve));
        }
    }
}

impl Widget for CurveEditor<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self {
            curve,
            id_salt,
            height,
        } = self;
        let id = match id_salt {
            Some(salt) => ui.id().with(salt),
            None => ui.next_auto_id(),
        };
        let width = ui.available_width().max(0.0);
        let (rect, graph) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        graph
            .widget_info(|| WidgetInfo::labeled(WidgetType::Other, ui.is_enabled(), "Curve graph"));
        let mut response = graph.clone();

        // Keep the value range still while a key is dragged, so the key
        // stays under the pointer.
        let range_id = id.with("drag_range");
        let range = ui
            .data(|data| data.get_temp::<ValueRange>(range_id))
            .unwrap_or_else(|| ValueRange::fit(curve));
        let transform = GraphTransform {
            rect: rect.shrink(HANDLE_RADIUS),
            range,
        };

        let mut changed = false;
        let mut remove = None;
        for index in 0..curve.keys.len() {
            let key = curve.keys[index];
            let center = transform.to_screen(key.time, key.value);
            let handle = ui.interact(
                Rect::from_center_size(center, vec2(3.0, 3.0) * HANDLE_RADIUS),
                id.with(("key", index)),
                Sense::click_and_drag(),
            );
            handle.widget_info(|| {
                WidgetInfo::labeled(
                    WidgetType::Other,
                    ui.is_enabled(),
                    format!("Curve key {index}"),
                )
            });
            if handle.drag_started() {
                ui.data_mut(|data| data.insert_temp(range_id, range));
            }
            if handle.dragged() {
                if let Some(pointer) = handle.interact_pointer_pos() {
                    let (time, value) = transform.to_curve(pointer);
                    let (min, max) = time_bounds(curve, index);
                    let target = &mut curve.keys[index];
                    let time = time.clamp(min, max);
                    changed |= target.time != time || target.value != value;
                    target.time = time;
                    target.value = value;
                }
            }
            if handle.drag_stopped() {
                ui.data_mut(|data| data.remove::<ValueRange>(range_id));
            }
            handle.context_menu(|ui| {
                changed |= key_menu(ui, curve, index, &mut remove);
            });
            response = response.union(handle);
        }

        if let Some(index) = remove {
            curve.keys.remove(index);
            changed = true;
        }
        if graph.double_clicked() {
            if let Some(pointer) = graph.interact_pointer_pos() {
                let (time, value) = transform.to_curve(pointer);
                let previous = curve.keys.partition_point(|key| key.time <= time);
                let interpolation = previous
                    .checked_sub(1)
                    .map_or(CurveInterpolation::Linear, |index| {
                        curve.keys[index].interpolation
                    });
                curve.insert(CurveKey::new(time, value).with_interpolation(interpolation));
                changed = true;
            }
        }

        if ui.is_rect_visible(rect) {
            paint(ui, rect, &transform, curve, id);
        }
        if changed {
            response.mark_changed();
        }
        response
    }
}

/// The value range the graph shows, with `min < max`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ValueRange {
    min: f32,
    max: f32,
}

impl ValueRange {
    /// The range that fits `curve`'s keys, always including `0.0` to `1.0`
    /// when the keys span less than that, with padding above and below.
    fn fit(curve: &Curve) -> Self {
        let (mut min, mut max) = curve.value_range().unwrap_or((0.0, 1.0));
        if max - min < 1.0 {
            min = min.min(0.0);
            max = max.max(1.0);
        }
        let padding = (max - min) * VALUE_PADDING;
        Self {
            min: min - padding,
            max: max + padding,
        }
    }
}

/// Maps between curve coordinates (time, value) and the graph's rectangle.
#[derive(Clone, Copy)]
struct GraphTransform {
    rect: Rect,
    range: ValueRange,
}

impl GraphTransform {
    fn to_screen(self, time: f32, value: f32) -> Pos2 {
        let y = (value - self.range.min) / (self.range.max - self.range.min);
        pos2(
            self.rect.left() + time * self.rect.width(),
            self.rect.bottom() - y * self.rect.height(),
        )
    }

    fn to_curve(self, pos: Pos2) -> (f32, f32) {
        let time = (pos.x - self.rect.left()) / self.rect.width();
        let y = (self.rect.bottom() - pos.y) / self.rect.height();
        (
            time.clamp(0.0, 1.0),
            self.range.min + y * (self.range.max - self.range.min),
        )
    }
}

/// The times key `index` may take without passing its neighbours.
fn time_bounds(curve: &Curve, index: usize) -> (f32, f32) {
    let min = index
        .checked_sub(1)
        .and_then(|prev| curve.keys.get(prev))
        .map_or(0.0, |key| key.time);
    let max = curve.keys.get(index + 1).map_or(1.0, |key| key.time);
    (min.clamp(0.0, 1.0), max.clamp(0.0, 1.0))
}

fn interpolation_name(interpolation: CurveInterpolation) -> &'static str {
    match interpolation {
        CurveInterpolation::Constant => "Constant",
        CurveInterpolation::Linear => "Linear",
        CurveInterpolation::Smooth => "Smooth",
    }
}

/// The context menu of key `index`: its time and value, its interpolation,
/// and deleting it, which `remove` takes. Returns whether it changed the
/// key.
fn key_menu(ui: &mut Ui, curve: &mut Curve, index: usize, remove: &mut Option<usize>) -> bool {
    let mut changed = false;
    let (min, max) = time_bounds(curve, index);
    let key = &mut curve.keys[index];
    egui::Grid::new("curve_key").num_columns(2).show(ui, |ui| {
        ui.label("Time");
        changed |= ui
            .add(
                egui::DragValue::new(&mut key.time)
                    .speed(0.01)
                    .range(min..=max),
            )
            .changed();
        ui.end_row();
        ui.label("Value");
        changed |= ui
            .add(egui::DragValue::new(&mut key.value).speed(0.01))
            .changed();
        ui.end_row();
    });
    ui.separator();
    for interpolation in [
        CurveInterpolation::Constant,
        CurveInterpolation::Linear,
        CurveInterpolation::Smooth,
    ] {
        changed |= ui
            .radio_value(
                &mut key.interpolation,
                interpolation,
                interpolation_name(interpolation),
            )
            .changed();
    }
    ui.separator();
    let can_remove = curve.keys.len() > 1;
    if ui
        .add_enabled(can_remove, egui::Button::new("Delete Key"))
        .clicked()
    {
        *remove = Some(index);
        ui.close_menu();
    }
    changed
}

/// `curve` drawn as a line through `transform`'s rectangle.
fn curve_line(ui: &Ui, transform: &GraphTransform, curve: &Curve) -> Shape {
    let samples = (transform.rect.width() / 2.0).ceil().max(2.0) as usize;
    let points = (0..=samples)
        .map(|step| {
            let time = step as f32 / samples as f32;
            transform.to_screen(time, curve.evaluate(time))
        })
        .collect();
    let visuals = ui.visuals();
    let line = visuals.selection.stroke.color;
    let line = if line == Color32::TRANSPARENT {
        visuals.strong_text_color()
    } else {
        line
    };
    Shape::line(points, Stroke::new(1.5_f32, line))
}

fn paint(ui: &Ui, rect: Rect, transform: &GraphTransform, curve: &Curve, id: Id) {
    let visuals = ui.visuals();
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        visuals.widgets.inactive.corner_radius,
        visuals.extreme_bg_color,
        visuals.widgets.noninteractive.bg_stroke,
        StrokeKind::Inside,
    );

    let grid = Stroke::new(1.0_f32, visuals.weak_text_color().gamma_multiply(0.25));
    for step in 1..4 {
        let x = transform.to_screen(step as f32 / 4.0, 0.0).x;
        painter.vline(x, rect.y_range(), grid);
    }
    let range = transform.range;
    if range.min < 0.0 && range.max > 0.0 {
        let y = transform.to_screen(0.0, 0.0).y;
        painter.hline(rect.x_range(), y, grid);
    }

    painter.add(curve_line(ui, transform, curve));

    for (index, key) in curve.keys.iter().enumerate() {
        let center = transform.to_screen(key.time, key.value);
        let state = ui.ctx().read_response(id.with(("key", index)));
        let active = state
            .as_ref()
            .is_some_and(|response| response.hovered() || response.dragged());
        let fill = if active {
            visuals.widgets.hovered.fg_stroke.color
        } else {
            visuals.widgets.inactive.fg_stroke.color
        };
        painter.circle(
            center,
            HANDLE_RADIUS,
            fill,
            Stroke::new(1.0_f32, visuals.extreme_bg_color),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_driver::WidgetDriver;

    #[test]
    fn value_range_includes_the_unit_range_for_flat_curves() {
        let range = ValueRange::fit(&Curve::constant(1.0));
        assert!(range.min < 0.0 && range.max > 1.0);
        let range = ValueRange::fit(&Curve::linear(10.0, 20.0));
        assert!(range.min < 10.0 && range.min > 0.0 && range.max > 20.0);
    }

    #[test]
    fn graph_transform_round_trips_points() {
        let transform = GraphTransform {
            rect: Rect::from_min_size(pos2(10.0, 20.0), vec2(100.0, 50.0)),
            range: ValueRange {
                min: -1.0,
                max: 3.0,
            },
        };
        let (time, value) = transform.to_curve(transform.to_screen(0.25, 2.0));
        assert!((time - 0.25).abs() < 1e-5);
        assert!((value - 2.0).abs() < 1e-5);
    }

    #[test]
    fn keys_move_only_between_their_neighbours() {
        let curve = Curve::new(vec![
            CurveKey::new(0.0, 0.0),
            CurveKey::new(0.4, 1.0),
            CurveKey::new(0.9, 0.0),
        ]);
        assert_eq!(time_bounds(&curve, 0), (0.0, 0.4));
        assert_eq!(time_bounds(&curve, 1), (0.0, 0.9));
        assert_eq!(time_bounds(&curve, 2), (0.4, 1.0));
    }

    /// In a bare `Ui`, with no inspector around it, dragging a key moves it
    /// and double-clicking the graph adds one, each reported as a change.
    #[test]
    fn the_editor_drags_and_adds_keys_in_a_bare_ui() {
        let mut driver = WidgetDriver::new();
        let mut curve = Curve::linear(0.0, 1.0);
        let mut draw = |ui: &mut Ui| ui.add(CurveEditor::new(&mut curve).id_salt("curve"));

        assert!(!driver.idle(3, &mut draw).iter().any(Response::changed));
        let key = driver.labelled("Curve key 1").center();
        let drag = driver.drag(key, vec2(-150.0, 20.0), &mut draw);
        assert!(drag.iter().any(Response::changed));
        assert!(drag.iter().any(Response::drag_started));
        assert!(drag.iter().any(Response::drag_stopped));

        let graph = driver.labelled("Curve graph").center();
        let add = driver.double_click(graph, &mut draw);
        assert_eq!(add.iter().filter(|response| response.changed()).count(), 1);
        drop(draw);
        assert_eq!(curve.keys.len(), 3);
        assert!(curve.keys[2].time < 1.0, "{curve:?}");
    }

    /// The graph takes no more width than its `Ui` has, however narrow.
    #[test]
    fn the_editor_fits_a_narrow_ui() {
        let mut driver = WidgetDriver::new();
        let mut curve = Curve::linear(0.0, 1.0);
        driver.idle(1, &mut |ui: &mut Ui| {
            ui.allocate_ui(vec2(40.0, 200.0), |ui| ui.add(CurveEditor::new(&mut curve)))
                .inner
        });
        let rect = driver.labelled("Curve graph");
        assert!(rect.width() <= 40.0, "{rect:?}");
    }
}
