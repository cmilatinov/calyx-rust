use legion::storage::ComponentTypeId;
use legion::world::{Entry, EntryRef};

use engine_derive::reflect_trait;
pub use engine_derive::Component;

use crate as engine;
use crate::context::ReadOnlyAssetContext;
use crate::input::Input;
use crate::reflect::Reflect;
use crate::render::Gizmos;
use crate::resource::ResourceMap;
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

pub struct ComponentEventContext<'a> {
    pub assets: &'a ReadOnlyAssetContext,
    pub scene: &'a mut Scene,
    pub game_object: GameObject,
}

#[allow(unused)]
#[reflect_trait]
pub trait Component: TypeUuidDynamic + ComponentInstance {
    fn reset(&mut self, ctx: ComponentEventContext) {}
    fn update(&mut self, ctx: ComponentEventContext, resources: &mut ResourceMap, input: &Input) {}
    fn destroy(&mut self, ctx: ComponentEventContext) {}
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {}
}
