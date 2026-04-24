use std::collections::HashMap;
use std::ops::Deref;

use uuid::Uuid;

use crate::component::{Component, ComponentResetFn, ComponentUpdateFn, ReflectComponent};
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{ReflectDefault, TypeInfo};
use crate::type_uuids;
use crate::utils::ReflectTypeUuidDynamic;
use crate::{ComponentResetRegistration, ComponentUpdateRegistration};

pub struct ComponentRegistry {
    components: HashMap<Uuid, Box<dyn Component>>,
    update_fns: HashMap<Uuid, ComponentUpdateFn>,
    reset_fns: HashMap<Uuid, ComponentResetFn>,
}

impl ComponentRegistry {
    pub fn new(type_registry: &TypeRegistry) -> Self {
        let mut registry = Self {
            components: Default::default(),
            update_fns: Default::default(),
            reset_fns: Default::default(),
        };
        registry.refresh_class_lists(type_registry);
        registry
    }
}

impl ComponentRegistry {
    pub fn component(&self, id: Uuid) -> Option<&dyn Component> {
        self.components.get(&id).map(|b| b.deref())
    }

    pub fn update_fn(&self, id: Uuid) -> Option<ComponentUpdateFn> {
        self.update_fns.get(&id).copied()
    }

    pub fn reset_fn(&self, id: Uuid) -> Option<ComponentResetFn> {
        self.reset_fns.get(&id).copied()
    }

    pub fn components_with_update(&self) -> impl Iterator<Item = (Uuid, ComponentUpdateFn)> + '_ {
        self.update_fns.iter().map(|(id, f)| (*id, *f))
    }

    pub fn components(&self) -> impl Iterator<Item = (&Uuid, &Box<dyn Component>)> {
        self.components.iter()
    }

    pub fn refresh_class_lists(&mut self, type_registry: &TypeRegistry) {
        use crate as engine;
        self.components.clear();
        self.update_fns.clear();
        self.reset_fns.clear();

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
            self.components.insert(type_id, component);
        }

        // Collect fn-pointer registrations from inventory
        for reg in inventory::iter::<ComponentUpdateRegistration> {
            self.update_fns.insert(reg.type_uuid, reg.update_fn);
        }
        for reg in inventory::iter::<ComponentResetRegistration> {
            self.reset_fns.insert(reg.type_uuid, reg.reset_fn);
        }
    }
}
