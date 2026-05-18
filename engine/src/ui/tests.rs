use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{ReflectedType, TypeInfo};

use super::*;

fn viewport(width: f32, height: f32) -> UiRect {
    UiRect::from_min_size(UiPoint::ZERO, UiSize::new(width, height))
}

fn pointer(x: f32, y: f32, down: bool) -> UiInput {
    pointer_dt(x, y, down, 0.0)
}

fn pointer_dt(x: f32, y: f32, down: bool, delta_time: f32) -> UiInput {
    UiInput {
        pointer_position: Some(UiPoint::new(x, y)),
        pointer_down: down,
        delta_time,
    }
}

fn no_pointer(delta_time: f32) -> UiInput {
    UiInput {
        pointer_position: None,
        pointer_down: false,
        delta_time,
    }
}

fn node_rect(frame: &UiFrame, id: &str) -> UiRect {
    frame
        .layout
        .find(&ElementId::from(id))
        .unwrap_or_else(|| panic!("missing node {id}"))
        .rect
}

#[test]
fn row_layout_applies_flex_gap_padding_and_constraints() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let fixed = ui
        .sized_box()
        .id(&mut ui, "fixed")
        .width(&mut ui, UiLength::Px(50.0))
        .height(&mut ui, UiLength::Px(20.0));
    let flex = ui
        .sized_box()
        .id(&mut ui, "flex")
        .width(&mut ui, UiLength::Fill)
        .height(&mut ui, UiLength::Px(20.0))
        .flex_grow(&mut ui, 1.0);
    let root = ui
        .row()
        .id(&mut ui, "root")
        .padding(&mut ui, EdgeInsets::all(10.0))
        .gap(&mut ui, 5.0)
        .child(&mut ui, fixed)
        .child(&mut ui, flex);

    let frame = runtime.frame(&ui, root, viewport(200.0, 60.0), UiInput::default(), &theme);

    assert_eq!(node_rect(&frame, "root"), viewport(200.0, 60.0));
    assert_eq!(
        node_rect(&frame, "fixed"),
        UiRect::from_min_size(UiPoint::new(10.0, 10.0), UiSize::new(50.0, 20.0))
    );
    assert_eq!(
        node_rect(&frame, "flex"),
        UiRect::from_min_size(UiPoint::new(65.0, 10.0), UiSize::new(125.0, 20.0))
    );
}

#[test]
fn row_layout_shrinks_fixed_children_when_they_overflow() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let first = ui
        .sized_box()
        .id(&mut ui, "first")
        .width(&mut ui, UiLength::Px(80.0))
        .height(&mut ui, UiLength::Px(10.0));
    let second = ui
        .sized_box()
        .id(&mut ui, "second")
        .width(&mut ui, UiLength::Px(80.0))
        .height(&mut ui, UiLength::Px(10.0));
    let root = ui
        .row()
        .id(&mut ui, "root")
        .children(&mut ui, [first, second]);

    let frame = runtime.frame(&ui, root, viewport(100.0, 40.0), UiInput::default(), &theme);

    assert_eq!(node_rect(&frame, "first").width(), 50.0);
    assert_eq!(node_rect(&frame, "second").min.x, 50.0);
    assert_eq!(node_rect(&frame, "second").width(), 50.0);
}

#[test]
fn flex_shrink_zero_keeps_child_size_when_space_is_tight() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let fixed = ui
        .sized_box()
        .id(&mut ui, "fixed")
        .width(&mut ui, UiLength::Px(70.0))
        .height(&mut ui, UiLength::Px(10.0))
        .flex_shrink(&mut ui, 0.0);
    let flexible = ui
        .sized_box()
        .id(&mut ui, "flexible")
        .width(&mut ui, UiLength::Px(70.0))
        .height(&mut ui, UiLength::Px(10.0));
    let root = ui
        .row()
        .id(&mut ui, "root")
        .children(&mut ui, [fixed, flexible]);

    let frame = runtime.frame(&ui, root, viewport(100.0, 40.0), UiInput::default(), &theme);

    assert_eq!(node_rect(&frame, "fixed").width(), 70.0);
    assert_eq!(node_rect(&frame, "flexible").min.x, 70.0);
    assert_eq!(node_rect(&frame, "flexible").width(), 30.0);
}

