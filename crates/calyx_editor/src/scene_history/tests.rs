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
