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

/// Registry of reflected component types and their optional lifecycle hooks.
pub struct ComponentRegistry {
    components: HashMap<Uuid, Box<dyn Component>>,
    update_components: Vec<(Uuid, Box<dyn ComponentUpdate>)>,
    reset_components: HashMap<Uuid, Box<dyn ComponentReset>>,
}

impl ComponentRegistry {
    /// Builds a component registry from the current reflection type registry.
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
    /// Returns the reflected component binder for `id`.
    pub fn component(&self, id: Uuid) -> Option<&dyn Component> {
        self.components.get(&id).map(|b| b.deref())
    }

    /// Iterates components that implement [`ComponentUpdate`].
    pub fn components_with_update(&self) -> impl Iterator<Item = (Uuid, &dyn ComponentUpdate)> {
        self.update_components
            .iter()
            .map(|(id, updater)| (*id, updater.deref()))
    }

    /// Returns the reset hook registered for `id`, if any.
    pub fn reset_component(&self, id: Uuid) -> Option<&dyn ComponentReset> {
        self.reset_components.get(&id).map(|b| b.deref())
    }

    /// Iterates every registered component binder keyed by type UUID.
    pub fn components(&self) -> impl Iterator<Item = (&Uuid, &Box<dyn Component>)> {
        self.components.iter()
    }

    /// Rebuilds the component, update, and reset lookup tables from
    /// `type_registry`.
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
            let Some(meta_default) = type_registry.trait_meta::<ReflectDefault>(type_id) else {
                log::warn!("Skipping component {type_id}: missing ReflectDefault metadata");
                continue;
            };
            let Some(meta_component) = type_registry.trait_meta::<ReflectComponent>(type_id) else {
                log::warn!("Skipping component {type_id}: missing ReflectComponent metadata");
                continue;
            };
            let instance = meta_default.default();
            let Ok(component) = meta_component.get_boxed(instance) else {
                log::warn!("Skipping component {type_id}: failed to bind Component metadata");
                continue;
            };
            self.components.insert(type_id, component);

            // Discover ComponentUpdate implementations via reflection
            if let Some(meta_update) = type_registry.trait_meta::<ReflectComponentUpdate>(type_id) {
                let instance = meta_default.default();
                if let Ok(updater) = meta_update.get_boxed(instance) {
                    self.update_components.push((type_id, updater));
                } else {
                    log::warn!(
                        "Skipping update hook for component {type_id}: failed to bind metadata"
                    );
                }
            }

            // Discover ComponentReset implementations via reflection
            if let Some(meta_reset) = type_registry.trait_meta::<ReflectComponentReset>(type_id) {
                let instance = meta_default.default();
                if let Ok(resetter) = meta_reset.get_boxed(instance) {
                    self.reset_components.insert(type_id, resetter);
                } else {
                    log::warn!(
                        "Skipping reset hook for component {type_id}: failed to bind metadata"
                    );
                }
            }
        }
    }
}
