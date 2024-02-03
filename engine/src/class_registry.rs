use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};

use uuid::Uuid;

use crate::component::{Component, ReflectComponent};
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{ReflectDefault, TypeInfo};
use crate::type_ids;
use crate::utils::{singleton, Init, ReflectTypeUuidDynamic};

#[derive(Default)]
pub struct ClassRegistry {
    component_ids: HashMap<Uuid, TypeId>,
    component_uuids: HashMap<TypeId, Uuid>,
    components: HashMap<TypeId, Box<dyn Component>>,
    components_update: HashSet<TypeId>,
}

impl Init for ClassRegistry {
    fn initialize(&mut self) {
        self.refresh_class_lists();
    }
}

singleton!(ClassRegistry);

impl ClassRegistry {
    pub fn component(&self, id: TypeId) -> Option<&dyn Component> {
        self.components.get(&id).map(|b| b.deref())
    }

    pub fn components_update(&self) -> impl Iterator<Item = (&TypeId, &Box<(dyn Component)>)> {
        self.components_update
            .iter()
            .filter_map(|id| if let Some(component) = self.components.get(id) {
                Some((id, component))
            } else {
                None
            })
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

    pub fn refresh_class_lists(&mut self) {
        self.components.clear();
        self.components_update.clear();
        let registry = TypeRegistry::get();
        for type_id in registry.all_of(type_ids!(
            ReflectDefault,
            ReflectComponent,
            ReflectTypeUuidDynamic
        )) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_component = registry.trait_meta::<ReflectComponent>(type_id).unwrap();
            let meta_type_uuid = registry
                .trait_meta::<ReflectTypeUuidDynamic>(type_id)
                .unwrap();
            let instance = meta_default.default();
            let component = meta_component.get_boxed(instance).unwrap();
            let type_uuid = meta_type_uuid.get(component.as_reflect()).unwrap().uuid();
            let type_info = registry.type_info_by_id(type_id).unwrap();
            if let TypeInfo::Struct(struct_info) = type_info {
                if let Some(_) = struct_info.attr("update") {
                    self.components_update.insert(type_id);
                }
            }
            self.component_ids.insert(type_uuid, type_id);
            self.component_uuids.insert(type_id, type_uuid);
            self.components.insert(type_id, component);
        }
    }
}
