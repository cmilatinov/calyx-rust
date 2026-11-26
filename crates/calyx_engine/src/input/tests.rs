use super::{ActionMap, Input, InputBinding, InputState, ModifierBinding};
use crate::assets::{Asset, LoadedAsset};
use crate::context::GameContext;
use crate::test_support::test_asset_context_with_assets;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Sense, Vec2};
use uuid::Uuid;

fn binds_nothing(state: &InputState) -> bool {
    state
        .action_map
        .with(|map| map.actions.is_empty() && map.axes.is_empty())
}

#[test]
fn default_action_map_binds_nothing() {
    let map = ActionMap::default();
    assert!(map.actions.is_empty());
    assert!(map.axes.is_empty());
    assert!(binds_nothing(&InputState::default()));
}

#[test]
fn project_without_an_action_map_binds_nothing() {
    let dir = std::env::temp_dir().join(format!("calyx-input-empty-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let game = GameContext::new(test_asset_context_with_assets(vec![dir.clone()]));
    assert!(binds_nothing(game.resources.input()));
    drop(game);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn action_bindings_are_configurable() {
    let mut map = ActionMap::new();
    map.bind_action("shoot", InputBinding::Key(Key::Space));
    map.bind_action("shoot", InputBinding::Key(Key::Enter));

    assert_eq!(map.actions["shoot"].len(), 2);
}

#[test]
fn axis_bindings_are_configurable() {
    let mut map = ActionMap::new();
    map.bind_axis(
        "move_forward",
        InputBinding::Key(Key::W),
        InputBinding::Key(Key::S),
    );

    let axis = &map.axes["move_forward"];
    assert_eq!(axis.positive, InputBinding::Key(Key::W));
    assert_eq!(axis.negative, InputBinding::Key(Key::S));
}

#[test]
fn key_combos_store_exact_modifier_requirements() {
    let binding = InputBinding::key_combo(Key::S, Modifiers::CTRL);

    assert_eq!(
        binding,
        InputBinding::ModifiedKey {
            key: Key::S,
            modifiers: ModifierBinding::exact(Modifiers::CTRL),
        }
    );
}

#[test]
fn exact_modifier_bindings_reject_extra_modifiers() {
    let binding = ModifierBinding::exact(Modifiers::SHIFT);
    let shift_ctrl = Modifiers {
        shift: true,
        ctrl: true,
        ..Default::default()
    };

    assert!(binding.matches(Modifiers::SHIFT));
    assert!(!binding.matches(shift_ctrl));
}

#[test]
fn requiring_modifier_bindings_allow_extra_modifiers() {
    let binding = ModifierBinding::requiring(Modifiers::SHIFT);
    let shift_ctrl = Modifiers {
        shift: true,
        ctrl: true,
        ..Default::default()
    };

    assert!(binding.matches(Modifiers::SHIFT));
    assert!(binding.matches(shift_ctrl));
}

#[test]
fn action_maps_load_from_asset_files() {
    let asset_path = std::env::temp_dir().join(format!("calyx-input-{}.cxinput", Uuid::new_v4()));
    let mut map = ActionMap::new();
    map.bind_action("save", InputBinding::key_combo(Key::S, Modifiers::CTRL));
    serde_json::to_writer_pretty(
        std::fs::File::create(&asset_path).expect("failed to create action map asset"),
        &map,
    )
    .expect("failed to write action map asset");

    let loaded =
        LoadedAsset::<ActionMap>::from_json_file(&asset_path).expect("action map should load");

    assert_eq!(ActionMap::file_extensions(), &["cxinput"]);
    assert_eq!(loaded.asset.actions["save"].len(), 1);
    std::fs::remove_file(asset_path).expect("failed to remove temp action map asset");
}

fn pass(events: Vec<Event>, modifiers: Modifiers) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0))),
        modifiers,
        events,
        ..Default::default()
    }
}

#[test]
fn pointer_state_is_read_only_while_input_is_active() {
    let ctx = egui::Context::default();
    let mut state = InputState::default();
    ctx.begin_pass(pass(
        vec![
            Event::PointerMoved(Pos2::new(20.0, 30.0)),
            Event::MouseMoved(Vec2::new(4.0, -2.0)),
        ],
        Modifiers::NONE,
    ));
    let active = Input::from_ctx(&ctx, None, &mut state, true);
    let inactive = Input::from_ctx(&ctx, None, &mut state, false);

    assert_eq!(active.pointer_pos(), Some(Pos2::new(20.0, 30.0)));
    assert_eq!(active.pointer_motion(), Vec2::new(4.0, -2.0));
    assert_eq!(active.surface_rect(), None);
    assert_eq!(active.drag_delta(PointerButton::Primary), None);
    assert_eq!(inactive.pointer_pos(), None);
    assert_eq!(inactive.pointer_motion(), Vec2::ZERO);
    assert_eq!(inactive.scroll_delta(), Vec2::ZERO);
    let _ = ctx.end_pass();
}

#[test]
fn drag_delta_reports_surface_drags_by_the_given_button() {
    let ctx = egui::Context::default();
    let mut state = InputState::default();
    let surface = Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0));
    let mut frame = |events: Vec<Event>| {
        ctx.begin_pass(pass(events, Modifiers::NONE));
        let mut drags = None;
        egui::CentralPanel::default().show(&ctx, |ui| {
            let res = ui.interact(surface, ui.id().with("surface"), Sense::drag());
            let input = Input::from_ctx(&ctx, Some(&res), &mut state, true);
            assert_eq!(input.surface_rect(), Some(surface));
            drags = Some((
                input.drag_delta(PointerButton::Secondary),
                input.drag_delta(PointerButton::Primary),
            ));
        });
        let _ = ctx.end_pass();
        drags.unwrap()
    };
    let button = |pos: Pos2, pressed: bool| Event::PointerButton {
        pos,
        button: PointerButton::Secondary,
        pressed,
        modifiers: Modifiers::NONE,
    };

    frame(vec![Event::PointerMoved(Pos2::new(50.0, 50.0))]);
    frame(vec![button(Pos2::new(50.0, 50.0), true)]);
    frame(vec![Event::PointerMoved(Pos2::new(70.0, 50.0))]);
    let (secondary, primary) = frame(vec![Event::PointerMoved(Pos2::new(80.0, 55.0))]);

    assert_eq!(secondary, Some(Vec2::new(10.0, 5.0)));
    assert_eq!(primary, None);
}

#[test]
fn modifier_bindings_hold_while_their_modifiers_are_held() {
    let mut map = ActionMap::new();
    map.bind_action("boost", InputBinding::Modifier(Modifiers::CTRL));
    map.bind_axis(
        "vertical",
        InputBinding::Key(Key::Space),
        InputBinding::Modifier(Modifiers::SHIFT),
    );
    let ctx = egui::Context::default();
    let mut state = InputState::new(map);
    let ctrl_shift = Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };

    ctx.begin_pass(pass(Vec::new(), ctrl_shift));
    let input = Input::from_ctx(&ctx, None, &mut state, true);
    assert!(input.action("boost").pressed());
    assert!(!input.action("boost").just_pressed());
    assert_eq!(input.axis("vertical"), -1.0);
    let _ = ctx.end_pass();

    ctx.begin_pass(pass(Vec::new(), Modifiers::NONE));
    let input = Input::from_ctx(&ctx, None, &mut state, true);
    assert!(!input.action("boost").pressed());
    assert_eq!(input.axis("vertical"), 0.0);
    let _ = ctx.end_pass();
}
