use engine::component::{Component, ReflectComponent};
use engine::scene::Scene;
use engine::serde::{Deserialize, Serialize};
use engine::serde_json;
use reflect::{Reflect, ReflectDefault, TypeUuid};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[reflect(Default, Component)]
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
