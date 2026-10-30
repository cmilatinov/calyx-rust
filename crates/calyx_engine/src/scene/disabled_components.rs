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
use serde_json::Value;
use uuid::Uuid;

use crate::component::{
    ComponentCamera, ComponentCollider, ComponentGlobalPostProcessVolume, ComponentRigidBody,
};
use crate::scene::prefab_link::{component_hash, value_hash};
use crate::scene::SceneData;
use crate::utils::TypeUuid;

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

/// The component types that carried their own `enabled` field before the
/// scene kept the state.
fn legacy_enabled_types() -> [Uuid; 4] {
    [
        ComponentCamera::type_uuid(),
        ComponentCollider::type_uuid(),
        ComponentRigidBody::type_uuid(),
        ComponentGlobalPostProcessVolume::type_uuid(),
    ]
}

/// Moves the `enabled` field that cameras, colliders, rigid bodies and global
/// post-process volumes saved before the scene kept the state out of their
/// payloads. A component saved with `enabled: false` becomes a disabled
/// entry; `enabled: true` is the default and is only dropped.
///
/// A prefab instance's base hash was taken over the payload with its flag.
/// One that still matches the saved payload, a component the instance left
/// as the prefab gave it, becomes the hash of the migrated value and state,
/// so the component keeps following the prefab. One that does not match is
/// an override and stays one.
pub(crate) fn migrate_legacy_enabled(data: &mut SceneData) {
    let legacy = legacy_enabled_types();
    for (object_id, components) in data.components.iter_mut() {
        for type_uuid in legacy {
            let Some(payload @ Value::Object(_)) = components.get_mut(&type_uuid) else {
                continue;
            };
            let legacy_hash = value_hash(payload);
            let Some(enabled) = payload
                .as_object_mut()
                .and_then(|fields| fields.remove("enabled"))
            else {
                continue;
            };
            let enabled = enabled != Value::Bool(false);
            if !enabled {
                data.disabled_components
                    .entry(*object_id)
                    .or_default()
                    .insert(type_uuid);
            }
            if let Some(base) = data
                .prefab_links
                .get_mut(object_id)
                .and_then(|link| link.base.get_mut(&type_uuid))
                .filter(|base| **base == legacy_hash)
            {
                *base = component_hash(payload, enabled);
            }
        }
    }
}
