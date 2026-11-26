//! The objects a pass of component hooks visits, grouped by archetype so a
//! hook visits only the objects holding its component.

use std::sync::Arc;

use legion::storage::{ArchetypeIndex, ComponentTypeId, EntityLayout};
use legion::{Entity, IntoQuery};

use super::{GameObject, Scene};

/// One run of consecutive objects that shared an archetype when grouped.
struct Run {
    /// One past the run's last object.
    end: usize,
    /// The components every object of the run holds, or `None` for objects
    /// that no longer exist.
    layout: Option<Arc<EntityLayout>>,
}

/// Every non-root object of a scene when a pass of hooks starts, in world
/// order, grouped into runs of the same archetype.
///
/// A hook of one component type then skips each run whose archetype lacks
/// the type, instead of asking every object whether it holds the component.
/// When an object moves to another archetype during the pass, because a hook
/// added or removed a component, the runs are grouped again before the next
/// object is visited, so each object is visited exactly when it holds the
/// component at that moment, as when every object is asked. Objects spawned
/// during the pass are not part of it.
pub(crate) struct HookTargets {
    objects: Vec<GameObject>,
    runs: Vec<Run>,
    /// The scene's [`Scene::layout_changes`] the runs were grouped at.
    layout_changes: u64,
}

impl HookTargets {
    /// The non-root objects of `scene` now.
    pub fn new(scene: &Scene) -> Self {
        // Each archetype's objects are one run already, so grouping them
        // reads no object's entry.
        let mut objects = Vec::new();
        let mut runs: Vec<Run> = Vec::new();
        for chunk in <Entity>::query().iter_chunks(scene.world()) {
            let layout = chunk.archetype().layout().clone();
            objects.extend(
                chunk
                    .into_iter()
                    .filter_map(|entity| scene.game_object_from_entity(*entity))
                    .filter(|go| *go != scene.root()),
            );
            if runs.last().map_or(0, |run| run.end) < objects.len() {
                runs.push(Run {
                    end: objects.len(),
                    layout: Some(layout),
                });
            }
        }
        Self {
            objects,
            runs,
            layout_changes: scene.layout_changes(),
        }
    }

    /// Groups the objects into runs of their archetypes now.
    fn group(&mut self, scene: &Scene) {
        self.runs.clear();
        self.layout_changes = scene.layout_changes();
        let mut current: Option<Option<ArchetypeIndex>> = None;
        for (index, game_object) in self.objects.iter().enumerate() {
            let entry = scene.entry(*game_object);
            let archetype = entry.as_ref().map(|entry| entry.location().archetype());
            if current == Some(archetype) {
                if let Some(run) = self.runs.last_mut() {
                    run.end = index + 1;
                }
                continue;
            }
            current = Some(archetype);
            self.runs.push(Run {
                end: index + 1,
                layout: entry.map(|entry| entry.archetype().layout().clone()),
            });
        }
    }

    /// Calls `visit`, in order, on each object that holds a component of
    /// type `component` when its turn comes; on every object when
    /// `component` is `None`.
    pub fn visit_holding(
        &mut self,
        scene: &mut Scene,
        component: Option<ComponentTypeId>,
        mut visit: impl FnMut(&mut Scene, GameObject),
    ) {
        let Some(component) = component else {
            for &game_object in &self.objects {
                visit(scene, game_object);
            }
            return;
        };
        let mut index = 0;
        let mut run = 0;
        while index < self.objects.len() {
            if self.layout_changes != scene.layout_changes() {
                self.group(scene);
                run = self.runs.partition_point(|run| run.end <= index);
            }
            let Run { end, layout } = &self.runs[run];
            let end = *end;
            if !layout
                .as_ref()
                .is_some_and(|layout| layout.has_component_by_id(component))
            {
                index = end;
                run += 1;
                continue;
            }
            visit(scene, self.objects[index]);
            index += 1;
            if index == end {
                run += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests;
