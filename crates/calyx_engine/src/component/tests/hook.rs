//! Lifecycle hooks receive their own component, not the registry's prototype.

use serde::{Deserialize, Serialize};

use crate as calyx_engine;
use crate::component::{
    Component, ComponentEventContext, ComponentHud, ComponentRef, ComponentStart, ComponentUpdate,
    ReflectComponent, ReflectComponentHud, ReflectComponentStart, ReflectComponentUpdate,
};
use crate::input::Input;
use crate::reflect::{Reflect, ReflectDefault};
use crate::resource::{Resource, ResourceMap};
use crate::scene::{GameObject, Scene, SceneHud};
use crate::test_support::{test_registries, HeadlessSceneRunner};
use crate::ui::{PaintCommand, UiArena, UiInput, UiNodeHandle, UiPoint, UiRect, UiSize};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};

/// Records what its hooks read of its own `speed`, which a test sets to a
/// value other than the default.
#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "46fdc823-8f9d-4ede-9f4b-b640b5efde07"]
#[reflect(
    Default,
    TypeUuidDynamic,
    Component,
    ComponentStart,
    ComponentUpdate,
    ComponentHud
)]
struct SpeedProbe {
    speed: f32,
    speed_at_start: f32,
    speed_in_update: f32,
}

impl Component for SpeedProbe {}

impl ComponentStart for SpeedProbe {
    fn start(this: ComponentRef<Self>, ctx: ComponentEventContext) {
        let speed = this.read(ctx.scene, |probe| probe.speed).unwrap();
        this.write(ctx.scene, |probe| probe.speed_at_start = speed);
    }
}

impl ComponentUpdate for SpeedProbe {
    fn update(
        this: ComponentRef<Self>,
        ctx: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        if let Some(calls) = resources.resource_mut::<UpdateCalls>() {
            calls.0 += 1;
        }
        let speed = this.read(ctx.scene, |probe| probe.speed).unwrap();
        this.write(ctx.scene, |probe| probe.speed_in_update = speed);
    }
}

impl ComponentHud for SpeedProbe {
    fn build_hud(
        this: &Self,
        _scene: &Scene,
        _game_object: GameObject,
        ui: &mut UiArena,
        _viewport: UiRect,
    ) -> Option<UiNodeHandle> {
        Some(ui.text(format!("speed {}", this.speed)))
    }
}

/// Counts the update hook calls of every [`SpeedProbe`] in a frame.
#[derive(Default, Resource, TypeUuid)]
#[uuid = "09d74672-5cc6-42f2-9e1b-cea9c8ea78e3"]
struct UpdateCalls(u32);

/// What a [`SelfEditor`]'s update hook does to its own component.
const APPEND_IN_ORDER: u32 = 0;
const REMOVE_ITSELF: u32 = 1;
const REPLACE_ITSELF: u32 = 2;
const IDLE: u32 = 3;

/// Writes its own component through its handle and through `ctx.scene` in
/// one update, or removes or replaces itself.
#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "168d9532-328f-4042-a5e6-4124423ac87c"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
struct SelfEditor {
    action: u32,
    trail: String,
}

impl Component for SelfEditor {}

impl ComponentUpdate for SelfEditor {
    fn update(
        this: ComponentRef<Self>,
        ctx: ComponentEventContext,
        _resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let game_object = ctx.game_object;
        match this.read(ctx.scene, |editor| editor.action) {
            Some(APPEND_IN_ORDER) => {
                this.write(ctx.scene, |editor| editor.trail.push_str("handle;"));
                ctx.scene
                    .write_component::<SelfEditor, _>(game_object, |editor| {
                        editor.trail.push_str("scene;")
                    });
                this.write(ctx.scene, |editor| editor.trail.push_str("handle;"));
            }
            Some(REMOVE_ITSELF) => {
                ctx.scene.remove_component::<SelfEditor>(game_object);
                assert!(
                    this.write(ctx.scene, |editor| editor.action = IDLE)
                        .is_none(),
                    "the handle of a removed component writes nothing"
                );
            }
            Some(REPLACE_ITSELF) => {
                ctx.scene.add_component(
                    game_object,
                    SelfEditor {
                        action: IDLE,
                        trail: "replacement".into(),
                    },
                );
            }
            _ => {}
        }
    }
}

fn trail(scene: &Scene, game_object: GameObject) -> Option<String> {
    scene.read_component::<SelfEditor, _, _>(game_object, |editor| editor.trail.clone())
}

fn self_editor_runner(action: u32) -> (HeadlessSceneRunner, GameObject) {
    let mut scene = test_registries().scene();
    let game_object = scene.create(None, None);
    scene.add_component(
        game_object,
        SelfEditor {
            action,
            trail: String::new(),
        },
    );
    (HeadlessSceneRunner::from_scene(scene), game_object)
}

#[test]
fn start_and_update_read_the_objects_own_component() {
    let mut scene = test_registries().scene();
    let game_object = scene.create(None, None);
    scene.add_component(
        game_object,
        SpeedProbe {
            speed: 3.5,
            ..Default::default()
        },
    );

    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.step();

    let seen = runner
        .scene()
        .read_component::<SpeedProbe, _, _>(game_object, |probe| {
            (probe.speed_at_start, probe.speed_in_update)
        })
        .unwrap();
    assert_eq!(seen, (3.5, 3.5));
}

#[test]
fn the_hud_hook_reads_the_objects_own_component() {
    let mut scene = test_registries().scene();
    let game_object = scene.create(None, None);
    scene.add_component(
        game_object,
        SpeedProbe {
            speed: 3.5,
            ..Default::default()
        },
    );

    let viewport = UiRect::from_min_size(UiPoint::new(0.0, 0.0), UiSize::new(320.0, 240.0));
    let frame = SceneHud::default()
        .frame(&scene, viewport, UiInput::default())
        .unwrap();
    let text: Vec<&str> = frame
        .paint_commands
        .iter()
        .filter_map(|command| match command {
            PaintCommand::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, ["speed 3.5"]);
}

#[test]
fn hooks_run_only_for_objects_holding_their_component() {
    let mut scene = test_registries().scene();
    let holder = scene.create(None, None);
    scene.create(None, None);
    scene.create(None, None);
    scene.add_component(holder, SpeedProbe::default());

    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.resources_mut().insert(UpdateCalls::default());
    runner.step();

    assert_eq!(runner.resources().resource::<UpdateCalls>().unwrap().0, 1);
}

#[test]
fn handle_and_scene_writes_to_the_own_component_apply_in_call_order() {
    let (mut runner, game_object) = self_editor_runner(APPEND_IN_ORDER);
    runner.step();
    assert_eq!(
        trail(runner.scene(), game_object).as_deref(),
        Some("handle;scene;handle;")
    );
}

#[test]
fn a_hook_that_removes_its_own_component_leaves_it_removed() {
    let (mut runner, game_object) = self_editor_runner(REMOVE_ITSELF);
    runner.step();
    assert_eq!(trail(runner.scene(), game_object), None);
}

#[test]
fn a_hook_that_replaces_its_own_component_keeps_the_replacement() {
    let (mut runner, game_object) = self_editor_runner(REPLACE_ITSELF);
    runner.step();
    assert_eq!(
        trail(runner.scene(), game_object).as_deref(),
        Some("replacement")
    );
}
