use calyx_engine::scene::{Scene, SceneSnapshot};
use serde_json::Value;
use uuid::Uuid;

use crate::scene_file::scene_fingerprint;

const DEFAULT_HISTORY_LIMIT: usize = 100;

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
        game_object: Uuid,
        component: Uuid,
        /// The values to write back, each at its path inside the component.
        values: Vec<(Vec<String>, Value)>,
    },
}

/// One value an inspector edit changed, at its path of object keys and array
/// indices inside the component's serialized form.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectorValueChange {
    pub value_path: Vec<String>,
    pub before: Value,
    pub after: Value,
}

/// One inspector edit of one component: every value a drag, a text-field
/// session or a discrete edit changed, undone and redone together.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectorValueEdit {
    pub label: String,
    pub game_object: Uuid,
    pub component: Uuid,
    pub changes: Vec<InspectorValueChange>,
}

impl InspectorValueEdit {
    fn restore(&self, after: bool) -> SceneHistoryRestore {
        SceneHistoryRestore::InspectorValue {
            label: self.label.clone(),
            game_object: self.game_object,
            component: self.component,
            values: self
                .changes
                .iter()
                .map(|change| {
                    let value = if after { &change.after } else { &change.before };
                    (change.value_path.clone(), value.clone())
                })
                .collect(),
        }
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
        if edit
            .changes
            .iter()
            .all(|change| change.before == change.after)
        {
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
            SceneHistoryEntry::InspectorValue(entry) => entry.restore(false),
        };
        self.redo.push(entry);
        Some(restore)
    }

    pub fn redo(&mut self) -> Option<SceneHistoryRestore> {
        let entry = self.redo.pop()?;
        let restore = match &entry {
            SceneHistoryEntry::Snapshot(entry) => entry.after.restore(&entry.label),
            SceneHistoryEntry::InspectorValue(entry) => entry.restore(true),
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
        let scene = calyx_engine::test_support::test_scene();
        let before = SceneEditSnapshot::capture(&scene);
        let after = SceneEditSnapshot::capture(&scene);
        let mut history = SceneHistory::default();

        assert!(!history.push("No-op", before, after));
        assert!(!history.can_undo());
    }

    #[test]
    fn undo_and_redo_move_entries_between_stacks() {
        let mut scene = calyx_engine::test_support::test_scene();
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
    fn inspector_value_history_restores_every_value_of_an_edit() {
        let game_object = Uuid::new_v4();
        let component = Uuid::new_v4();
        let mut history = SceneHistory::default();

        assert!(history.push_inspector_value(InspectorValueEdit {
            label: "Edit Transform rotation".into(),
            game_object,
            component,
            changes: vec![
                InspectorValueChange {
                    value_path: vec!["rotation".into(), "0".into()],
                    before: json!(0.0),
                    after: json!(0.5),
                },
                InspectorValueChange {
                    value_path: vec!["rotation".into(), "3".into()],
                    before: json!(1.0),
                    after: json!(0.8),
                },
            ],
        }));

        let Some(SceneHistoryRestore::InspectorValue { values, .. }) = history.undo() else {
            panic!("undo should restore the inspector edit");
        };
        assert_eq!(
            values,
            vec![
                (vec!["rotation".to_owned(), "0".to_owned()], json!(0.0)),
                (vec!["rotation".to_owned(), "3".to_owned()], json!(1.0)),
            ]
        );
        assert!(!history.can_undo());
        let Some(SceneHistoryRestore::InspectorValue { values, .. }) = history.redo() else {
            panic!("redo should restore the inspector edit");
        };
        assert_eq!(values[0].1, json!(0.5));
        assert_eq!(values[1].1, json!(0.8));
    }

    #[test]
    fn inspector_value_history_skips_an_edit_without_changes() {
        let mut history = SceneHistory::default();
        assert!(!history.push_inspector_value(InspectorValueEdit {
            label: "Edit Collider enabled".into(),
            game_object: Uuid::new_v4(),
            component: Uuid::new_v4(),
            changes: vec![InspectorValueChange {
                value_path: vec!["enabled".into()],
                before: json!(true),
                after: json!(true),
            }],
        }));
        assert!(!history.can_undo());
    }
}
