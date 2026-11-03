use std::collections::HashMap;
use std::ops::Deref;

use uuid::Uuid;

use crate::component::{
    Component, ReflectComponent, ReflectComponentHud, ReflectComponentReset, ReflectComponentStart,
    ReflectComponentUpdate,
};
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::ReflectDefault;
use crate::type_uuids;
use crate::utils::ReflectTypeUuidDynamic;

/// Registry of reflected component types and their optional lifecycle hooks.
///
/// Each component type has one prototype, a default instance that binds,
/// reads and serializes components of its type. The lifecycle hooks are kept
/// as the type's reflected hook functions instead, with no instance: the
/// scene runs each one on the component the object holds.
pub struct ComponentRegistry {
    components: HashMap<Uuid, Box<dyn Component>>,
    start_components: Vec<(Uuid, ReflectComponentStart)>,
    update_components: Vec<(Uuid, ReflectComponentUpdate)>,
    hud_components: Vec<(Uuid, ReflectComponentHud)>,
    reset_components: HashMap<Uuid, ReflectComponentReset>,
}

impl ComponentRegistry {
    /// Builds a component registry from the current reflection type registry.
    pub fn new(type_registry: &TypeRegistry) -> Self {
        let mut registry = Self {
            components: Default::default(),
            start_components: Default::default(),
            update_components: Default::default(),
            hud_components: Default::default(),
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

    /// Whether components of type `id` can be disabled: the type is
    /// registered and opts in through [`Component::can_disable`].
    pub fn can_disable(&self, id: Uuid) -> bool {
        self.component(id).is_some_and(Component::can_disable)
    }

    /// Iterates the update hooks of the components that implement
    /// [`ComponentUpdate`](crate::component::ComponentUpdate).
    pub fn components_with_update(&self) -> impl Iterator<Item = (Uuid, &ReflectComponentUpdate)> {
        self.update_components.iter().map(|(id, hook)| (*id, hook))
    }

    /// Iterates the HUD hooks of the components that implement
    /// [`ComponentHud`](crate::component::ComponentHud).
    pub fn components_with_hud(&self) -> impl Iterator<Item = (Uuid, &ReflectComponentHud)> {
        self.hud_components.iter().map(|(id, hook)| (*id, hook))
    }

    /// Iterates the start hooks of the components that implement
    /// [`ComponentStart`](crate::component::ComponentStart).
    pub fn components_with_start(&self) -> impl Iterator<Item = (Uuid, &ReflectComponentStart)> {
        self.start_components.iter().map(|(id, hook)| (*id, hook))
    }

    /// Returns the reset hook registered for `id`, if any.
    pub fn reset_component(&self, id: Uuid) -> Option<&ReflectComponentReset> {
        self.reset_components.get(&id)
    }

    /// Iterates every registered component binder keyed by type UUID.
    pub fn components(&self) -> impl Iterator<Item = (&Uuid, &Box<dyn Component>)> {
        self.components.iter()
    }

    /// Rebuilds the component lifecycle lookup tables from
    /// `type_registry`.
    pub fn refresh_class_lists(&mut self, type_registry: &TypeRegistry) {
        use crate as calyx_engine;
        self.components.clear();
        self.start_components.clear();
        self.update_components.clear();
        self.hud_components.clear();
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

            if let Some(hook) = type_registry.trait_meta::<ReflectComponentStart>(type_id) {
                self.start_components.push((type_id, hook.clone()));
            }
            if let Some(hook) = type_registry.trait_meta::<ReflectComponentUpdate>(type_id) {
                self.update_components.push((type_id, hook.clone()));
            }
            if let Some(hook) = type_registry.trait_meta::<ReflectComponentHud>(type_id) {
                self.hud_components.push((type_id, hook.clone()));
            }
            if let Some(hook) = type_registry.trait_meta::<ReflectComponentReset>(type_id) {
                self.reset_components.insert(type_id, hook.clone());
            }
        }
    }
}
