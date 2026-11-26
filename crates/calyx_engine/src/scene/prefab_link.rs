//! Links from prefab instances back to their prefab asset, and the update that
//! brings an instance in line with its prefab after the prefab changes.
//!
//! [`Scene::instantiate_linked_prefab`] gives every object of the new instance
//! a [`PrefabLink`]: the prefab asset and the prefab object it was made from,
//! and a hash of each of its components' values as the prefab gave them. The
//! scene saves the links with its objects.
//!
//! [`Scene::update_prefab_instances`] updates the instances only when it is
//! called; nothing else changes an instance after its prefab changes. Its
//! rules:
//!
//! - An instance keeps its root object's transform and every object's id.
//! - A prefab whose root object is no longer the one its instances were made
//!   from is refused, and nothing changes.
//! - A component is an override when its value on the instance differs from
//!   the value the prefab last gave it, or when the instance added or removed
//!   it. An override is kept as it is. Every other component takes the
//!   prefab's new value, is added when the prefab added it, and is removed
//!   when the prefab removed it.
//! - An object the prefab added is added to the instance under its prefab
//!   parent, after its prefab sibling, unless the instance deleted that
//!   parent.
//! - An object the prefab removed is deleted from the instance, unless it
//!   carries an override; then it stays as an ordinary, unlinked object. A
//!   child of a deleted object that is not deleted itself, such as an object
//!   added to the instance, moves to the nearest ancestor that stays. A
//!   reference the prefab still makes to a removed object reaches the
//!   object the instance kept, or no object, in this and later updates.
//! - An object the instance deleted stays deleted, and a reference the
//!   prefab makes to it reaches no object.
//! - Every object keeps its parent and sibling order in the instance. An
//!   object moved out from under its instance root stays part of the
//!   instance and is updated where it is.
//! - Deleting an instance's root object ends the instance: every member that
//!   survives it becomes an ordinary, unlinked object.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use bimap::BiHashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::class_registry::ComponentRegistry;
use crate::component::{ComponentID, ComponentTransform};
use crate::reflect::TypeInfo;
use crate::scene::dropped_components::registered_type_name;
use crate::scene::{
    DropReason, DroppedComponent, GameObject, Prefab, Scene, SceneError, SiblingDir,
};
use crate::utils::TypeUuid;

/// The link from one object of a prefab instance to the prefab object it was
/// made from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefabLink {
    /// The prefab asset's id.
    pub prefab: Uuid,
    /// The UUID of the object inside the prefab this object was made from.
    pub object: Uuid,
    /// The UUID of the instance's root object. It names the instance this
    /// object belongs to wherever the object sits in the hierarchy, so an
    /// object moved out from under its instance root still updates.
    pub instance: Uuid,
    /// A hash of each component's value and enabled state, keyed by component
    /// type UUID, as the prefab last gave it to this object. A component whose
    /// value or state hashes differently is an override.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub base: BTreeMap<Uuid, u64>,
    /// On the instance's root object only: every object the prefab held when
    /// the instance was made or last updated, so an object the instance
    /// deleted is told apart from one the prefab added since.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub objects: BTreeSet<Uuid>,
    /// On the instance's root object only: each object the prefab removed,
    /// keyed by its prefab UUID, with the id a reference the prefab still
    /// makes to it takes in the instance. That is the object the instance
    /// kept for its overrides, or an id that names no object when the
    /// instance holds none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub removed: BTreeMap<Uuid, Uuid>,
}

/// What [`Scene::update_prefab_instances`] changed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PrefabUpdate {
    /// Instances updated.
    pub instances: usize,
    /// Objects added to instances because the prefab added them.
    pub added: usize,
    /// Objects deleted from instances because the prefab removed them.
    pub removed: usize,
    /// Objects the prefab removed that were kept, unlinked, for their
    /// overrides.
    pub unlinked: usize,
    /// Components kept as overrides.
    pub kept_overrides: usize,
    /// Prefab components skipped because their type is not registered or
    /// their data does not deserialize.
    pub dropped: Vec<DroppedComponent>,
}

