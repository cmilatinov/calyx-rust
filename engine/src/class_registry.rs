use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;

use uuid::Uuid;

use crate::component::{Component, ReflectComponent};
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{ReflectDefault, TypeInfo};
use crate::type_ids;
use crate::utils::ReflectTypeUuidDynamic;

pub struct ComponentRegistry {
    component_ids: HashMap<Uuid, TypeId>,
    component_uuids: HashMap<TypeId, Uuid>,
    components: HashMap<TypeId, Box<dyn Component>>,
    components_update: HashSet<TypeId>,
}

impl ComponentRegistry {
    pub fn new(type_registry: &TypeRegistry) -> Self {
        let mut registry = Self {
            component_ids: Default::default(),
            component_uuids: Default::default(),
            components: Default::default(),
            components_update: Default::default(),
        };
        registry.refresh_class_lists(type_registry);
        registry
    }
}

impl ComponentRegistry {
    pub fn component(&self, id: TypeId) -> Option<&dyn Component> {
        self.components.get(&id).map(|b| b.deref())
    }

    pub fn components_update(&self) -> impl Iterator<Item = (&TypeId, &Box<(dyn Component)>)> {
        self.components_update
            .iter()
            .filter_map(|id| self.components.get(id).map(|component| (id, component)))
    }

    pub fn component_by_uuid(&self, uuid: Uuid) -> Option<&dyn Component> {
        self.component_ids
            .get(&uuid)
            .and_then(|id| self.component(*id))
    }

    pub fn components(&self) -> impl Iterator<Item = (&TypeId, &Box<(dyn Component)>)> {
        self.components.iter()
    }

    pub fn components_uuid(&self) -> impl Iterator<Item = (&Uuid, &Box<(dyn Component)>)> {
        self.components
            .iter()
            .map(move |(id, comp)| (self.component_uuids.get(id).unwrap(), comp))
    }

    pub fn refresh_class_lists(&mut self, type_registry: &TypeRegistry) {
        self.components.clear();
        self.components_update.clear();
        for type_id in type_registry.all_of(type_ids!(
            ReflectDefault,
            ReflectComponent,
            ReflectTypeUuidDynamic
        )) {
            let meta_default = type_registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_component = type_registry
                .trait_meta::<ReflectComponent>(type_id)
                .unwrap();
            let meta_type_uuid = type_registry
                .trait_meta::<ReflectTypeUuidDynamic>(type_id)
                .unwrap();
            let instance = meta_default.default();
            let component = meta_component.get_boxed(instance).unwrap();
            let type_uuid = meta_type_uuid.get(component.as_reflect()).unwrap().uuid();
            let type_info = type_registry.type_info_by_id(type_id).unwrap();
            if let TypeInfo::Struct(struct_info) = type_info {
                if struct_info.attr("update").is_some() {
                    self.components_update.insert(type_id);
                }
            }
            self.component_ids.insert(type_uuid, type_id);
            self.component_uuids.insert(type_id, type_uuid);
            self.components.insert(type_id, component);
        }
    }
}
