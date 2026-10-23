//! Undo baselines for the components the inspector shows.
//!
//! The inspector keeps each shown component's serialized form as a baseline
//! and learns about edits from the widgets that make them, through
//! [`InspectorResponse`]. It serializes a component only to take a baseline,
//! when the object is selected or its baselines were refreshed, and to commit
//! an edit; a frame without a reported edit serializes nothing. A commit diffs
//! the baseline against the component and becomes one undo step: once per
//! drag or text-field focus, once per discrete edit.

use std::collections::{BTreeSet, HashMap};

use engine::class_registry::ComponentRegistry;
use engine::component::Component;
use engine::scene::{ComponentEdit, GameObject, Scene, SceneView};
use serde_json::Value;
use uuid::Uuid;

use super::type_inspector::InspectorResponse;
use crate::scene_history::InspectorValueChange;

/// One component of one game object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComponentKey {
    pub game_object: Uuid,
    pub component: Uuid,
}

/// The values one commit changed in one component.
#[derive(Debug, PartialEq)]
pub struct CommittedEdit {
    pub key: ComponentKey,
    pub changes: Vec<InspectorValueChange>,
}

struct Baseline {
    value: Option<Value>,
    /// An edit was reported since the baseline was taken.
    dirty: bool,
    /// A drag or text-field focus is in progress.
    dragging: bool,
    /// The scene's write version of the component when the baseline last
    /// matched it: when the baseline was taken, or when the inspector last
    /// wrote the component. `None` when unknown.
    version: Option<u64>,
}

/// One frame of a component shown through
/// [`InspectorEditTracker::show_component`].
pub struct ShownComponent<R> {
    /// What the draw closure returned.
    pub value: R,
    /// Whether the widgets changed the component this frame. The change may
    /// still be pending, its undo step waiting for a drag or text-field
    /// session to end, but the scene already holds it.
    pub changed: bool,
    /// Whether an edit completed this frame. It completes without a
    /// `committed` step when the interaction left the component as it was,
    /// such as a drag that returned to its start.
    pub completed: bool,
    /// The undo step this frame completed, if any.
    pub committed: Option<CommittedEdit>,
}

/// The selected object's component baselines and the edits pending on them.
#[derive(Default)]
pub struct InspectorEditTracker {
    object: Option<Uuid>,
    baselines: HashMap<ComponentKey, Baseline>,
    /// Turns off the unreported-edit check, whose serialization would
    /// otherwise hide the tracker's own in tests that count them.
    #[cfg(test)]
    pub(crate) skip_reported_check: bool,
}

impl InspectorEditTracker {
    /// The game object whose baselines are held.
    pub fn object(&self) -> Option<Uuid> {
        self.object
    }

    /// Makes `object` the tracked object and drops every baseline. The
    /// caller commits pending edits first.
    pub fn reset(&mut self, object: Option<Uuid>) {
        self.object = object;
        self.baselines.clear();
    }

    /// Drops every baseline, so each is taken again the next time its
    /// component is shown. The caller commits pending edits first.
    pub fn clear(&mut self) {
        self.baselines.clear();
    }

    /// Takes `value` as `key`'s baseline, with no edit pending.
    #[cfg(test)]
    pub fn set_baseline(&mut self, key: ComponentKey, value: Option<Value>) {
        self.set_baseline_at(key, value, None);
    }

    fn set_baseline_at(&mut self, key: ComponentKey, value: Option<Value>, version: Option<u64>) {
        self.baselines.insert(
            key,
            Baseline {
                value,
                dirty: false,
                dragging: false,
                version,
            },
        );
    }

    /// Whether `key`'s baseline must be taken (again) before the component
    /// is shown: it has none, or something other than the inspector wrote
    /// the component since, as gameplay, physics or an undo does. A baseline
    /// with an edit pending is kept, since its step must start where the
    /// edit did.
    fn needs_baseline(&self, key: ComponentKey, version: u64) -> bool {
        match self.baselines.get(&key) {
            None => true,
            Some(baseline) => {
                !baseline.dirty && baseline.version.is_some_and(|seen| seen != version)
            }
        }
    }

