use std::collections::HashMap;
use std::time::{Duration, Instant};

use engine::scene::{Scene, SceneSnapshot};
use serde_json::Value;
use uuid::Uuid;

use crate::scene_document::scene_fingerprint;

const DEFAULT_HISTORY_LIMIT: usize = 100;
pub const INSPECTOR_VALUE_DEBOUNCE: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub struct SceneEditSnapshot {
    snapshot: SceneSnapshot,
    fingerprint: Option<String>,
}

impl SceneEditSnapshot {
    pub fn capture(scene: &Scene) -> Self {
        Self {
            snapshot: scene.snapshot(),
            fingerprint: scene_fingerprint(scene),
        }
    }

    pub fn fingerprint(&self) -> Option<&str> {
        self.fingerprint.as_deref()
    }
}

pub enum SceneHistoryRestore {
    Scene {
        label: String,
        snapshot: SceneSnapshot,
        fingerprint: Option<String>,
    },
    InspectorValue {
        label: String,
        key: InspectorValueKey,
        value: Value,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InspectorValueKey {
    pub game_object: Uuid,
    pub component: Uuid,
    pub value_path: Vec<String>,
}

#[derive(Clone)]
pub struct InspectorValueEdit {
    pub label: String,
    pub key: InspectorValueKey,
    pub before: Value,
    pub after: Value,
}

#[derive(Default)]
pub struct InspectorValueDebouncer {
    pending: HashMap<InspectorValueKey, PendingInspectorValueEdit>,
    next_sequence: u64,
}

struct PendingInspectorValueEdit {
    edit: InspectorValueEdit,
    last_change: Instant,
    sequence: u64,
}

impl InspectorValueDebouncer {
    pub fn record(&mut self, edit: InspectorValueEdit, now: Instant) {
        if edit.before == edit.after {
            return;
        }

        self.next_sequence = self.next_sequence.saturating_add(1);
        let sequence = self.next_sequence;
        let key = edit.key.clone();
        let remove = if let Some(pending) = self.pending.get_mut(&key) {
            pending.edit.after = edit.after;
            pending.last_change = now;
            pending.sequence = sequence;
            pending.edit.before == pending.edit.after
        } else {
            self.pending.insert(
                key.clone(),
                PendingInspectorValueEdit {
                    edit,
                    last_change: now,
                    sequence,
                },
            );
            false
        };
        if remove {
            self.pending.remove(&key);
        }
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    pub fn drain_due(&mut self, now: Instant) -> Vec<InspectorValueEdit> {
        self.drain_matching(|pending| {
            now.duration_since(pending.last_change) >= INSPECTOR_VALUE_DEBOUNCE
        })
    }

    pub fn drain_all(&mut self) -> Vec<InspectorValueEdit> {
        self.drain_matching(|_| true)
    }

    fn drain_matching(
        &mut self,
        predicate: impl Fn(&PendingInspectorValueEdit) -> bool,
    ) -> Vec<InspectorValueEdit> {
        let pending = std::mem::take(&mut self.pending);
        let (mut ready, waiting): (Vec<_>, Vec<_>) = pending
            .into_values()
            .partition(|pending| predicate(pending));
        self.pending = waiting
            .into_iter()
            .map(|pending| (pending.edit.key.clone(), pending))
            .collect();
        ready.sort_by_key(|pending| (pending.last_change, pending.sequence));
        ready.into_iter().map(|pending| pending.edit).collect()
    }
}

struct SceneSnapshotHistoryEntry {
    label: String,
    before: SceneEditSnapshot,
    after: SceneEditSnapshot,
}

enum SceneHistoryEntry {
    Snapshot(SceneSnapshotHistoryEntry),
    InspectorValue(InspectorValueEdit),
}

pub struct SceneHistory {
    undo: Vec<SceneHistoryEntry>,
    redo: Vec<SceneHistoryEntry>,
    limit: usize,
}

impl Default for SceneHistory {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: DEFAULT_HISTORY_LIMIT,
        }
    }
}

impl SceneHistory {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn push(
        &mut self,
        label: impl Into<String>,
        before: SceneEditSnapshot,
        after: SceneEditSnapshot,
    ) -> bool {
        if before.matches(&after) {
            return false;
        }

        self.undo
            .push(SceneHistoryEntry::Snapshot(SceneSnapshotHistoryEntry {
                label: label.into(),
                before,
                after,
            }));
        self.redo.clear();

        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }

        true
    }

    pub fn push_inspector_value(&mut self, edit: InspectorValueEdit) -> bool {
        if edit.before == edit.after {
            return false;
        }

        self.undo.push(SceneHistoryEntry::InspectorValue(edit));
        self.redo.clear();

        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }

