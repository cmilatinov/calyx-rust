use std::collections::{HashMap, HashSet};
use std::ops::Deref;

use uuid::Uuid;

use crate::component::{Component, ReflectComponent};
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{ReflectDefault, TypeInfo};
use crate::type_uuids;
use crate::utils::ReflectTypeUuidDynamic;

pub struct ComponentRegistry {
    components: HashMap<Uuid, Box<dyn Component>>,
    components_update: HashSet<Uuid>,
}

impl ComponentRegistry {
    pub fn new(type_registry: &TypeRegistry) -> Self {
        let mut registry = Self {
            components: Default::default(),
            components_update: Default::default(),
        };
        registry.refresh_class_lists(type_registry);
        registry
    }
}

impl ComponentRegistry {
    pub fn component(&self, id: Uuid) -> Option<&dyn Component> {
        self.components.get(&id).map(|b| b.deref())
    }

    pub fn components_update(&self) -> impl Iterator<Item = (Uuid, &Box<(dyn Component)>)> {
        self.components_update
            .iter()
            .filter_map(|id| self.components.get(id).map(|component| (*id, component)))
    }

    pub fn components(&self) -> impl Iterator<Item = (&Uuid, &Box<(dyn Component)>)> {
        self.components.iter()
    }

    pub fn refresh_class_lists(&mut self, type_registry: &TypeRegistry) {
        use crate as engine;
        self.components.clear();
        self.components_update.clear();
        for type_id in type_registry.all_of(type_uuids!(
            ReflectDefault,
            ReflectComponent,
            ReflectTypeUuidDynamic
        )) {
            let meta_default = type_registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_component = type_registry
                .trait_meta::<ReflectComponent>(type_id)
                .unwrap();
            let instance = meta_default.default();
            let component = meta_component.get_boxed(instance).unwrap();
            let type_info = type_registry.type_info_by_id(type_id).unwrap();
            if let TypeInfo::Struct(struct_info) = type_info {
                if struct_info.attr("update").is_some() {
                    self.components_update.insert(type_id);
                }
            }
            self.components.insert(type_id, component);
        }
    }
}
