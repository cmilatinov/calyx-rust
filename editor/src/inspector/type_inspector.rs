use engine::egui::Ui;
use engine::indextree::NodeId;
use engine::legion::World;
use engine::scene::Scene;
use reflect::type_registry::TypeRegistry;
use reflect::{reflect_trait, Reflect};
use std::any::TypeId;

#[reflect_trait]
pub trait TypeInspector {
    fn target_type_ids(&self) -> Vec<TypeId>;
    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect);
    fn show_inspector_context(
        &self,
        _ui: &mut Ui,
        _ctx: &InspectorContext,
        _instance: &mut dyn Reflect,
    ) {
    }
}

pub struct InspectorContext<'a> {
    pub registry: &'a TypeRegistry,
    pub scene: &'a Scene,
    pub node: NodeId,
    pub parent_node: Option<NodeId>,
    pub world: &'a World,
}
