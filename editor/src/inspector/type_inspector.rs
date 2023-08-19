use std::any::TypeId;
use engine::egui::Ui;
use reflect::{NamedField, Reflect};
use reflect::reflect_trait;

#[reflect_trait]
pub trait TypeInspector {
    fn target_type_id(&self) -> TypeId;
    fn show_inspector(&self, instance: &mut dyn Reflect, ui: &mut Ui);
    fn show_inspector_field(&self, instance: &mut dyn Reflect, field: &NamedField, ui: &mut Ui);
}