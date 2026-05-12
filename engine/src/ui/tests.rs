use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{ReflectedType, TypeInfo};

use super::*;

fn viewport(width: f32, height: f32) -> UiRect {
    UiRect::from_min_size(UiPoint::ZERO, UiSize::new(width, height))
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
    let root = row()
        .id("root")
        .padding(EdgeInsets::all(10.0))
        .gap(5.0)
        .child(
            sized_box()
                .id("fixed")
                .width(UiLength::Px(50.0))
                .height(UiLength::Px(20.0)),
        )
        .child(
            sized_box()
                .id("flex")
                .width(UiLength::Fill)
                .height(UiLength::Px(20.0))
                .flex_grow(1.0),
        );

    let frame = runtime.frame(&root, viewport(200.0, 60.0), UiInput::default(), &theme);

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
    runtime.state.hovered = Some(ElementId::from("panel"));
    let theme = Theme::default();
    let node = container()
        .id("panel")
        .class("hud.panel")
        .when(
            ScreenClass::Compact,
            StylePatch::default().padding(EdgeInsets::all(2.0)),
        )
        .hover_style(
            StylePatch::default()
                .background(theme.colors.hover)
                .radius(CornerRadius::left(6.0)),
        );

    let frame = runtime.frame(
        &node,
        viewport(320.0, 200.0),
        UiInput {
            pointer_position: Some(UiPoint::new(1.0, 1.0)),
            pointer_down: false,
        },
        &theme,
    );
    let panel = frame.layout.find(&ElementId::from("panel")).unwrap();

    assert_eq!(panel.style.padding, EdgeInsets::all(2.0));
    assert_eq!(panel.style.background, theme.colors.hover);
    assert_eq!(panel.style.radius, CornerRadius::left(6.0));
}

#[test]
fn hit_testing_skips_pass_through_nodes_and_clicks_underlying_element() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let root = stack()
        .id("root")
        .child(
            button("under")
                .id("under")
                .width(UiLength::Px(100.0))
                .height(UiLength::Px(40.0)),
        )
        .child(
            container()
                .id("overlay")
                .width(UiLength::Px(100.0))
                .height(UiLength::Px(40.0))
                .pointer_events(PointerEvents::None),
        );

    runtime.frame(
        &root,
        viewport(120.0, 60.0),
        UiInput {
            pointer_position: Some(UiPoint::new(20.0, 20.0)),
            pointer_down: true,
        },
        &theme,
    );
    let frame = runtime.frame(
        &root,
        viewport(120.0, 60.0),
        UiInput {
            pointer_position: Some(UiPoint::new(20.0, 20.0)),
            pointer_down: false,
        },
        &theme,
    );

    assert!(frame.clicked("under"));
    assert!(!frame.clicked("overlay"));
}

#[test]
fn event_dispatch_uses_capture_target_bubble_and_can_stop_propagation() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let root = container().id("root").child(
        container()
            .id("parent")
            .stop_propagation_on(PointerEventKind::PointerDown)
            .child(
                button("child")
                    .id("child")
                    .width(UiLength::Px(80.0))
                    .height(UiLength::Px(24.0)),
            ),
    );

    let frame = runtime.frame(
        &root,
        viewport(120.0, 60.0),
        UiInput {
            pointer_position: Some(UiPoint::new(10.0, 10.0)),
            pointer_down: true,
        },
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
    let root = button("Launch")
        .id("launch")
        .width(UiLength::Px(100.0))
        .height(UiLength::Px(32.0))
        .hover_style(StylePatch::default().background(UiColor::rgba(1, 2, 3, 255)))
        .pressed_style(StylePatch::default().background(UiColor::rgba(4, 5, 6, 255)));

    let hover = runtime.frame(
        &root,
        viewport(200.0, 100.0),
        UiInput {
            pointer_position: Some(UiPoint::new(10.0, 10.0)),
            pointer_down: false,
        },
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
        &root,
        viewport(200.0, 100.0),
        UiInput {
            pointer_position: Some(UiPoint::new(10.0, 10.0)),
            pointer_down: true,
        },
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
        &root,
        viewport(200.0, 100.0),
        UiInput {
            pointer_position: Some(UiPoint::new(20.0, 10.0)),
            pointer_down: true,
        },
        &theme,
    );
    assert!(dragged.response("launch").dragged);

    let clicked = runtime.frame(
        &root,
        viewport(200.0, 100.0),
        UiInput {
            pointer_position: Some(UiPoint::new(20.0, 10.0)),
            pointer_down: false,
        },
        &theme,
    );
    assert!(clicked.clicked("launch"));
}

