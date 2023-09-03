use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ops::Deref;
use engine::egui::Ui;
use reflect::{Reflect, ReflectDefault, TypeInfo};
use reflect::registry::TypeRegistry;
use utils::type_ids;
use crate::{EditorAppState, EditorSelection};
use crate::inspector::type_inspector::{ReflectTypeInspector, TypeInspector};
use crate::panel::Panel;
use engine::component::{Component, ReflectComponent};

struct ComponentDef {
    pub type_id: TypeId,
    pub instance: Box<dyn Component>,
}

pub struct PanelInspector {
    components: Vec<ComponentDef>,
    inspectors: HashMap<TypeId, Box<dyn Reflect>>,
    type_association: HashMap<TypeId, TypeId>,
}

impl Default for PanelInspector {
    fn default() -> Self {
        let registry = TypeRegistry::get();
        let mut components = Vec::new();
        for type_id in registry.all_of(type_ids!(ReflectDefault, ReflectComponent)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_component = registry.trait_meta::<ReflectComponent>(type_id).unwrap();
            let instance = meta_default.default();
            let component = meta_component.get_boxed(instance).unwrap();
            components.push(ComponentDef {
                type_id,
                instance: component,
            });
        }
        let mut inspectors = HashMap::new();
        let mut type_association = HashMap::new();
        for type_id in registry.all_of(type_ids!(ReflectDefault, ReflectTypeInspector)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_inspector = registry.trait_meta::<ReflectTypeInspector>(type_id).unwrap();
            let instance = meta_default.default();
            let inspector = meta_inspector.get(instance.deref()).unwrap();
            for target_type_id in inspector.target_type_ids() {
                type_association.insert(target_type_id, type_id);
            }
            inspectors.insert(type_id, instance);
        }
        Self {
            inspectors,
            type_association,
            components,
        }
    }
}


impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get_mut();
        if let Some(selection) = app_state.selection.clone() {
            if let EditorSelection::Entity(entities) = selection {
                let node_id = entities.iter().next().unwrap();
                let mut entry = app_state.scene.entry(*node_id).unwrap();
                for component in self.components.iter() {
                    if let Some(instance) = component.instance.get_instance_mut(&mut entry) {
                        self.show_inspector(ui, instance.as_reflect_mut());
                    }
                }
            }
        }
    }
}

impl PanelInspector {
    fn inspector_lookup(&self, type_id: TypeId) -> Option<&dyn TypeInspector> {
        let inspector = match self.type_association.get(&type_id) {
            Some(inspector_id) => match self.inspectors.get(&inspector_id) {
                Some(inspector) => Some(inspector),
                None => None
            }
            None => match self.inspectors.get(&type_id) {
                Some(inspector) => Some(inspector),
                None => None
            }
        };
        match inspector {
            Some(inspector_ref) => {
                let registry = TypeRegistry::get();
                let type_id = inspector_ref.as_ref().type_id();
                let meta = registry.trait_meta::<ReflectTypeInspector>(type_id)?;
                meta.get(inspector_ref.as_ref())
            },
            None => None
        }
    }

    fn show_inspector(&self, ui: &mut Ui, instance: &mut dyn Reflect) {
        let binding = TypeRegistry::get();
        let registry = binding.deref();
        match self.inspector_lookup(instance.as_any().type_id()) {
            Some(inspector) => {
                ui.collapsing(format!("{}", instance.type_name_short()), |ui| {
                   inspector.show_inspector(ui, registry, instance);
                });
            },
            None => {
                ui.collapsing(format!("{}", instance.type_name_short()), |ui| {
                    self.show_default_inspector(ui, registry, instance);
                });
            }
        };
    }

    fn show_default_inspector(
        &self,
        ui: &mut Ui,
        registry: &TypeRegistry,
        instance: &mut dyn Reflect
    ) {
        if let Some(TypeInfo::Struct(info)) = registry.type_info_by_id(instance.as_any().type_id()) {
            for (_, field) in info.fields.iter() {
                if let Some(value) = field.get_reflect_mut(instance.as_reflect_mut()) {
                    self.show_default_inspector_field(ui, registry, field.name, value);
                }
            }
        }
    }

    fn show_default_inspector_field(
        &self,
        ui: &mut Ui,
        registry: &TypeRegistry,
        field_name: &str,
        instance: &mut dyn Reflect
    ) {
        if let Some(inspector) = self.inspector_lookup(instance.as_any().type_id()) {
            ui.horizontal(|ui| {
                ui.label(field_name);
                inspector.show_inspector(ui, registry, instance);
            });
        }
    }
}