    /// Whether any component has an edit not yet committed.
    pub fn has_pending(&self) -> bool {
        self.baselines.values().any(|baseline| baseline.dirty)
    }

    /// The components with an edit not yet committed.
    pub fn pending(&self) -> Vec<ComponentKey> {
        self.baselines
            .iter()
            .filter(|(_, baseline)| baseline.dirty)
            .map(|(key, _)| *key)
            .collect()
    }

    /// Records what `key`'s widgets reported this frame and returns whether
    /// the edit is complete and should be committed now.
    ///
    /// A drag or text-field focus holds the commit until it stops. A
    /// discrete edit commits once the pointer is released, so a widget that
    /// changes its value while the pointer is held, such as a color picker,
    /// also records one step.
    pub fn report(
        &mut self,
        key: ComponentKey,
        response: InspectorResponse,
        pointer_down: bool,
    ) -> bool {
        let Some(baseline) = self.baselines.get_mut(&key) else {
            return false;
        };
        baseline.dirty |= response.changed;
        let mut commit = false;
        if response.drag_stopped {
            baseline.dragging = false;
            commit = baseline.dirty;
        }
        if response.drag_started {
            baseline.dragging = true;
        }
        commit || (baseline.dirty && !baseline.dragging && !pointer_down)
    }

    /// Commits `key`'s pending edit against `current`, the component as it
    /// is now, which becomes the new baseline. Returns the changed values,
    /// or `None` when nothing changed or either side cannot be serialized.
    pub fn commit(&mut self, key: ComponentKey, current: Option<Value>) -> Option<CommittedEdit> {
        let baseline = self.baselines.get_mut(&key)?;
        baseline.dirty = false;
        let before = std::mem::replace(&mut baseline.value, current);
        let (Some(before), Some(after)) = (before, baseline.value.as_ref()) else {
            return None;
        };
        let changes = changed_json_values(&before, after);
        (!changes.is_empty()).then_some(CommittedEdit { key, changes })
    }

    /// Shows `component` of `game_object` through `draw` and tracks the edit
    /// it reports. With `track` set, the component's baseline is taken when
    /// it has none or the scene saw an outside write to it since, and a
    /// completed edit is committed and returned. Without
    /// it, as in play mode, nothing is recorded. The scene is told about a
    /// write whenever `draw` reports a change.
    ///
    /// In debug builds and tests, a change `draw` made without reporting it
    /// panics; see [`assert_edit_reported`].
    ///
    /// Returns what `draw` returned, whether it changed the component and
    /// the committed edit, or `None` when the object or component is
    /// missing.
    ///
    /// As with [`Scene::edit_component_dyn`], `draw` reads the scene through
    /// a [`SceneView`] that cannot read the shown component's type.
    pub fn show_component<R>(
        &mut self,
        scene: &mut Scene,
        game_object: GameObject,
        component: &dyn Component,
        track: bool,
        pointer_down: bool,
        draw: impl FnOnce(&mut dyn Component, &SceneView) -> (InspectorResponse, R),
    ) -> Option<ShownComponent<R>> {
        let key = ComponentKey {
            game_object: scene.uuid(game_object),
            component: component.uuid(),
        };
        let version = scene.write_version(game_object, component);
        let needs_baseline = track && self.needs_baseline(key, version);
        #[cfg(any(debug_assertions, test))]
        let check_reported = self.checks_reported();
        let (value, changed, current): (R, bool, bool) =
            scene.edit_component_dyn(game_object, component, |instance, scene| {
                if needs_baseline {
                    self.set_baseline_at(key, instance.serialize(), Some(version));
                }
                #[cfg(any(debug_assertions, test))]
                let before = check_reported.then(|| instance.serialize());
                let (response, value) = draw(instance, scene);
                #[cfg(any(debug_assertions, test))]
                if let Some(before) = before {
                    assert_edit_reported(
                        component.type_name(),
                        &before,
                        &instance.serialize(),
                        response,
                    );
                }
                let commit = track && self.report(key, response, pointer_down);
                ComponentEdit {
                    value: (value, response.changed, commit),
                    written: response.changed,
                }
            })?;
        if changed {
            // The inspector's own write moved the version; the baseline
            // still holds every outside write before it.
            let version = scene.write_version(game_object, component);
            if let Some(baseline) = self.baselines.get_mut(&key) {
                baseline.version = Some(version);
            }
        }
        // Read after the write is reported: the scene may adjust the value,
        // as it puts back an object's id after a ComponentID write.
        let completed = current;
        let committed = current
            .then(|| {
                scene
                    .read_component_dyn(game_object, component, |c| c.serialize())
                    .flatten()
            })
            .and_then(|current| self.commit(key, current));
        Some(ShownComponent {
            value,
            changed,
            completed,
            committed,
        })
    }
}

