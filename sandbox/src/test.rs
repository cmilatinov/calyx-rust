use engine::component::{Component, ReflectComponent};
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::Scene;
use engine::serde::{Deserialize, Serialize};
use engine::serde_json;
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Test Component")]
struct ComponentTest {
    cool: String,
    value: f32,
}

impl Component for ComponentTest {
    fn update(&mut self, _scene: &Scene) {
        println!("Testing");
    }
}
