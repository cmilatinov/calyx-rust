use engine::component::{Component, ReflectComponent};
use engine::scene::Scene;
use reflect::{Reflect, TypeUuid, ReflectDefault};

#[derive(Default, TypeUuid, Component, Reflect)]
#[reflect(Default, Component)]
#[reflect_attr(name = "Test Component")]
struct ComponentTest {
    cool: String,
    value: f32
}

impl Component for ComponentTest {
    fn update(&mut self, _scene: &Scene) {
        println!("Testing");
    }
}
