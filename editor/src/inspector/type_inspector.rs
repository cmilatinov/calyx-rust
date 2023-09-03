use std::any::TypeId;
use engine::egui::Ui;
use reflect::Reflect;
use reflect::reflect_trait;
use reflect::registry::TypeRegistry;

#[reflect_trait]
pub trait TypeInspector {
    fn target_type_ids(&self) -> Vec<TypeId>;
    fn show_inspector(
        &self,
        ui: &mut Ui,
        registry: &TypeRegistry,
        instance: &mut dyn Reflect
    );
}