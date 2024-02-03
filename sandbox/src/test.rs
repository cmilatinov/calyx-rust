use engine::component::{Component, ReflectComponent};
use engine::core::Time;
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
    fn update(
        &mut self,
        _scene: &mut Scene,
        _node: engine::indextree::NodeId,
        _ui: &engine::egui::Ui,
    ) {
        self.value = self.value + 1.0 * Time::delta_time();
    }
}