        true
    }

    pub fn undo(&mut self) -> Option<SceneHistoryRestore> {
        let entry = self.undo.pop()?;
        let restore = match &entry {
            SceneHistoryEntry::Snapshot(entry) => entry.before.restore(&entry.label),
            SceneHistoryEntry::InspectorValue(entry) => SceneHistoryRestore::InspectorValue {
                label: entry.label.clone(),
                key: entry.key.clone(),
                value: entry.before.clone(),
            },
        };
        self.redo.push(entry);
        Some(restore)
    }

    pub fn redo(&mut self) -> Option<SceneHistoryRestore> {
        let entry = self.redo.pop()?;
        let restore = match &entry {
            SceneHistoryEntry::Snapshot(entry) => entry.after.restore(&entry.label),
            SceneHistoryEntry::InspectorValue(entry) => SceneHistoryRestore::InspectorValue {
                label: entry.label.clone(),
                key: entry.key.clone(),
                value: entry.after.clone(),
            },
        };
        self.undo.push(entry);
        Some(restore)
    }
}

impl SceneEditSnapshot {
    fn matches(&self, other: &Self) -> bool {
        match (self.fingerprint(), other.fingerprint()) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }

    fn restore(&self, label: &str) -> SceneHistoryRestore {
        SceneHistoryRestore::Scene {
            label: label.to_owned(),
            snapshot: self.snapshot.clone(),
            fingerprint: self.fingerprint.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn history_skips_unchanged_scene_snapshots() {
        let scene = engine::test_support::test_scene();
        let before = SceneEditSnapshot::capture(&scene);
        let after = SceneEditSnapshot::capture(&scene);
        let mut history = SceneHistory::default();

        assert!(!history.push("No-op", before, after));
        assert!(!history.can_undo());
    }

    #[test]
    fn undo_and_redo_move_entries_between_stacks() {
        let mut scene = engine::test_support::test_scene();
        let before = SceneEditSnapshot::capture(&scene);
        scene.create(None, None);
        let after = SceneEditSnapshot::capture(&scene);
        let mut history = SceneHistory::default();

        assert!(history.push("Add object", before, after));
        assert!(history.can_undo());
        assert!(!history.can_redo());

        let undo = history.undo().expect("undo entry");
        assert!(matches!(
            undo,
            SceneHistoryRestore::Scene { ref label, .. } if label == "Add object"
        ));
        assert!(!history.can_undo());
        assert!(history.can_redo());

        let redo = history.redo().expect("redo entry");
        assert!(matches!(
            redo,
            SceneHistoryRestore::Scene { ref label, .. } if label == "Add object"
        ));
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn inspector_value_debouncer_coalesces_only_matching_value_keys() {
        let object = Uuid::new_v4();
        let component = Uuid::new_v4();
        let position_x = InspectorValueKey {
            game_object: object,
            component,
            value_path: vec!["position".into(), "0".into()],
        };
        let position_y = InspectorValueKey {
            game_object: object,
            component,
            value_path: vec!["position".into(), "1".into()],
        };
        let now = Instant::now();
        let mut debouncer = InspectorValueDebouncer::default();

        debouncer.record(
            InspectorValueEdit {
                label: "Edit Transform position.0".into(),
                key: position_x.clone(),
                before: json!(0.0),
                after: json!(1.0),
            },
            now,
        );
        debouncer.record(
            InspectorValueEdit {
                label: "Edit Transform position.0".into(),
                key: position_x.clone(),
                before: json!(1.0),
                after: json!(2.0),
            },
            now + Duration::from_millis(500),
        );
        debouncer.record(
            InspectorValueEdit {
                label: "Edit Transform position.1".into(),
                key: position_y.clone(),
                before: json!(0.0),
                after: json!(3.0),
            },
            now + Duration::from_millis(500),
        );

        assert!(debouncer
            .drain_due(now + Duration::from_millis(1499))
            .is_empty());
        let edits =
            debouncer.drain_due(now + INSPECTOR_VALUE_DEBOUNCE + Duration::from_millis(500));
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].key, position_x);
        assert_eq!(edits[0].before, json!(0.0));
        assert_eq!(edits[0].after, json!(2.0));
        assert_eq!(edits[1].key, position_y);
        assert_eq!(edits[1].before, json!(0.0));
        assert_eq!(edits[1].after, json!(3.0));
    }

    #[test]
    fn inspector_value_history_restores_each_value() {
        let key = InspectorValueKey {
            game_object: Uuid::new_v4(),
            component: Uuid::new_v4(),
            value_path: vec!["enabled".into()],
        };
        let mut history = SceneHistory::default();

        assert!(history.push_inspector_value(InspectorValueEdit {
            label: "Edit Collider enabled".into(),
            key: key.clone(),
            before: json!(true),
            after: json!(false),
        }));

        assert!(matches!(
            history.undo(),
            Some(SceneHistoryRestore::InspectorValue { key: restored, value, .. })
                if restored == key && value == json!(true)
        ));
        assert!(matches!(
            history.redo(),
            Some(SceneHistoryRestore::InspectorValue { key: restored, value, .. })
                if restored == key && value == json!(false)
        ));
    }
}
