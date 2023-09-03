use std::any::TypeId;
use engine::component::ComponentTransform;
use engine::egui::{DragValue, Ui};
use reflect::Reflect;
use reflect::ReflectDefault;
use reflect::registry::TypeRegistry;
use utils::type_ids;
use crate::inspector::type_inspector::{TypeInspector, ReflectTypeInspector};

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct TransformInspector;

impl TypeInspector for TransformInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(ComponentTransform)
    }

    fn show_inspector(&self, ui: &mut Ui, _registry: &TypeRegistry, instance: &mut dyn Reflect) {
        if let Some(t_comp) = instance.downcast_mut::<ComponentTransform>() {
            ui.horizontal(|ui| {
                ui.label("Position");
                ui.columns(3, |ui| {
                    ui[0].add(DragValue::new(&mut t_comp.transform.position.x));
                    ui[1].add(DragValue::new(&mut t_comp.transform.position.y));
                    ui[2].add(DragValue::new(&mut t_comp.transform.position.z));
                });
            });
            ui.horizontal(|ui| {
                ui.label("Rotation");
                ui.columns(3, |ui| {
                    ui[0].add(DragValue::new(&mut t_comp.transform.rotation.x));
                    ui[1].add(DragValue::new(&mut t_comp.transform.rotation.y));
                    ui[2].add(DragValue::new(&mut t_comp.transform.rotation.z));
                });
            });
            ui.horizontal(|ui| {
                ui.label("Scale");
                ui.columns(3, |ui| {
                    ui[0].add(DragValue::new(&mut t_comp.transform.scale.x));
                    ui[1].add(DragValue::new(&mut t_comp.transform.scale.y));
                    ui[2].add(DragValue::new(&mut t_comp.transform.scale.z));
                });
            });
            t_comp.transform.update_matrix();
        }
    }
}