#[test]
fn row_layout_distributes_space_between_children() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let first = ui
        .sized_box()
        .id(&mut ui, "first")
        .width(&mut ui, UiLength::Px(20.0))
        .height(&mut ui, UiLength::Px(10.0));
    let second = ui
        .sized_box()
        .id(&mut ui, "second")
        .width(&mut ui, UiLength::Px(20.0))
        .height(&mut ui, UiLength::Px(10.0));
    let third = ui
        .sized_box()
        .id(&mut ui, "third")
        .width(&mut ui, UiLength::Px(20.0))
        .height(&mut ui, UiLength::Px(10.0));
    let root = ui
        .row()
        .id(&mut ui, "root")
        .justify_content(&mut ui, JustifyContent::SpaceBetween)
        .children(&mut ui, [first, second, third]);

    let frame = runtime.frame(&ui, root, viewport(200.0, 40.0), UiInput::default(), &theme);

    assert_eq!(node_rect(&frame, "first").min.x, 0.0);
    assert_eq!(node_rect(&frame, "second").min.x, 90.0);
    assert_eq!(node_rect(&frame, "third").min.x, 180.0);
}

#[test]
fn row_layout_includes_child_margins_in_spacing() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let first = ui
        .sized_box()
        .id(&mut ui, "first")
        .width(&mut ui, UiLength::Px(20.0))
        .height(&mut ui, UiLength::Px(10.0))
        .margin(
            &mut ui,
            EdgeInsets {
                top: 2.0,
                right: 10.0,
                bottom: 3.0,
                left: 5.0,
            },
        );
    let second = ui
        .sized_box()
        .id(&mut ui, "second")
        .width(&mut ui, UiLength::Px(20.0))
        .height(&mut ui, UiLength::Px(10.0))
        .margin(
            &mut ui,
            EdgeInsets {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: 7.0,
            },
        );
    let root = ui
        .row()
        .id(&mut ui, "root")
        .gap(&mut ui, 4.0)
        .child(&mut ui, first)
        .child(&mut ui, second);

    let frame = runtime.frame(&ui, root, viewport(100.0, 50.0), UiInput::default(), &theme);

    assert_eq!(
        node_rect(&frame, "first"),
        UiRect::from_min_size(UiPoint::new(5.0, 2.0), UiSize::new(20.0, 10.0))
    );
    assert_eq!(
        node_rect(&frame, "second"),
        UiRect::from_min_size(UiPoint::new(46.0, 0.0), UiSize::new(20.0, 10.0))
    );
}

#[test]
fn column_layout_includes_child_margins_in_spacing() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let first = ui
        .sized_box()
        .id(&mut ui, "first")
        .width(&mut ui, UiLength::Px(12.0))
        .height(&mut ui, UiLength::Px(20.0))
        .margin(
            &mut ui,
            EdgeInsets {
                top: 0.0,
                right: 0.0,
                bottom: 10.0,
                left: 4.0,
            },
        );
    let second = ui
        .sized_box()
        .id(&mut ui, "second")
        .width(&mut ui, UiLength::Px(12.0))
        .height(&mut ui, UiLength::Px(20.0))
        .margin(
            &mut ui,
            EdgeInsets {
                top: 5.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            },
        );
    let root = ui
        .column()
        .id(&mut ui, "root")
        .gap(&mut ui, 2.0)
        .child(&mut ui, first)
        .child(&mut ui, second);

    let frame = runtime.frame(&ui, root, viewport(50.0, 100.0), UiInput::default(), &theme);

    assert_eq!(
        node_rect(&frame, "first"),
        UiRect::from_min_size(UiPoint::new(4.0, 0.0), UiSize::new(12.0, 20.0))
    );
    assert_eq!(
        node_rect(&frame, "second"),
        UiRect::from_min_size(UiPoint::new(0.0, 37.0), UiSize::new(12.0, 20.0))
    );
}

