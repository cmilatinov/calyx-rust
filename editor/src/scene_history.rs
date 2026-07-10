use engine::scene::{Scene, SceneSnapshot};

use crate::scene_document::scene_fingerprint;

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

pub struct SceneHistoryRestore {
    pub label: String,
    pub snapshot: SceneSnapshot,
    pub fingerprint: Option<String>,
}

struct SceneHistoryEntry {
    label: String,
    before: SceneEditSnapshot,
    after: SceneEditSnapshot,
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

        self.undo.push(SceneHistoryEntry {
            label: label.into(),
            before,
            after,
        });
        self.redo.clear();

        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }

        true
    }

    pub fn undo(&mut self) -> Option<SceneHistoryRestore> {
        let entry = self.undo.pop()?;
        let restore = entry.before.restore(&entry.label);
        self.redo.push(entry);
        Some(restore)
    }

    pub fn redo(&mut self) -> Option<SceneHistoryRestore> {
        let entry = self.redo.pop()?;
        let restore = entry.after.restore(&entry.label);
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
        SceneHistoryRestore {
            label: label.to_owned(),
            snapshot: self.snapshot.clone(),
            fingerprint: self.fingerprint.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(undo.label, "Add object");
        assert!(!history.can_undo());
        assert!(history.can_redo());

        let redo = history.redo().expect("redo entry");
        assert_eq!(redo.label, "Add object");
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }
}
