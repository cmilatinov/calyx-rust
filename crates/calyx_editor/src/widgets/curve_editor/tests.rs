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