#[test]
fn space_between_layout_includes_child_margins() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let first = ui
        .sized_box()
        .id(&mut ui, "first")
        .width(&mut ui, UiLength::Px(20.0))
        .height(&mut ui, UiLength::Px(10.0))
        .margin(&mut ui, EdgeInsets::symmetric(5.0, 0.0));
    let second = ui
        .sized_box()
        .id(&mut ui, "second")
        .width(&mut ui, UiLength::Px(20.0))
        .height(&mut ui, UiLength::Px(10.0))
        .margin(
            &mut ui,
            EdgeInsets {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: 10.0,
            },
        );
    let root = ui
        .row()
        .id(&mut ui, "root")
        .justify_content(&mut ui, JustifyContent::SpaceBetween)
        .child(&mut ui, first)
        .child(&mut ui, second);

    let frame = runtime.frame(&ui, root, viewport(200.0, 40.0), UiInput::default(), &theme);

    assert_eq!(node_rect(&frame, "first").min.x, 5.0);
    assert_eq!(node_rect(&frame, "second").min.x, 180.0);
}

#[test]
fn style_resolution_applies_tokens_class_responsive_and_state_overrides() {
    let mut styles = StyleRegistry::default();
    styles.insert(
        "hud.panel",
        StylePatch::default()
            .padding(EdgeInsets::all(8.0))
            .background(UiColor::rgba(10, 20, 30, 200))
            .radius(CornerRadius::all(4.0)),
    );
    let mut runtime = UiRuntime::with_styles(styles);
    let theme = Theme::default();
    let mut ui = UiArena::default();
    let root = ui
        .container()
        .id(&mut ui, "panel")
        .class(&mut ui, "hud.panel")
        .when(
            &mut ui,
            ScreenClass::Compact,
            StylePatch::default().padding(EdgeInsets::all(2.0)),
        )
        .hover_style(
            &mut ui,
            StylePatch::default()
                .background(theme.colors.hover)
                .radius(CornerRadius::left(6.0)),
        );

    let frame = runtime.frame(
        &ui,
        root,
        viewport(320.0, 200.0),
        pointer(1.0, 1.0, false),
        &theme,
    );
    let panel = frame.layout.find(&ElementId::from("panel")).unwrap();

    assert_eq!(panel.style.padding, EdgeInsets::all(2.0));
    assert_eq!(panel.style.background, theme.colors.hover);
    assert_eq!(panel.style.radius, CornerRadius::left(6.0));
}

#[test]
fn state_style_transitions_blend_in_and_out_over_time() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let base = UiColor::rgba(10, 20, 30, 255);
    let hover = UiColor::rgba(110, 120, 130, 255);
    let target_transform = UiTransform::tilt_degrees(0.0, 12.0);
    let root = ui
        .container()
        .id(&mut ui, "panel")
        .background(&mut ui, base)
        .transition_duration(&mut ui, 0.1)
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(50.0))
        .hover_style(
            &mut ui,
            StylePatch::default()
                .background(hover)
                .transform(target_transform),
        );

    let entering = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        pointer_dt(10.0, 10.0, false, 0.05),
        &theme,
    );
    let entering_style = &entering
        .layout
        .find(&ElementId::from("panel"))
        .unwrap()
        .style;
    assert!(entering_style.background.r > base.r);
    assert!(entering_style.background.r < hover.r);
    assert!(entering_style.transform.rotate_y > 0.0);
    assert!(entering_style.transform.rotate_y < target_transform.rotate_y);

    let entered = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        pointer_dt(10.0, 10.0, false, 0.05),
        &theme,
    );
    let entered_style = &entered
        .layout
        .find(&ElementId::from("panel"))
        .unwrap()
        .style;
    assert_eq!(entered_style.background, hover);
    assert!((entered_style.transform.rotate_y - target_transform.rotate_y).abs() < 0.001);

    let leaving = runtime.frame(&ui, root, viewport(200.0, 100.0), no_pointer(0.05), &theme);
    let leaving_style = &leaving
        .layout
        .find(&ElementId::from("panel"))
        .unwrap()
        .style;
    assert!(leaving_style.background.r > base.r);
    assert!(leaving_style.background.r < hover.r);
    assert!(leaving_style.transform.rotate_y > 0.0);
    assert!(leaving_style.transform.rotate_y < target_transform.rotate_y);
}

