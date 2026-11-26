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
