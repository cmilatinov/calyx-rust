use indextree::NodeId;
use legion::storage::ComponentTypeId;
use legion::world::{Entry, EntryRef};

pub use engine_derive::Component;
use reflect::{reflect_trait, Reflect};

use crate::render::Gizmos;
use crate::scene::Scene;
use crate::utils::TypeUuidDynamic;

pub trait ComponentInstance {
    fn component_type_id(&self) -> ComponentTypeId;
    fn get_instance<'a>(&self, entry: &'a EntryRef) -> Option<&'a dyn Component>;
    fn get_instance_mut<'a>(&self, entry: &'a mut Entry) -> Option<&'a mut dyn Component>;
    fn bind_instance(&self, entry: &mut Entry, instance: Box<dyn Reflect>);
    fn remove_instance(&self, entry: &mut Entry);
}

#[reflect_trait]
pub trait Component: TypeUuidDynamic + Reflect + ComponentInstance {
    fn start(&mut self, _scene: &Scene) {}
    fn update(&mut self, _scene: &Scene) {}
    fn destroy(&mut self, _scene: &Scene) {}
    fn draw_gizmos(&self, _scene: &Scene, _node: NodeId, _gizmos: &mut Gizmos) {}
}
