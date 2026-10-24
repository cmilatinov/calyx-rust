use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use egui::{
    pos2, vec2, Color32, Id, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    WidgetInfo, WidgetType,
};
use engine::math::{Curve, CurveInterpolation, CurveKey};
use engine::reflect::{Reflect, ReflectDefault};
use engine::utils::TypeUuid;
use uuid::Uuid;

/// Height of the curve graph, in points.
const GRAPH_HEIGHT: f32 = 72.0;
/// Radius of a key's handle, in points.
const HANDLE_RADIUS: f32 = 4.0;
/// Share of the value range left empty above and below the keys.
const VALUE_PADDING: f32 = 0.1;

/// Edits a [`Curve`] on a graph of its normalized timeline: drag a key to
/// move it, double-click the graph to add a key, and right-click a key to
/// type its time and value, pick its interpolation or delete it.
#[derive(Default, Clone, TypeUuid, Reflect)]
#[uuid = "fb42379b-3cb7-49fd-aedd-780a0fef6186"]
#[reflect(Default, TypeInspector)]
#[repr(C)]
pub struct CurveInspector;

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
    fn to_screen(&self, time: f32, value: f32) -> Pos2 {
        let y = (value - self.range.min) / (self.range.max - self.range.min);
        pos2(
            self.rect.left() + time * self.rect.width(),
            self.rect.bottom() - y * self.rect.height(),
        )
    }

    fn from_screen(&self, pos: Pos2) -> (f32, f32) {
        let time = (pos.x - self.rect.left()) / self.rect.width();
        let y = (self.rect.bottom() - pos.y) / self.rect.height();
        (
            time.clamp(0.0, 1.0),
            self.range.min + y * (self.range.max - self.range.min),
        )
    }
}

/// Reports a widget's change and drag, but not its keyboard focus: a popup
/// can close while one of its fields holds focus, and the focus would then
/// never report its end.
fn without_focus(response: &Response) -> InspectorResponse {
    InspectorResponse {
        changed: response.changed(),
        drag_started: response.drag_started(),
        drag_stopped: response.drag_stopped(),
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

impl CurveInspector {
    /// Draws the editor for `curve` and reports what it did to it.
    pub fn curve_editor(ui: &mut Ui, id: Id, curve: &mut Curve) -> InspectorResponse {
        let mut response = InspectorResponse::NONE;
        let width = ui.available_width().max(4.0 * GRAPH_HEIGHT / 3.0);
        let (rect, graph) = ui.allocate_exact_size(vec2(width, GRAPH_HEIGHT), Sense::click());
        graph.widget_info(|| WidgetInfo::labeled(WidgetType::Other, ui.is_enabled(), "Curve"));

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
            let mut moved = false;
            if handle.dragged() {
                if let Some(pointer) = handle.interact_pointer_pos() {
                    let (time, value) = transform.from_screen(pointer);
                    let (min, max) = time_bounds(curve, index);
                    let target = &mut curve.keys[index];
                    let time = time.clamp(min, max);
                    moved = target.time != time || target.value != value;
                    target.time = time;
                    target.value = value;
                }
            }
            if handle.drag_stopped() {
                ui.data_mut(|data| data.remove::<ValueRange>(range_id));
            }
            response |= InspectorResponse {
                changed: moved,
                drag_started: handle.drag_started(),
                drag_stopped: handle.drag_stopped(),
            };
            handle.context_menu(|ui| {
                response |= Self::key_menu(ui, curve, index, &mut remove);
            });
        }

        if let Some(index) = remove {
            curve.keys.remove(index);
            response |= InspectorResponse::CHANGED;
        }
        if graph.double_clicked() {
            if let Some(pointer) = graph.interact_pointer_pos() {
                let (time, value) = transform.from_screen(pointer);
                let previous = curve.keys.partition_point(|key| key.time <= time);
                let interpolation = previous
                    .checked_sub(1)
                    .map_or(CurveInterpolation::Linear, |index| {
                        curve.keys[index].interpolation
                    });
                curve.insert(CurveKey::new(time, value).with_interpolation(interpolation));
                response |= InspectorResponse::CHANGED;
            }
        }

        if ui.is_rect_visible(rect) {
            Self::paint(ui, rect, &transform, curve, id);
        }
        response
    }

    /// The context menu of key `index`: its time and value, its
    /// interpolation, and deleting it, which `remove` takes.
    fn key_menu(
        ui: &mut Ui,
        curve: &mut Curve,
        index: usize,
        remove: &mut Option<usize>,
    ) -> InspectorResponse {
        let mut response = InspectorResponse::NONE;
        let (min, max) = time_bounds(curve, index);
        let key = &mut curve.keys[index];
        egui::Grid::new("curve_key").num_columns(2).show(ui, |ui| {
            ui.label("Time");
            response |= without_focus(
                &ui.add(
                    egui::DragValue::new(&mut key.time)
                        .speed(0.01)
                        .range(min..=max),
                ),
            );
            ui.end_row();
            ui.label("Value");
            response |= without_focus(&ui.add(egui::DragValue::new(&mut key.value).speed(0.01)));
            ui.end_row();
        });
        ui.separator();
        for interpolation in [
            CurveInterpolation::Constant,
            CurveInterpolation::Linear,
            CurveInterpolation::Smooth,
        ] {
            response |= without_focus(&ui.radio_value(
                &mut key.interpolation,
                interpolation,
                interpolation_name(interpolation),
            ));
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
        response
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

        let samples = (transform.rect.width() / 2.0).ceil().max(2.0) as usize;
        let points = (0..=samples)
            .map(|step| {
                let time = step as f32 / samples as f32;
                transform.to_screen(time, curve.evaluate(time))
            })
            .collect();
        let line = ui.visuals().selection.stroke.color;
        let line = if line == Color32::TRANSPARENT {
            visuals.strong_text_color()
        } else {
            line
        };
        painter.add(Shape::line(points, Stroke::new(1.5_f32, line)));

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
}

impl TypeInspector for CurveInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        vec![Curve::type_uuid()]
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        let Some(curve) = instance.downcast_mut::<Curve>() else {
            return InspectorResponse::NONE;
        };
        let id = ui.id().with(ctx.field_name).with("curve");
        Self::curve_editor(ui, id, curve)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let (time, value) = transform.from_screen(transform.to_screen(0.25, 2.0));
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
}
