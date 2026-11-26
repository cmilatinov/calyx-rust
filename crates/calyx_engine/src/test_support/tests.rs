use super::{
    test_asset_context_with_assets, test_registries, HeadlessSceneRunner, SceneBuilder, TestAssets,
};
use crate as calyx_engine;
use crate::component::{
    Component, ComponentEventContext, ComponentRef, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use crate::context::GameContext;
use crate::input::{ActionMap, InputBinding, PROJECT_ACTION_MAP_NAME};
use crate::input::{Input, InputState};
use crate::reflect::{Reflect, ReflectDefault};
use crate::resource::{Resource, ResourceMap};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::{Key, PointerButton, Pos2, RawInput, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Default, Resource, TypeUuid)]
#[uuid = "c5fa8823-a151-4543-8559-5307172cd255"]
struct InputProbe {
    shoot_presses: usize,
    forward_axis: f32,
    cursor_delta: Vec2,
}

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "8336b78b-5e57-474a-95ce-904bd42b5135"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
struct TestInputComponent;

impl Component for TestInputComponent {}

impl ComponentUpdate for TestInputComponent {
    fn update(
        _this: ComponentRef<Self>,
        _ctx: ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        // A scene without the probe resource is another test's scene.
        let Some(probe) = resources.resource_mut::<InputProbe>() else {
            return;
        };
        if input.action("shoot").just_pressed() {
            probe.shoot_presses += 1;
        }
        probe.forward_axis = input.axis("move_forward");
        probe.cursor_delta = input.cursor_delta();
    }
}

#[test]
fn headless_runner_applies_queued_input_events() {
    let mut builder = SceneBuilder::new(test_registries());
    let game_object = builder.spawn("input-probe");
    builder.add_component(game_object, TestInputComponent);

    let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
    runner.resources_mut().insert(InputProbe::default());
    let mut map = ActionMap::new();
    map.bind_action("shoot", InputBinding::Key(Key::Space));
    map.bind_axis(
        "move_forward",
        InputBinding::Key(Key::W),
        InputBinding::Key(Key::S),
    );
    runner.resources_mut().input_mut().set_action_map(map);

    runner.press_key(Key::W);
    runner.press_key(Key::Space);
    runner.step();

    let probe = runner.resources().resource::<InputProbe>().unwrap();
    assert_eq!(probe.shoot_presses, 1);
    assert_eq!(probe.forward_axis, 1.0);

    runner.release_key(Key::Space);
    runner.release_key(Key::W);
    runner.step();

    let probe = runner.resources().resource::<InputProbe>().unwrap();
    assert_eq!(probe.forward_axis, 0.0);
}

/// Lists every file under `root` with its size and modification time.
fn snapshot(root: &std::path::Path) -> Vec<(std::path::PathBuf, u64, std::time::SystemTime)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                files.push((entry.path(), metadata.len(), metadata.modified().unwrap()));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn loading_test_assets_does_not_modify_source_asset_directory() {
    let source =
        dunce::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"))
            .unwrap();
    let before = snapshot(&source);

    let assets = TestAssets::engine();
    let registries = assets.registries();
    assert!(registries.assets.read().asset_id("meshes/cube").is_some());
    drop(registries);

    assert_eq!(before, snapshot(&source));
    let copy = assets.engine_root().to_path_buf();
    assert!(copy.starts_with(std::env::temp_dir()));
    drop(assets);
    assert!(!copy.exists(), "temporary copy should be removed on drop");
}

fn input_probe_runner() -> HeadlessSceneRunner {
    let mut builder = SceneBuilder::new(test_registries());
    let game_object = builder.spawn("input-probe");
    builder.add_component(game_object, TestInputComponent);
    let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
    runner.resources_mut().insert(InputProbe::default());
    runner
}

fn shoot_presses(runner: &HeadlessSceneRunner) -> usize {
    runner
        .resources()
        .resource::<InputProbe>()
        .unwrap()
        .shoot_presses
}

#[test]
fn rebound_action_fires_on_the_new_binding() {
    let mut runner = input_probe_runner();
    let mut map = ActionMap::new();
    map.bind_action("shoot", InputBinding::Key(Key::Enter));
    runner.resources_mut().input_mut().set_action_map(map);

    runner.press_key(Key::Space);
    runner.step();
    assert_eq!(shoot_presses(&runner), 0);

    runner.release_key(Key::Space);
    runner.press_key(Key::Enter);
    runner.step();
    assert_eq!(shoot_presses(&runner), 1);
}

