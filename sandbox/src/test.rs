use engine::component::{Component, ReflectComponent};
use engine::context::ReadOnlyAssetContext;
use engine::core::Time;
use engine::egui::Key;
use engine::input::Input;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::{GameObject, Scene};
use engine::serde::{Deserialize, Serialize};
use engine::serde_json;
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Test Component", update)]
struct ComponentTest {
    cool: String,
    value: f32,
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
        if input.input(|input| input.key_down(Key::Space)) {
            self.value = self.value + 1.0 * time.delta_time();
        }
    }
}