#[test]
fn hit_testing_skips_pass_through_nodes_and_clicks_underlying_element() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let under = ui
        .button("under")
        .id(&mut ui, "under")
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(40.0));
    let overlay = ui
        .container()
        .id(&mut ui, "overlay")
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(40.0))
        .pointer_events(&mut ui, PointerEvents::None);
    let root = ui
        .stack()
        .id(&mut ui, "root")
        .child(&mut ui, under)
        .child(&mut ui, overlay);

    runtime.frame(
        &ui,
        root,
        viewport(120.0, 60.0),
        pointer(20.0, 20.0, true),
        &theme,
    );
    let frame = runtime.frame(
        &ui,
        root,
        viewport(120.0, 60.0),
        pointer(20.0, 20.0, false),
        &theme,
    );

    assert!(frame.clicked("under"));
    assert!(!frame.clicked("overlay"));
    assert!(frame.hovered("under"));
    assert!(frame.hovered("overlay"));
}

#[test]
fn hover_passes_through_overlapping_elements_while_click_targets_frontmost() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let back_hover = UiColor::rgba(20, 80, 160, 255);
    let front_hover = UiColor::rgba(200, 80, 20, 255);
    let back = ui
        .button("Back")
        .id(&mut ui, "back")
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(40.0))
        .hover_style(&mut ui, StylePatch::default().background(back_hover));
    let front = ui
        .button("Front")
        .id(&mut ui, "front")
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(40.0))
        .hover_style(&mut ui, StylePatch::default().background(front_hover));
    let root = ui
        .stack()
        .id(&mut ui, "root")
        .child(&mut ui, back)
        .child(&mut ui, front);

    let hover = runtime.frame(
        &ui,
        root,
        viewport(120.0, 60.0),
        pointer(20.0, 20.0, false),
        &theme,
    );

    assert!(hover.hovered("back"));
    assert!(hover.hovered("front"));
    assert_eq!(
        hover
            .layout
            .find(&ElementId::from("back"))
            .unwrap()
            .style
            .background,
        back_hover
    );
    assert_eq!(
        hover
            .layout
            .find(&ElementId::from("front"))
            .unwrap()
            .style
            .background,
        front_hover
    );

    runtime.frame(
        &ui,
        root,
        viewport(120.0, 60.0),
        pointer(20.0, 20.0, true),
        &theme,
    );
    let click = runtime.frame(
        &ui,
        root,
        viewport(120.0, 60.0),
        pointer(20.0, 20.0, false),
        &theme,
    );

    assert!(click.clicked("front"));
    assert!(!click.clicked("back"));
}

#[test]
fn passive_overlay_with_passive_children_does_not_consume_pointer() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let label = ui
        .text("FPS")
        .id(&mut ui, "label")
        .pointer_events(&mut ui, PointerEvents::None);
    let overlay = ui
        .container()
        .id(&mut ui, "overlay")
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(40.0))
        .pointer_events(&mut ui, PointerEvents::None)
        .child(&mut ui, label);
    let root = ui
        .stack()
        .id(&mut ui, "root")
        .pointer_events(&mut ui, PointerEvents::None)
        .child(&mut ui, overlay);

    let frame = runtime.frame(
        &ui,
        root,
        viewport(120.0, 60.0),
        pointer(20.0, 20.0, false),
        &theme,
    );

    assert!(!frame.consumed_pointer);
    assert!(frame.hovered("overlay"));
    assert!(frame.hovered("label"));
}

