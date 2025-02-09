use legion::storage::ComponentTypeId;
use legion::world::{Entry, EntryRef};

use engine_derive::reflect_trait;
pub use engine_derive::Component;

use crate as engine;
use crate::input::Input;
use crate::reflect::Reflect;
use crate::render::Gizmos;
use crate::scene::{GameObject, Scene};
use crate::utils::TypeUuidDynamic;

pub trait ComponentInstance: Reflect {
    fn component_type_id(&self) -> ComponentTypeId;
    fn get_instance<'a>(&self, entry: &'a EntryRef) -> Option<&'a dyn Component>;
    fn get_instance_mut<'a>(&self, entry: &'a mut Entry) -> Option<&'a mut dyn Component>;
    fn bind_instance(&self, entry: &mut Entry, instance: Box<dyn Reflect>) -> bool;
    fn remove_instance(&self, entry: &mut Entry);
    fn serialize(&self) -> Option<serde_json::Value>;
    fn deserialize(&self, value: serde_json::Value) -> Option<Box<dyn Reflect>>;
    fn deserialize_in_place(&mut self, value: serde_json::Value) -> bool {
        if let Some(value) = self.deserialize(value) {
            self.assign(value)
        } else {
            false
        }
    }
}

#[allow(unused)]
#[reflect_trait]
pub trait Component: TypeUuidDynamic + ComponentInstance {
    fn reset(&mut self, scene: &mut Scene, game_object: GameObject) {}
    fn start(&mut self, scene: &mut Scene, game_object: GameObject) {}
    fn update(&mut self, scene: &mut Scene, game_object: GameObject, input: &Input) {}
    fn destroy(&mut self, scene: &mut Scene, game_object: GameObject) {}
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {}
}