#[test]
fn project_action_map_asset_drives_the_scene() {
    let dir = std::env::temp_dir().join(format!("calyx-input-asset-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut map = ActionMap::new();
    map.bind_action(
        "shoot",
        InputBinding::PointerButton(PointerButton::Secondary),
    );
    let asset_path = dir.join(format!("{PROJECT_ACTION_MAP_NAME}.cxinput"));
    serde_json::to_writer_pretty(std::fs::File::create(&asset_path).unwrap(), &map).unwrap();

    let assets = test_asset_context_with_assets(vec![dir.clone()]);
    let game = GameContext::new(assets.clone());
    let mut builder = SceneBuilder::new(assets.registries.lock_read());
    let game_object = builder.spawn("input-probe");
    builder.add_component(game_object, TestInputComponent);
    let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
    runner.resources_mut().insert(InputProbe::default());
    runner
        .resources_mut()
        .insert(game.resources.input().clone());

    runner.press_key(Key::Space);
    runner.step();
    assert_eq!(shoot_presses(&runner), 0);

    runner.release_key(Key::Space);
    runner.press_pointer_button(Pos2::new(10.0, 10.0), PointerButton::Secondary);
    runner.step();
    assert_eq!(shoot_presses(&runner), 1);

    // A hot reload updates the asset handle in place; the running input follows it.
    let mut reloaded = ActionMap::new();
    reloaded.bind_action("shoot", InputBinding::Key(Key::Enter));
    let handle = assets
        .registries
        .assets
        .read()
        .load::<ActionMap>(PROJECT_ACTION_MAP_NAME)
        .unwrap();
    *handle.write() = reloaded;
    runner.release_pointer_button(Pos2::new(10.0, 10.0), PointerButton::Secondary);
    runner.press_pointer_button(Pos2::new(10.0, 10.0), PointerButton::Secondary);
    runner.step();
    assert_eq!(shoot_presses(&runner), 1);
    runner.press_key(Key::Enter);
    runner.step();
    assert_eq!(shoot_presses(&runner), 2);

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cursor_delta_reports_movement_since_the_previous_frame() {
    let mut runner = input_probe_runner();
    let delta = |runner: &HeadlessSceneRunner| {
        runner
            .resources()
            .resource::<InputProbe>()
            .unwrap()
            .cursor_delta
    };

    runner.move_pointer(Pos2::new(10.0, 10.0));
    runner.step();
    assert_eq!(delta(&runner), Vec2::ZERO);

    runner.move_pointer(Pos2::new(25.0, 12.0));
    runner.step();
    assert_eq!(delta(&runner), Vec2::new(15.0, 2.0));

    runner.step();
    assert_eq!(delta(&runner), Vec2::ZERO);

    runner.move_pointer(Pos2::new(35.0, 12.0));
    runner.step();
    assert_eq!(delta(&runner), Vec2::new(10.0, 0.0));
}

#[test]
fn input_surfaces_in_one_pass_share_the_cursor_delta() {
    let ctx = egui::Context::default();
    let mut state = InputState::default();
    let pass = |position: Pos2| RawInput {
        events: vec![egui::Event::PointerMoved(position)],
        ..Default::default()
    };

    ctx.begin_pass(pass(Pos2::new(10.0, 10.0)));
    Input::from_ctx(&ctx, None, &mut state, true);
    let _ = ctx.end_pass();

    ctx.begin_pass(pass(Pos2::new(25.0, 12.0)));
    let first = Input::from_ctx(&ctx, None, &mut state, true);
    let second = Input::from_ctx(&ctx, None, &mut state, true);
    let inactive = Input::from_ctx(&ctx, None, &mut state, false);
    let _ = ctx.end_pass();

    assert_eq!(first.cursor_delta(), Vec2::new(15.0, 2.0));
    assert_eq!(second.cursor_delta(), Vec2::new(15.0, 2.0));
    assert_eq!(inactive.cursor_delta(), Vec2::ZERO);
}

#[test]
fn malformed_project_action_map_is_reported() {
    let dir = std::env::temp_dir().join(format!("calyx-input-bad-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{PROJECT_ACTION_MAP_NAME}.cxinput")),
        "not json",
    )
    .unwrap();
    let assets = test_asset_context_with_assets(vec![dir.clone()]);

    let mut state = InputState::default();
    let result = state.load_project_action_map(&assets.registries.assets.read());
    assert!(result.is_err());

    let empty = std::env::temp_dir().join(format!("calyx-input-none-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&empty).unwrap();
    let assets = test_asset_context_with_assets(vec![empty.clone()]);
    let result = state.load_project_action_map(&assets.registries.assets.read());
    assert!(result.is_ok());

    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(empty).unwrap();
}
