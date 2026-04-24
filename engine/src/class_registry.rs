use std::collections::HashMap;
use std::ops::Deref;

use uuid::Uuid;

use crate::component::{
    Component, ComponentReset, ComponentUpdate, ReflectComponent, ReflectComponentReset,
    ReflectComponentUpdate,
};
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::ReflectDefault;
use crate::type_uuids;
use crate::utils::ReflectTypeUuidDynamic;

pub struct ComponentRegistry {
    components: HashMap<Uuid, Box<dyn Component>>,
    update_components: Vec<(Uuid, Box<dyn ComponentUpdate>)>,
    reset_components: HashMap<Uuid, Box<dyn ComponentReset>>,
}

impl ComponentRegistry {
    pub fn new(type_registry: &TypeRegistry) -> Self {
        let mut registry = Self {
            components: Default::default(),
            update_components: Default::default(),
            reset_components: Default::default(),
        };
        registry.refresh_class_lists(type_registry);
        registry
    }
}

impl ComponentRegistry {
    pub fn component(&self, id: Uuid) -> Option<&dyn Component> {
        self.components.get(&id).map(|b| b.deref())
    }

    pub fn components_with_update(&self) -> impl Iterator<Item = (Uuid, &dyn ComponentUpdate)> {
        self.update_components
            .iter()
            .map(|(id, updater)| (*id, updater.deref()))
    }

    pub fn reset_component(&self, id: Uuid) -> Option<&dyn ComponentReset> {
        self.reset_components.get(&id).map(|b| b.deref())
    }

    pub fn components(&self) -> impl Iterator<Item = (&Uuid, &Box<dyn Component>)> {
        self.components.iter()
    }

    pub fn refresh_class_lists(&mut self, type_registry: &TypeRegistry) {
        use crate as engine;
        self.components.clear();
        self.update_components.clear();
        self.reset_components.clear();

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

            // Discover ComponentUpdate implementations via reflection
            if let Some(meta_update) =
                type_registry.trait_meta::<ReflectComponentUpdate>(type_id)
            {
                let instance = meta_default.default();
                let updater = meta_update.get_boxed(instance).unwrap();
                self.update_components.push((type_id, updater));
            }

            // Discover ComponentReset implementations via reflection
            if let Some(meta_reset) = type_registry.trait_meta::<ReflectComponentReset>(type_id)
            {
                let instance = meta_default.default();
                let resetter = meta_reset.get_boxed(instance).unwrap();
                self.reset_components.insert(type_id, resetter);
            }
        }
    }
}
