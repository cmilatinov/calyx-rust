//! Persistent per-view settings resolved directly from borrowed volume effects.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::assets::post_process_profile::PostProcessProfile;
use crate::component::{
    ComponentGlobalPostProcessVolume, ComponentLocalPostProcessVolume, ComponentTonemap,
    PostProcessEffect, ReflectPostProcessEffect,
};
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::ReflectDefault;
use crate::scene::{GameObject, Scene, SceneId};
use crate::utils::TypeUuid;
use legion::storage::ComponentTypeId;
use legion::{Entity, IntoQuery, Query};
use nalgebra_glm::Vec3;
use uuid::Uuid;

/// Storage for one effect, retained even during frames in which it is off.
pub struct EffectSlot {
    /// Whether a pass may read the effect this frame.
    pub active: bool,
    /// The same concrete component type registered under the slot's UUID.
    pub effect: Box<dyn PostProcessEffect>,
}

/// The post-processing settings of one view. Resolve into this same value each
/// frame: slots, volume queries and sorting storage are retained after warm-up.
/// An inactive or absent effect is `None`; tone mapping is always present.
pub struct PostProcessSettings {
    effects: HashMap<Uuid, EffectSlot>,
    scene_id: Option<SceneId>,
    global_query: Query<(Entity, &'static ComponentGlobalPostProcessVolume)>,
    local_query: Query<(Entity, &'static ComponentLocalPostProcessVolume)>,
    local_volumes: Vec<(i32, Uuid, (GameObject, f32))>,
    global_ties: HashSet<i32>,
    local_ties: HashSet<i32>,
}

impl Default for PostProcessSettings {
    fn default() -> Self {
        Self::empty().with(ComponentTonemap::default())
    }
}

impl PostProcessSettings {
    pub(crate) fn empty() -> Self {
        Self {
            effects: HashMap::new(),
            scene_id: None,
            global_query: <(Entity, &ComponentGlobalPostProcessVolume)>::query(),
            local_query: <(Entity, &ComponentLocalPostProcessVolume)>::query(),
            local_volumes: Vec::new(),
            global_ties: HashSet::new(),
            local_ties: HashSet::new(),
        }
    }

    /// Borrows an active effect by its component type.
    pub fn get<T: PostProcessEffect + TypeUuid>(&self) -> Option<&T> {
        self.effects
            .get(&T::type_uuid())
            .filter(|slot| slot.active)
            .and_then(|slot| slot.effect.as_any().downcast_ref())
    }

    /// Adds or replaces one effect, for constructing settings outside a scene.
    pub fn with(mut self, effect: impl PostProcessEffect) -> Self {
        self.effects.insert(
            effect.uuid(),
            EffectSlot {
                active: true,
                effect: Box::new(effect),
            },
        );
        self
    }

    /// Drops all effect values while their defining code is still loaded.
    /// Call before unloading a project assembly that registered effect types.
    pub fn clear(&mut self) {
        self.effects.clear();
        self.local_volumes.clear();
    }

    /// Resets active flags and resolves the selected global volume followed by
    /// local volumes in ascending priority/object-id order. No intermediate
    /// effect sets are cloned. The queries and contributing-volume buffer are
    /// warmed for all local volumes, even those currently outside the camera.
    pub fn resolve_into(&mut self, scene: &Scene, camera_position: &Vec3) {
        if self.scene_id != Some(scene.id()) {
            // Legion retains layout matches per world. Keep only this view's
            // current scene, rather than every play snapshot it ever rendered.
            self.global_query = <(Entity, &ComponentGlobalPostProcessVolume)>::query();
            self.local_query = <(Entity, &ComponentLocalPostProcessVolume)>::query();
            self.scene_id = Some(scene.id());
        }
        for slot in self.effects.values_mut() {
            slot.active = false;
        }
        let types = scene.registries().types.read();
        self.apply(&ComponentTonemap::default(), &types, None);
        drop(types);

        let selection = select_volume(self.global_query.iter(scene.world()).map(
            |(entity, volume)| {
                (
                    scene.is_type_enabled(
                        *entity,
                        ComponentTypeId::of::<ComponentGlobalPostProcessVolume>(),
                    ),
                    volume.priority,
                    *entity,
                )
            },
        ));
        if let Some(priority) = selection.tie {
            if self.global_ties.insert(priority) {
                warn_global_tie(priority);
            }
        }
        if let Some(volume) = selection
            .selected
            .and_then(|entity| scene.game_object_from_entity(entity))
        {
            self.apply_volume(scene, volume, None);
        }
        self.gather_local_volumes(scene, camera_position);
        for index in 0..self.local_volumes.len() {
            let (_, _, (volume, factor)) = self.local_volumes[index];
            self.apply_volume(scene, volume, Some(factor));
        }
    }

    /// Resolves a fresh settings value. Renderers use `resolve_into` to reuse it.
    pub fn resolve(scene: &Scene, camera_position: &Vec3) -> Self {
        let mut settings = Self::default();
        settings.resolve_into(scene, camera_position);
        settings
    }

    fn gather_local_volumes(&mut self, scene: &Scene, camera_position: &Vec3) {
        self.local_volumes.clear();
        // Reserve for every volume, so entering an already-known volume does
        // not grow the scratch buffer on a later frame.
        let count = self.local_query.iter(scene.world()).count();
        self.local_volumes.reserve(count);
        for (entity, volume) in self.local_query.iter(scene.world()) {
            if !scene.is_type_enabled(
                *entity,
                ComponentTypeId::of::<ComponentLocalPostProcessVolume>(),
            ) {
                continue;
            }
            let Some(object) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            let factor = volume.blend_factor(&scene.world_transform(object), camera_position);
            if factor > 0.0 {
                self.local_volumes
                    .push((volume.priority, scene.uuid(object), (object, factor)));
            }
        }
        if let Some(priority) = order_local_volumes(&mut self.local_volumes) {
            if self.local_ties.insert(priority) {
                crate::logging::warn_once(
                    format!("local-post-process-volume-tie:{priority}"),
                    || {
                        format!("Several local post-process volumes around the camera share priority {priority}; applying them in object id order")
                    },
                );
            }
        }
    }

    /// The contributing local volumes and their factors, in application order.
    /// This inspection helper allocates; frame resolution retains its buffer.
    pub fn local_volumes(scene: &Scene, camera_position: &Vec3) -> Vec<(GameObject, f32)> {
        let mut settings = Self::default();
        settings.gather_local_volumes(scene, camera_position);
        settings
            .local_volumes
            .iter()
            .map(|(_, _, item)| *item)
            .collect()
    }

    /// The enabled global volume with highest priority, first found on a tie.
    pub fn selected_volume(scene: &Scene) -> Option<GameObject> {
        let mut query = <(Entity, &ComponentGlobalPostProcessVolume)>::query();
        let selection = select_volume(query.iter(scene.world()).map(|(entity, volume)| {
            (
                scene.is_type_enabled(
                    *entity,
                    ComponentTypeId::of::<ComponentGlobalPostProcessVolume>(),
                ),
                volume.priority,
                *entity,
            )
        }));
        if let Some(priority) = selection.tie {
            warn_global_tie(priority);
        }
        selection
            .selected
            .and_then(|entity| scene.game_object_from_entity(entity))
    }

    /// Resolves a volume's profile and enabled component overrides on their own.
    pub fn from_game_object(scene: &Scene, game_object: GameObject) -> Self {
        let mut settings = Self::default();
        settings.apply_volume(scene, game_object, None);
        settings
    }

    fn apply_volume(&mut self, scene: &Scene, object: GameObject, factor: Option<f32>) {
        let profile = PostProcessProfile::of_volume(scene, object);
        let types = scene.registries().types.read();
        let components = scene.registries().components.read();
        if let Some(profile) = profile {
            let profile = profile.read();
            for effect in profile.enabled_effects() {
                // A component overrides a profile entry before any blending.
                // Applying both would incorrectly blend a local effect twice.
                let overridden = components
                    .component(effect.uuid())
                    .is_some_and(|prototype| {
                        scene.is_component_enabled_dyn(object, prototype)
                            && scene
                                .read_component_dyn(object, prototype, |_| ())
                                .is_some()
                    });
                if !overridden {
                    self.apply(effect, &types, factor);
                }
            }
        }
        for (id, prototype) in components.components() {
            let Some(meta) = types.trait_meta::<ReflectPostProcessEffect>(*id) else {
                continue;
            };
            if !scene.is_component_enabled_dyn(object, prototype.as_ref()) {
                continue;
            }
            scene.read_component_dyn(object, prototype.as_ref(), |component| {
                if let Some(effect) = meta.get(component.as_reflect()) {
                    self.apply(effect, &types, factor);
                }
            });
        }
    }

    fn apply(&mut self, source: &dyn PostProcessEffect, types: &TypeRegistry, factor: Option<f32>) {
        let slot = self.effects.entry(source.uuid()).or_insert_with(|| {
            let meta = types
                .trait_meta::<ReflectPostProcessEffect>(source.uuid())
                .expect("a registered effect has reflected effect metadata");
            let default = types
                .trait_meta::<ReflectDefault>(source.uuid())
                .expect("a registered effect component has a reflected default");
            EffectSlot {
                active: false,
                effect: meta
                    .get_boxed(default.default())
                    .expect("effect metadata and default have the same concrete type"),
            }
        });
        match factor {
            None => {
                slot.effect.copy_from(source);
                slot.active = true;
            }
            Some(factor) => {
                let factor = factor.clamp(0.0, 1.0);
                if !slot.active {
                    slot.effect.copy_from(source);
                    if !slot.effect.reset_neutral() {
                        slot.active = factor >= 0.5;
                        return;
                    }
                }
                slot.effect.blend_into(source, factor);
                slot.active = true;
            }
        }
    }
}

/// Sorts local volumes in place and returns the lowest tied priority.
pub(crate) fn order_local_volumes<T>(volumes: &mut [(i32, Uuid, T)]) -> Option<i32> {
    volumes.sort_unstable_by_key(|(priority, id, _)| (*priority, *id));
    volumes
        .windows(2)
        .find(|pair| pair[0].0 == pair[1].0)
        .map(|pair| pair[0].0)
}

impl fmt::Debug for PostProcessSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(
                self.effects
                    .iter()
                    .filter(|(_, slot)| slot.active)
                    .map(|(id, slot)| (id, slot.effect.serialize())),
            )
            .finish()
    }
}

