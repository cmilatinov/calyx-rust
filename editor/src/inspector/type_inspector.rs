use std::any::TypeId;
use engine::egui::mutex::RwLockWriteGuard;
use engine::egui::Ui;
use engine::indextree::NodeId;
use engine::legion::World;
use engine::scene::Scene;
use reflect::Reflect;
use reflect::reflect_trait;
use reflect::type_registry::TypeRegistry;

#[reflect_trait]
pub trait TypeInspector {
    fn target_type_ids(&self) -> Vec<TypeId>;
    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect
    );
}

pub struct InspectorContext<'a> {
    pub registry: &'a TypeRegistry,
    pub scene: &'a Scene,
    pub node: NodeId,
    pub parent_node: Option<NodeId>
}