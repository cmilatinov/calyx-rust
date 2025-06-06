use engine::component::{Component, ReflectComponent};
use engine::context::ReadOnlyAssetContext;
use engine::core::Time;
use engine::input::Input;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::{GameObject, Scene};
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "14344d10-bba0-47e4-8c06-520352ddfc11"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Test Component", update)]
struct ComponentTest {
    cool: String,
    value: f32,
    another: String,
}

impl Component for ComponentTest {
    fn update(
        &mut self,
        _game: &ReadOnlyAssetContext,
        _scene: &mut Scene,
        _game_object: GameObject,
        time: &Time,
        input: &Input,
    ) {
        if input.input(|input| input.key_down(egui::Key::Space)) {
            self.value = self.value + 1.0 * time.delta_time();
        }
    }
}