#[test]
fn progress_bar_clamps_value_and_generates_fill_command() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let root = progress_bar(2.0, UiColor::rgba(10, 200, 20, 255))
        .id("health")
        .width(UiLength::Px(100.0))
        .height(UiLength::Px(10.0))
        .padding(EdgeInsets::ZERO)
        .radius(CornerRadius::all(3.0));

    let frame = runtime.frame(&root, viewport(200.0, 100.0), UiInput::default(), &theme);

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
fn paint_commands_include_rounded_background_border_text_and_clip() {
    let theme = Theme::default();
    let mut runtime = UiRuntime::default();
    let root = container()
        .id("panel")
        .width(UiLength::Px(80.0))
        .height(UiLength::Px(40.0))
        .padding(EdgeInsets::all(4.0))
        .background(UiColor::rgba(1, 2, 3, 255))
        .border(Border::solid(UiColor::rgba(9, 8, 7, 255), 2.0))
        .radius(CornerRadius::all(5.0))
        .style(StylePatch::default().clip(true))
        .child(text("HP").id("label"));

    let frame = runtime.frame(&root, viewport(100.0, 60.0), UiInput::default(), &theme);

    assert!(matches!(
        frame.paint_commands.first(),
        Some(PaintCommand::PushClip(_))
    ));
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
        .any(|command| { matches!(command, PaintCommand::Text { text, .. } if text == "HP") }));
    assert!(matches!(
        frame.paint_commands.last(),
        Some(PaintCommand::PopClip)
    ));
}

#[test]
fn backend_adapter_forwards_expected_operations() {
    let rect = viewport(50.0, 20.0);
    let commands = vec![
        PaintCommand::PushClip(rect),
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
        PaintCommand::PopClip,
    ];
    let mut backend = RecordingBackend::<String>::default();

    render_commands(&mut backend, rect, 1.0, &commands, |texture| {
        texture.to_owned()
    });

    assert_eq!(
        backend.ops.first(),
        Some(&BackendOp::BeginFrame {
            viewport: rect,
            scale_factor: 1.0
        })
    );
    assert!(backend.ops.contains(&BackendOp::PushClip(rect)));
    assert!(backend.ops.iter().any(|op| {
        matches!(
            op,
            BackendOp::FillRect { radius, .. } if *radius == CornerRadius::top(4.0)
        )
    }));
    assert!(backend
        .ops
        .iter()
        .any(|op| { matches!(op, BackendOp::Text { text, .. } if text == "Ammo") }));
    assert!(backend
        .ops
        .iter()
        .any(|op| { matches!(op, BackendOp::Image { texture, .. } if texture == "icon/ammo") }));
    assert_eq!(backend.ops.last(), Some(&BackendOp::EndFrame));
}

#[test]
fn full_frame_loop_handles_responsive_layout_interaction_and_backend_output() {
    let mut runtime = UiRuntime::default();
    let theme = Theme::default();
    let root = column()
        .id("hud")
        .padding(EdgeInsets::all(12.0))
        .when(
            ScreenClass::Compact,
            StylePatch::default().padding(EdgeInsets::all(4.0)),
        )
        .child(
            button("Fire")
                .id("fire")
                .width(UiLength::Px(80.0))
                .height(UiLength::Px(32.0)),
        );

    runtime.frame(
        &root,
        viewport(320.0, 200.0),
        UiInput {
            pointer_position: Some(UiPoint::new(10.0, 10.0)),
            pointer_down: true,
        },
        &theme,
    );
    let frame = runtime.frame(
        &root,
        viewport(320.0, 200.0),
        UiInput {
            pointer_position: Some(UiPoint::new(10.0, 10.0)),
            pointer_down: false,
        },
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
        .any(|op| { matches!(op, BackendOp::Text { text, .. } if text == "Fire") }));
}

#[test]
fn theme_and_style_are_registered_for_reflection() {
    let mut registry = TypeRegistry {
        types: Default::default(),
    };
    Theme::register(&mut registry);
    Style::register(&mut registry);
    CornerRadius::register(&mut registry);

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
}