impl PartialEq for PostProcessSettings {
    fn eq(&self, other: &Self) -> bool {
        let count = |settings: &Self| settings.effects.values().filter(|slot| slot.active).count();
        count(self) == count(other)
            && self
                .effects
                .iter()
                .filter(|(_, slot)| slot.active)
                .all(|(id, slot)| {
                    other.effects.get(id).is_some_and(|other| {
                        other.active && slot.effect.serialize() == other.effect.serialize()
                    })
                })
    }
}

fn warn_global_tie(priority: i32) {
    crate::logging::warn_once(format!("post-process-volume-tie:{priority}"), || {
        format!("Several enabled global post-process volumes share priority {priority}; using the first found")
    });
}

/// The item [`select_volume`] picked, and the priority several enabled
/// volumes tied at when it had to pick the first of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VolumeSelection<T> {
    pub selected: Option<T>,
    pub tie: Option<i32>,
}

/// Picks the enabled volume with the highest priority, or the first found
/// on a tie, from `(enabled, priority, item)` triples. `selected` is `None`
/// when no volume is enabled.
pub(crate) fn select_volume<T>(
    volumes: impl IntoIterator<Item = (bool, i32, T)>,
) -> VolumeSelection<T> {
    let mut selected: Option<(i32, T)> = None;
    let mut tie = None;
    let enabled = volumes
        .into_iter()
        .filter_map(|(enabled, priority, item)| enabled.then_some((priority, item)));
    for (priority, item) in enabled {
        match &selected {
            Some((best, _)) if priority < *best => {}
            Some((best, _)) if priority == *best => tie = Some(*best),
            _ => {
                selected = Some((priority, item));
                tie = None;
            }
        }
    }
    VolumeSelection {
        selected: selected.map(|(_, item)| item),
        tie,
    }
}

#[cfg(test)]
mod test_support;