#[test]
fn clipped_child_does_not_receive_pointer_outside_clip_rect() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let child = ui
        .button("child")
        .id(&mut ui, "child")
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(40.0));
    let root = ui
        .container()
        .id(&mut ui, "root")
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(40.0))
        .padding(
            &mut ui,
            EdgeInsets {
                top: 0.0,
                right: 0.0,
                bottom: 20.0,
                left: 0.0,
            },
        )
        .style(&mut ui, StylePatch::default().clip(true))
        .pointer_events(&mut ui, PointerEvents::None)
        .child(&mut ui, child);

    runtime.frame(
        &ui,
        root,
        viewport(120.0, 60.0),
        pointer(10.0, 30.0, true),
        &theme,
    );
    let frame = runtime.frame(
        &ui,
        root,
        viewport(120.0, 60.0),
        pointer(10.0, 30.0, false),
        &theme,
    );

    assert!(!frame.clicked("child"));
    assert!(!frame.hovered("child"));
    assert!(!frame.consumed_pointer);
}

#[test]
fn event_dispatch_uses_capture_target_bubble_and_can_stop_propagation() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let child = ui
        .button("child")
        .id(&mut ui, "child")
        .width(&mut ui, UiLength::Px(80.0))
        .height(&mut ui, UiLength::Px(24.0));
    let parent = ui
        .container()
        .id(&mut ui, "parent")
        .stop_propagation_on(&mut ui, PointerEventKind::PointerDown)
        .child(&mut ui, child);
    let root = ui.container().id(&mut ui, "root").child(&mut ui, parent);

    let frame = runtime.frame(
        &ui,
        root,
        viewport(120.0, 60.0),
        pointer(10.0, 10.0, true),
        &theme,
    );
    let down_events: Vec<_> = frame
        .events
        .iter()
        .filter(|event| event.kind == PointerEventKind::PointerDown)
        .map(|event| (event.element_id.as_str().to_owned(), event.phase))
        .collect();

    assert_eq!(
        down_events,
        vec![
            ("root".to_owned(), EventPhase::Capture),
            ("parent".to_owned(), EventPhase::Capture),
        ]
    );
}

#[test]
fn hover_pressed_click_and_drag_state_survives_across_frames_by_stable_id() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let root = ui
        .button("Launch")
        .id(&mut ui, "launch")
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(32.0))
        .hover_style(
            &mut ui,
            StylePatch::default().background(UiColor::rgba(1, 2, 3, 255)),
        )
        .pressed_style(
            &mut ui,
            StylePatch::default().background(UiColor::rgba(4, 5, 6, 255)),
        );

    let hover = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        pointer(10.0, 10.0, false),
        &theme,
    );
    assert!(hover.hovered("launch"));
    assert_eq!(
        hover
            .layout
            .find(&ElementId::from("launch"))
            .unwrap()
            .style
            .background,
        UiColor::rgba(1, 2, 3, 255)
    );

    let pressed = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        pointer(10.0, 10.0, true),
        &theme,
    );
    assert!(pressed.response("launch").pressed);
    assert_eq!(
        pressed
            .layout
            .find(&ElementId::from("launch"))
            .unwrap()
            .style
            .background,
        UiColor::rgba(4, 5, 6, 255)
    );

    let dragged = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        pointer(20.0, 10.0, true),
        &theme,
    );
    assert!(dragged.response("launch").dragged);

    let clicked = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        pointer(20.0, 10.0, false),
        &theme,
    );
    assert!(clicked.clicked("launch"));
}

#[test]
fn captured_drag_consumes_pointer_until_release_even_outside_target() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let root = ui
        .button("Drag")
        .id(&mut ui, "drag")
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(32.0));

    let pressed = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        pointer(10.0, 10.0, true),
        &theme,
    );
    assert!(pressed.consumed_pointer);

    let dragged_outside = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        pointer(150.0, 10.0, true),
        &theme,
    );
    assert!(dragged_outside.response("drag").dragged);
    assert!(dragged_outside.consumed_pointer);

    let released_outside = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        pointer(150.0, 10.0, false),
        &theme,
    );
    assert!(!released_outside.clicked("drag"));
    assert!(released_outside.consumed_pointer);

    let next_frame = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        pointer(150.0, 10.0, false),
        &theme,
    );
    assert!(!next_frame.consumed_pointer);
}

