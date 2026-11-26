use super::SceneHud;
use crate as calyx_engine;
use crate::component::{Component, ComponentHud, ReflectComponent, ReflectComponentHud};
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::{GameObject, Scene};
use crate::test_support::SceneBuilder;
use crate::ui::{PaintCommand, UiArena, UiInput, UiNodeHandle, UiPoint, UiRect, UiSize};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Default, Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "4f0e8d55-6a7c-4b0e-9d61-2c7f6f1f3a10"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentHud)]
struct TestHudCounter {
    value: u32,
    hidden: bool,
}

impl Component for TestHudCounter {}

impl ComponentHud for TestHudCounter {
    fn build_hud(
        counter: &Self,
        _scene: &Scene,
        _game_object: GameObject,
        ui: &mut UiArena,
        _viewport: UiRect,
    ) -> Option<UiNodeHandle> {
        (!counter.hidden).then(|| ui.text(format!("count {}", counter.value)))
    }
}

fn viewport() -> UiRect {
    UiRect::from_min_size(UiPoint::new(10.0, 20.0), UiSize::new(640.0, 360.0))
}

fn painted_text(scene: &Scene) -> Option<Vec<String>> {
    let frame = SceneHud::default().frame(scene, viewport(), UiInput::default())?;
    let mut text: Vec<String> = frame
        .paint_commands
        .iter()
        .filter_map(|command| match command {
            PaintCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    text.sort();
    Some(text)
}

#[test]
fn scene_hud_paints_every_hud_component_from_its_live_value() {
    let mut builder = SceneBuilder::new(crate::test_support::test_registries());
    let first = builder.spawn("first");
    let second = builder.spawn("second");
    let hidden = builder.spawn("hidden");
    builder.add_component(
        first,
        TestHudCounter {
            value: 3,
            hidden: false,
        },
    );
    builder.add_component(
        second,
        TestHudCounter {
            value: 7,
            hidden: false,
        },
    );
    builder.add_component(
        hidden,
        TestHudCounter {
            value: 9,
            hidden: true,
        },
    );
    let mut scene = builder.finish();

    assert_eq!(
        painted_text(&scene),
        Some(vec!["count 3".to_string(), "count 7".to_string()])
    );

    let _ = scene.write_component::<TestHudCounter, _>(first, |counter| counter.value = 4);
    assert_eq!(
        painted_text(&scene),
        Some(vec!["count 4".to_string(), "count 7".to_string()])
    );
}

#[test]
fn scene_without_hud_components_paints_nothing() {
    let mut builder = SceneBuilder::new(crate::test_support::test_registries());
    builder.spawn("plain");

    assert_eq!(painted_text(&builder.finish()), None);
}
