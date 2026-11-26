use super::*;

/// Lays out [`ListButtons`] in the one-button slot a list item gives its
/// button, and returns the slot, the buttons' combined rect and
/// [`ListButtons::width`].
fn layout_in_slot() -> (Rect, Rect, f32) {
    let context = egui::Context::default();
    let mut result = None;
    let _ = context.run(Default::default(), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            let side = ListButtons::button_size(ui).x;
            let slot = Rect::from_center_size(pos2(200.0 - side / 2.0, 50.0), Vec2::splat(side));
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(slot)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            let response = Box::new(ListButtons::new()).ui(&mut child);
            result = Some((slot, response.rect, ListButtons::width(ui)));
        });
    });
    result.unwrap()
}

#[test]
fn list_buttons_end_at_their_slot() {
    let (slot, buttons, _) = layout_in_slot();
    assert_eq!(buttons.right(), slot.right());
    // egui rounds widget rects to whole points.
    assert!((buttons.center().y - slot.center().y).abs() <= 1.0);
}

#[test]
fn list_buttons_stay_inside_the_clip_rect() {
    let context = egui::Context::default();
    let mut result = None;
    let _ = context.run(Default::default(), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            let side = ListButtons::button_size(ui).x;
            // A slot that ends past the visible area, as in a panel whose
            // content is wider than the panel.
            let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 100.0));
            let slot = Rect::from_center_size(pos2(210.0, 50.0), Vec2::splat(side));
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(slot)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            child.set_clip_rect(clip);
            let response = Box::new(ListButtons::new()).ui(&mut child);
            result = Some((clip, response.rect));
        });
    });
    let (clip, buttons) = result.unwrap();
    assert_eq!(buttons.right(), clip.right());
}

#[test]
fn list_buttons_width_matches_their_layout() {
    let (_, buttons, width) = layout_in_slot();
    assert_eq!(buttons.width(), width);
}