#[test]
fn progress_bar_clamps_value_and_generates_fill_command() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let root = ui
        .progress_bar(2.0, UiColor::rgba(10, 200, 20, 255))
        .id(&mut ui, "health")
        .width(&mut ui, UiLength::Px(100.0))
        .height(&mut ui, UiLength::Px(10.0))
        .padding(&mut ui, EdgeInsets::ZERO)
        .radius(&mut ui, CornerRadius::all(3.0));

    let frame = runtime.frame(
        &ui,
        root,
        viewport(200.0, 100.0),
        UiInput::default(),
        &theme,
    );

    assert!(frame.paint_commands.iter().any(|command| {
        matches!(
            command,
            PaintCommand::FillRect { rect, color, radius }
                if rect.width() == 100.0
                    && *color == UiColor::rgba(10, 200, 20, 255)
                    && *radius == CornerRadius::all(3.0)
        )
    }));
}

#[test]
fn paint_commands_include_rounded_background_border_text_clip_and_transform() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let label = ui.text("HP").id(&mut ui, "label");
    let root = ui
        .container()
        .id(&mut ui, "panel")
        .width(&mut ui, UiLength::Px(80.0))
        .height(&mut ui, UiLength::Px(40.0))
        .padding(&mut ui, EdgeInsets::all(4.0))
        .background(&mut ui, UiColor::rgba(1, 2, 3, 255))
        .border(&mut ui, Border::solid(UiColor::rgba(9, 8, 7, 255), 2.0))
        .radius(&mut ui, CornerRadius::all(5.0))
        .style(
            &mut ui,
            StylePatch::default()
                .clip(true)
                .transform(UiTransform::tilt_degrees(4.0, -8.0)),
        )
        .child(&mut ui, label);

    let frame = runtime.frame(&ui, root, viewport(100.0, 60.0), UiInput::default(), &theme);

    assert!(matches!(
        frame.paint_commands.first(),
        Some(PaintCommand::PushClip(_))
    ));
    assert!(frame
        .paint_commands
        .iter()
        .any(|command| matches!(command, PaintCommand::PushTransform { .. })));
    assert!(frame.paint_commands.iter().any(|command| {
        matches!(
            command,
            PaintCommand::FillRect { radius, .. } if *radius == CornerRadius::all(5.0)
        )
    }));
    assert!(frame.paint_commands.iter().any(|command| {
        matches!(
            command,
            PaintCommand::StrokeRect { border, radius, .. }
                if border.width == 2.0 && *radius == CornerRadius::all(5.0)
        )
    }));
    assert!(frame
        .paint_commands
        .iter()
        .any(|command| matches!(command, PaintCommand::Text { text, .. } if text == "HP")));
    assert!(matches!(
        frame.paint_commands.last(),
        Some(PaintCommand::PopClip)
    ));
}

#[test]
fn backend_adapter_forwards_expected_operations() {
    let rect = viewport(50.0, 20.0);
    let transform = UiTransform::tilt_degrees(3.0, -5.0);
    let commands = vec![
        PaintCommand::PushClip(rect),
        PaintCommand::PushTransform { rect, transform },
        PaintCommand::FillRect {
            rect,
            color: UiColor::rgba(1, 2, 3, 255),
            radius: CornerRadius::top(4.0),
        },
        PaintCommand::Text {
            rect,
            text: "Ammo".to_owned(),
            color: UiColor::WHITE,
            font_size: 12.0,
        },
        PaintCommand::Image {
            rect,
            texture: "icon/ammo".to_owned(),
            tint: UiColor::WHITE,
            radius: CornerRadius::none(),
        },
        PaintCommand::PopTransform,
        PaintCommand::PopClip,
    ];
    let mut backend = RecordingBackend::<String>::default();

    render_commands(&mut backend, rect, 1.0, &commands, str::to_owned);

    assert_eq!(
        backend.ops.first(),
        Some(&BackendOp::BeginFrame {
            viewport: rect,
            scale_factor: 1.0
        })
    );
    assert!(backend.ops.contains(&BackendOp::PushClip(rect)));
    assert!(backend
        .ops
        .contains(&BackendOp::PushTransform { rect, transform }));
    assert!(backend.ops.iter().any(|op| {
        matches!(
            op,
            BackendOp::FillRect { radius, .. } if *radius == CornerRadius::top(4.0)
        )
    }));
    assert!(backend
        .ops
        .iter()
        .any(|op| matches!(op, BackendOp::Text { text, .. } if text == "Ammo")));
    assert!(backend
        .ops
        .iter()
        .any(|op| matches!(op, BackendOp::Image { texture, .. } if texture == "icon/ammo")));
    assert_eq!(backend.ops.last(), Some(&BackendOp::EndFrame));
}