impl InspectorEditTracker {
    #[cfg(any(debug_assertions, test))]
    fn checks_reported(&self) -> bool {
        #[cfg(test)]
        return !self.skip_reported_check;
        #[cfg(not(test))]
        return true;
    }
}

/// Serializes `key`'s component as it is in `scene`, or `None` when the
/// object, the component type or the component is missing.
pub fn serialize_component(
    scene: &Scene,
    registry: &ComponentRegistry,
    key: ComponentKey,
) -> Option<Value> {
    let game_object = scene.find(key.game_object)?;
    let component = registry.component(key.component)?;
    scene
        .read_component_dyn(game_object, component, |instance| instance.serialize())
        .flatten()
}

/// Whether a component changed between `before` and `after` without
/// `response` reporting it.
#[cfg(any(debug_assertions, test))]
pub fn is_unreported_edit(
    before: &Option<Value>,
    after: &Option<Value>,
    response: InspectorResponse,
) -> bool {
    !response.changed && before != after
}

/// Panics when a component changed between `before` and `after` without
/// `response` reporting it. An unreported edit is neither written to the
/// scene nor recorded for undo.
///
/// Compiled in debug builds and in this crate's tests: the repository's
/// Windows build flags turn debug assertions off even in dev builds, so tests
/// enable the check themselves.
#[cfg(any(debug_assertions, test))]
pub fn assert_edit_reported(
    type_name: &str,
    before: &Option<Value>,
    after: &Option<Value>,
    response: InspectorResponse,
) {
    assert!(
        !is_unreported_edit(before, after, response),
        "the inspector for {type_name} changed it without reporting the edit: {:?}",
        match (before, after) {
            (Some(before), Some(after)) => changed_json_values(before, after),
            _ => Vec::new(),
        }
    );
}

/// The leaf values that differ between `before` and `after`, each with its
/// path of object keys and array indices. A container whose shape changed
/// is reported whole.
pub fn changed_json_values(before: &Value, after: &Value) -> Vec<InspectorValueChange> {
    let mut changes = Vec::new();
    collect_json_value_changes(before, after, &mut Vec::new(), &mut changes);
    changes
}

fn collect_json_value_changes(
    before: &Value,
    after: &Value,
    path: &mut Vec<String>,
    changes: &mut Vec<InspectorValueChange>,
) {
    match (before, after) {
        (Value::Object(before), Value::Object(after))
            if before.len() == after.len() && before.keys().all(|key| after.contains_key(key)) =>
        {
            let keys = before.keys().cloned().collect::<BTreeSet<_>>();
            for key in keys {
                path.push(key.clone());
                collect_json_value_changes(&before[&key], &after[&key], path, changes);
                path.pop();
            }
        }
        (Value::Array(before), Value::Array(after)) if before.len() == after.len() => {
            for (index, (before, after)) in before.iter().zip(after).enumerate() {
                path.push(index.to_string());
                collect_json_value_changes(before, after, path, changes);
                path.pop();
            }
        }
        _ if before != after => changes.push(InspectorValueChange {
            value_path: path.clone(),
            before: before.clone(),
            after: after.clone(),
        }),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