/// A stable 64-bit FNV-1a hash of `value`'s JSON text. Object keys are
/// sorted, so equal values hash equally across runs and builds.
fn value_hash(value: &serde_json::Value) -> u64 {
    let text = value.to_string();
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The hash of a component's `value` and whether it is `enabled`. An enabled
/// component hashes as its value alone.
fn component_hash(value: &serde_json::Value, enabled: bool) -> u64 {
    if enabled {
        value_hash(value)
    } else {
        value_hash(&serde_json::json!({ "disabled": value }))
    }
}

/// The id a reference to the prefab object `source` takes in the instance
/// rooted at `instance` when the instance does not hold that object. It is
/// the same on every update, so an update with nothing new changes nothing.
/// Mixing in the instance's random root id makes it as unlikely to name an
/// existing object as a fresh random id.
fn missing_member_id(instance: Uuid, source: Uuid) -> Uuid {
    Uuid::from_u128(instance.as_u128() ^ source.as_u128())
}

impl Scene {
    /// The prefab link of `game_object`, when it is part of a prefab
    /// instance.
    pub fn prefab_link(&self, game_object: GameObject) -> Option<&PrefabLink> {
        self.prefab_links.get(&self.uuid(game_object))
    }

    /// The ids of the prefab assets the scene holds instances of.
    pub fn linked_prefabs(&self) -> BTreeSet<Uuid> {
        self.prefab_links
            .iter()
            .filter(|(game_object_id, _)| self.find(**game_object_id).is_some())
            .map(|(_, link)| link.prefab)
            .collect()
    }

    /// The root objects of the scene's instances of the prefab asset
    /// `prefab_id`, in scene order. An instance's root is the object its
    /// links name as their instance, whatever prefab object it was made from.
    pub fn prefab_instances(&self, prefab_id: Uuid) -> Vec<GameObject> {
        self.graph
            .descendants_ordered(self.root())
            .filter(|game_object| {
                let game_object_id = self.uuid(*game_object);
                self.prefab_links
                    .get(&game_object_id)
                    .is_some_and(|link| link.prefab == prefab_id && link.instance == game_object_id)
            })
            .collect()
    }

    /// Checks that [`Self::update_prefab_instances`] can update the scene's
    /// instances of `prefab`, the prefab asset `prefab_id`, without failing
    /// midway: the prefab is well formed and its root object is still the
    /// one each instance was made from. An instance cannot follow a prefab
    /// whose root object changed, so that prefab is refused.
    pub fn check_prefab_update(&self, prefab_id: Uuid, prefab: &Prefab) -> Result<(), SceneError> {
        self.validate_prefab(prefab)?;
        for root in self.prefab_instances(prefab_id) {
            let Some(link) = self.prefab_link(root) else {
                continue;
            };
            if link.object != prefab.root {
                return Err(SceneError::InvalidPrefab(format!(
                    "its root object changed from {} to {}, and its instance {} cannot follow it",
                    link.object,
                    prefab.root,
                    self.name(root)
                )));
            }
        }
        Ok(())
    }

    /// Instantiates `prefab`, the prefab asset `prefab_id`, as
    /// [`Self::instantiate_prefab_reporting`] does, and links every object of
    /// the instance to the prefab object it was made from, so
    /// [`Self::update_prefab_instances`] can update it later.
    pub fn instantiate_linked_prefab(
        &mut self,
        prefab_id: Uuid,
        prefab: &Prefab,
        parent: Option<GameObject>,
    ) -> Result<(GameObject, Vec<DroppedComponent>), SceneError> {
        let (game_object, dropped, mapping) = self.instantiate_prefab_mapped(prefab, parent)?;
        let instance = self.uuid(game_object);
        let registry_ref = self.registries().components.clone();
        let registry = registry_ref.read();
        for (source, target) in mapping.iter() {
            let Some(instance_object) = self.find(*target) else {
                continue;
            };
            let objects = if *source == prefab.root {
                prefab.data.components.keys().copied().collect()
            } else {
                BTreeSet::new()
            };
            let link = PrefabLink {
                prefab: prefab_id,
                object: *source,
                instance,
                base: self.component_hashes(&registry, instance_object),
                objects,
                removed: BTreeMap::new(),
            };
            self.prefab_links.insert(*target, link);
        }
        Ok((game_object, dropped))
    }

    /// Updates every instance of `prefab`, the prefab asset `prefab_id`, from
    /// the prefab's current data, following the rules in the
    /// [module docs](self). Instances made by
    /// [`Self::instantiate_linked_prefab`] are the ones updated.
    ///
    /// Returns [`SceneError::InvalidPrefab`], and changes nothing, when
    /// [`Self::check_prefab_update`] refuses the prefab.
    pub fn update_prefab_instances(
        &mut self,
        prefab_id: Uuid,
        prefab: &Prefab,
    ) -> Result<PrefabUpdate, SceneError> {
        self.check_prefab_update(prefab_id, prefab)?;
        let mut update = PrefabUpdate::default();
        for root in self.prefab_instances(prefab_id) {
            self.update_prefab_instance(root, prefab_id, prefab, &mut update)?;
            update.instances += 1;
        }
        self.flush_deletes();
        Ok(update)
    }

    /// The hash of each registered component's value and enabled state on
    /// `game_object`, keyed by component type UUID.
    fn component_hashes(
        &self,
        registry: &ComponentRegistry,
        game_object: GameObject,
    ) -> BTreeMap<Uuid, u64> {
        let Some(entry) = self.entry(game_object) else {
            return BTreeMap::new();
        };
        registry
            .components()
            .filter_map(|(type_uuid, component)| {
                let value = component.get_instance(&entry)?.serialize()?;
                let enabled =
                    self.is_type_enabled(game_object.entity, component.component_type_id());
                Some((*type_uuid, component_hash(&value, enabled)))
            })
            .collect()
    }

    /// The objects of the instance rooted at `root`, keyed by the prefab
    /// object each was made from: every object whose link names `root` as
    /// its instance, wherever it sits in the hierarchy. Another instance of
    /// the same prefab, nested inside it or not, is not part of it.
    fn instance_members(
        &self,
        root: GameObject,
        prefab_id: Uuid,
        prefab_root: Uuid,
    ) -> BTreeMap<Uuid, GameObject> {
        let instance = self.uuid(root);
        let mut members = BTreeMap::from([(prefab_root, root)]);
        for (game_object_id, link) in &self.prefab_links {
            if link.prefab != prefab_id || link.instance != instance || link.object == prefab_root {
                continue;
            }
            if let Some(game_object) = self.find(*game_object_id) {
                members.entry(link.object).or_insert(game_object);
            }
        }
        members
    }

    fn update_prefab_instance(
        &mut self,
        root: GameObject,
        prefab_id: Uuid,
        prefab: &Prefab,
        update: &mut PrefabUpdate,
    ) -> Result<(), SceneError> {
        let instance = self.uuid(root);
        let mut members = self.instance_members(root, prefab_id, prefab.root);
        let known = self
            .prefab_link(root)
            .map(|link| link.objects.clone())
            .filter(|objects| !objects.is_empty())
            .unwrap_or_else(|| members.keys().copied().collect());
        let mut removed = self
            .prefab_link(root)
            .map(|link| link.removed.clone())
            .unwrap_or_default();

        let registry_ref = self.registries().components.clone();
        let registry = registry_ref.read();
        let types_ref = self.registries().types.clone();
        let types = types_ref.read();

        // Objects the prefab removed. A reference the prefab still makes to
        // one reaches the object the instance keeps for it, or no object.
        for source in &known {
            if !prefab.data.components.contains_key(source) && !members.contains_key(source) {
                removed
                    .entry(*source)
                    .or_insert_with(|| missing_member_id(instance, *source));
            }
        }
        let mut deleted = HashSet::new();
        for (source, game_object) in &members {
            if prefab.data.components.contains_key(source) {
                continue;
            }
            let base = self
                .prefab_link(*game_object)
                .map(|link| link.base.clone())
                .unwrap_or_default();
            if self.component_hashes(&registry, *game_object) == base {
                deleted.insert(*game_object);
                removed.insert(*source, missing_member_id(instance, *source));
            } else {
                let game_object_id = self.uuid(*game_object);
                self.prefab_links.remove(&game_object_id);
                removed.insert(*source, game_object_id);
                update.unlinked += 1;
            }
        }
        // An object the prefab holds again is no longer removed.
        removed.retain(|source, _| !prefab.data.components.contains_key(source));
        for game_object in &deleted {
            let mut keeper = self.parent(*game_object);
            while let Some(ancestor) = keeper.filter(|ancestor| deleted.contains(ancestor)) {
                keeper = self.parent(ancestor);
            }
            // A direct parent keeps the children where the deleted object
            // was; a further ancestor takes them at the end.
            let direct = keeper == self.parent(*game_object);
            let children = self.children_ordered(*game_object).collect::<Vec<_>>();
            for child in children {
                if !deleted.contains(&child) {
                    let sibling = direct.then_some((*game_object, SiblingDir::Before));
                    self.set_parent_with_sibling(child, keeper, sibling)?;
                }
            }
        }
        for game_object in &deleted {
            self.delete(*game_object);
        }
        update.removed += deleted.len();
        members.retain(|source, _| prefab.data.components.contains_key(source));

        // Objects the prefab added, parents first, each after its prefab
        // sibling. The whole prefab tree is walked, so a member the instance
        // moved out from under a deleted object still gets its new children.
        let mut added = HashSet::new();
        let mut stack = vec![prefab.root];
        while let Some(parent_source) = stack.pop() {
            let parent = members.get(&parent_source).copied();
            let mut previous: Option<GameObject> = None;
            for child_source in prefab
                .data
                .hierarchy
                .get(&parent_source)
                .into_iter()
                .flatten()
            {
                stack.push(*child_source);
                if let Some(child) = members.get(child_source).copied() {
                    if parent.is_some() && self.parent(child) == parent {
                        previous = Some(child);
                    }
                    continue;
                }
                let Some(parent) = parent else {
                    // The instance deleted its parent.
                    continue;
                };
                if known.contains(child_source) {
                    // The instance deleted it.
                    continue;
                }
                let id = prefab.data.components[child_source]
                    .get(&ComponentID::type_uuid())
                    .and_then(|data| serde_json::from_value::<ComponentID>(data.clone()).ok())
                    .ok_or_else(|| {
                        SceneError::InvalidPrefab(format!("object {child_source} has no valid id"))
                    })?;
                let child = self.spawn(
                    ComponentID {
                        id: Uuid::new_v4(),
                        ..id
                    },
                    Some(parent),
                );
                let sibling = match previous {
                    Some(previous) => Some((previous, SiblingDir::After)),
                    None => self
                        .children_ordered(parent)
                        .next()
                        .filter(|first| *first != child)
                        .map(|first| (first, SiblingDir::Before)),
                };
                if sibling.is_some() {
                    self.set_parent_with_sibling(child, Some(parent), sibling)?;
                }
                members.insert(*child_source, child);
                added.insert(child);
                previous = Some(child);
                update.added += 1;
            }
        }

        // Every member takes the prefab's components it does not override. A
        // reference to a prefab object the instance does not hold, such as
        // one it deleted, maps to an id of its own that names no object, so
        // it never falls back to the prefab's object.
        let mut mapping = members
            .iter()
            .map(|(source, game_object)| (*source, self.uuid(*game_object)))
            .collect::<BiHashMap<_, _>>();
        for source in prefab.data.components.keys() {
            if !members.contains_key(source) {
                mapping.insert(*source, missing_member_id(instance, *source));
            }
        }
        for (source, target) in &removed {
            mapping.insert(*source, *target);
        }
        let objects = prefab
            .data
            .components
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        for (source, game_object) in &members {
            let game_object = *game_object;
            let is_root = *source == prefab.root;
            let components = &prefab.data.components[source];
            let base = self
                .prefab_link(game_object)
                .map(|link| link.base.clone())
                .unwrap_or_default();
            let current = self.component_hashes(&registry, game_object);
            let mut next_base = BTreeMap::new();
            let type_uuids = base
                .keys()
                .chain(components.keys())
                .copied()
                .collect::<BTreeSet<_>>();
            for type_uuid in type_uuids {
                let keep_base = |next_base: &mut BTreeMap<Uuid, u64>| {
                    if let Some(hash) = base.get(&type_uuid) {
                        next_base.insert(type_uuid, *hash);
                    }
                };
                if is_root && type_uuid == ComponentTransform::type_uuid() {
                    // An instance keeps its root transform.
                    keep_base(&mut next_base);
                    continue;
                }
                // An object just added holds nothing of its own yet.
                let overridden = !added.contains(&game_object)
                    && match (base.get(&type_uuid), current.get(&type_uuid)) {
                        (Some(base), Some(current)) => base != current,
                        (None, None) => false,
                        _ => true,
                    };
                if overridden {
                    keep_base(&mut next_base);
                    update.kept_overrides += 1;
                    continue;
                }
                let Some(data) = components.get(&type_uuid) else {
                    if let Some(component) = registry.component(type_uuid) {
                        self.remove_instance(game_object, component);
                    }
                    continue;
                };
                let object_name = self.name(game_object);
                let mut skip = |type_name: Option<String>, reason: DropReason| {
                    update.dropped.push(DroppedComponent {
                        object_id: *source,
                        object_name: object_name.clone(),
                        type_uuid,
                        type_name,
                        reason,
                    });
                };
                if type_uuid == ComponentID::type_uuid() {
                    match serde_json::from_value::<ComponentID>(data.clone()) {
                        Ok(id) => {
                            self.write_component::<ComponentID, _>(game_object, |current| {
                                current.name = id.name;
                                current.visible = id.visible;
                            });
                        }
                        Err(err) => {
                            skip(
                                Some(std::any::type_name::<ComponentID>().to_owned()),
                                DropReason::Invalid(err.to_string()),
                            );
                            keep_base(&mut next_base);
                            continue;
                        }
                    }
                } else {
                    let component = match types.type_info_by_id(type_uuid) {
                        Some(TypeInfo::Struct(_)) => registry.component(type_uuid),
                        _ => None,
                    };
                    let Some(component) = component else {
                        skip(
                            registered_type_name(&types, type_uuid),
                            DropReason::Unregistered,
                        );
                        keep_base(&mut next_base);
                        continue;
                    };
                    match component.deserialize_remapped(data, &mapping) {
                        Ok(instance) => {
                            self.bind_instance(game_object, component, instance);
                            let enabled = prefab.data.is_component_enabled(*source, type_uuid);
                            self.set_instance_enabled(game_object, component, enabled);
                        }
                        Err(err) => {
                            skip(
                                Some(component.type_name().to_owned()),
                                DropReason::Invalid(err.to_string()),
                            );
                            keep_base(&mut next_base);
                            continue;
                        }
                    }
                }
                if let Some(hash) = self
                    .component_hashes(&registry, game_object)
                    .get(&type_uuid)
                {
                    next_base.insert(type_uuid, *hash);
                }
            }
            let link = PrefabLink {
                prefab: prefab_id,
                object: *source,
                instance,
                base: next_base,
                objects: if is_root {
                    objects.clone()
                } else {
                    BTreeSet::new()
                },
                removed: if is_root {
                    removed.clone()
                } else {
                    BTreeMap::new()
                },
            };
            let game_object_id = self.uuid(game_object);
            self.prefab_links.insert(game_object_id, link);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