#[test]
fn egui_clip_rects_are_intersected_for_nested_clips() {
    let parent = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
    let child = egui::Rect::from_min_max(egui::pos2(-10.0, 10.0), egui::pos2(50.0, 150.0));

    let clip = crate::ui::backend::intersect_clip_rect(parent, child);

    assert_eq!(
        clip,
        egui::Rect::from_min_max(egui::pos2(0.0, 10.0), egui::pos2(50.0, 100.0))
    );
}

#[test]
fn custom_widget_can_be_allocated_without_engine_enum_changes() {
    struct BadgeWidget;

    impl UiWidget for BadgeWidget {
        fn default_size(&self, _style: &Style, _max: UiSize) -> UiSize {
            UiSize::new(24.0, 12.0)
        }

        fn paint(&self, _node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
            commands.push(PaintCommand::Custom("badge".to_owned()));
        }
    }

    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let mut ui = UiArena::default();
    let root = ui.alloc_node(BadgeWidget).id(&mut ui, "badge");

    let frame = runtime.frame(&ui, root, viewport(100.0, 50.0), UiInput::default(), &theme);

    assert_eq!(node_rect(&frame, "badge").size(), UiSize::new(24.0, 12.0));
    assert!(frame
        .paint_commands
        .iter()
        .any(|command| matches!(command, PaintCommand::Custom(name) if name == "badge")));
}

#[test]
fn full_frame_loop_handles_responsive_layout_interaction_and_backend_output() {
    let mut runtime = UiRuntime::default();
    let theme = Theme::default();
    let mut ui = UiArena::default();
    let fire = ui
        .button("Fire")
        .id(&mut ui, "fire")
        .width(&mut ui, UiLength::Px(80.0))
        .height(&mut ui, UiLength::Px(32.0));
    let root = ui
        .column()
        .id(&mut ui, "hud")
        .padding(&mut ui, EdgeInsets::all(12.0))
        .when(
            &mut ui,
            ScreenClass::Compact,
            StylePatch::default().padding(EdgeInsets::all(4.0)),
        )
        .child(&mut ui, fire);

    runtime.frame(
        &ui,
        root,
        viewport(320.0, 200.0),
        pointer(10.0, 10.0, true),
        &theme,
    );
    let frame = runtime.frame(
        &ui,
        root,
        viewport(320.0, 200.0),
        pointer(10.0, 10.0, false),
        &theme,
    );
    let mut backend = RecordingBackend::<String>::default();
    render_commands(
        &mut backend,
        viewport(320.0, 200.0),
        1.0,
        &frame.paint_commands,
        str::to_owned,
    );

    assert!(frame.clicked("fire"));
    assert_eq!(node_rect(&frame, "fire").min, UiPoint::new(4.0, 4.0));
    assert!(backend
        .ops
        .iter()
        .any(|op| matches!(op, BackendOp::Text { text, .. } if text == "Fire")));
}

#[test]
fn theme_and_style_are_registered_for_reflection() {
    let mut registry = TypeRegistry {
        types: Default::default(),
    };
    Theme::register(&mut registry);
    Style::register(&mut registry);
    CornerRadius::register(&mut registry);
    UiTransform::register(&mut registry);

    assert!(matches!(
        registry.type_info::<Theme>(),
        Some(TypeInfo::Struct(_))
    ));
    assert!(matches!(
        registry.type_info::<Style>(),
        Some(TypeInfo::Struct(_))
    ));
    assert!(matches!(
        registry.type_info::<CornerRadius>(),
        Some(TypeInfo::Struct(_))
    ));
    assert!(matches!(
        registry.type_info::<UiTransform>(),
        Some(TypeInfo::Struct(_))
    ));
}
