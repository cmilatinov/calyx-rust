use engine::egui::Ui;
use engine::legion::World;
use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::{Reflect, StructInfo};
use engine::reflect_trait;
use engine::scene::{GameObject, Scene};
use std::any::TypeId;

#[reflect_trait]
pub trait TypeInspector: Send + Sync {
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

#[derive(Copy, Clone)]
pub struct InspectorContext<'a> {
    pub registry: &'a TypeRegistry,
    pub scene: &'a Scene,
    pub game_object: GameObject,
    pub parent: Option<GameObject>,
    pub world: &'a World,
    pub type_info: &'a StructInfo,
    pub field_name: Option<&'static str>,
}
