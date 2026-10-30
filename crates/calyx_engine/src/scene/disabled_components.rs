//! The components a scene holds switched off.
//!
//! Every component is enabled unless the scene lists it here, so the map holds
//! only disabled entries. Systems that honor the state, such as camera
//! selection and physics, read it through [`Scene`](crate::scene::Scene).
//! Only component types that opt in through
//! [`Component::can_disable`](crate::component::Component::can_disable) can
//! be listed.

use std::collections::{HashMap, HashSet};

use legion::storage::ComponentTypeId;
use legion::Entity;

/// The disabled components of a scene, keyed by entity and then component
/// type.
#[derive(Default, Clone, Debug)]
pub(crate) struct DisabledComponents(HashMap<Entity, HashSet<ComponentTypeId>>);

impl DisabledComponents {
    /// Whether `entity`'s component of type `component` is enabled.
    pub(crate) fn is_enabled(&self, entity: Entity, component: ComponentTypeId) -> bool {
        self.0
            .get(&entity)
            .is_none_or(|disabled| !disabled.contains(&component))
    }

    /// Enables or disables `entity`'s component of type `component`. Returns
    /// whether the state changed.
    pub(crate) fn set(
        &mut self,
        entity: Entity,
        component: ComponentTypeId,
        enabled: bool,
    ) -> bool {
        if enabled {
            let Some(disabled) = self.0.get_mut(&entity) else {
                return false;
            };
            let changed = disabled.remove(&component);
            if disabled.is_empty() {
                self.0.remove(&entity);
            }
            changed
        } else {
            self.0.entry(entity).or_default().insert(component)
        }
    }

    /// Drops every entry of `entity`, which left the scene.
    pub(crate) fn forget(&mut self, entity: Entity) {
        self.0.remove(&entity);
    }
}
