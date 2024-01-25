use egui::Ui;
use indextree::NodeId;
use legion::storage::ComponentTypeId;
use legion::world::{Entry, EntryRef};

use engine_derive::reflect_trait;
pub use engine_derive::Component;

use crate as engine;
use crate::reflect::Reflect;
use crate::render::Gizmos;
use crate::scene::Scene;
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

#[reflect_trait]
pub trait Component: TypeUuidDynamic + ComponentInstance {
    fn start(&mut self, _scene: &Scene, _node: NodeId) {}
    fn update(&mut self, _scene: &mut Scene, _node: NodeId, _ui: &Ui) {}
    fn destroy(&mut self, _scene: &Scene, _node: NodeId) {}
    fn draw_gizmos(&self, _scene: &Scene, _node: NodeId, _gizmos: &mut Gizmos) {}
}
