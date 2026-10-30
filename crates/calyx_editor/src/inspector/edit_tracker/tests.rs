use std::cell::Cell;

use calyx_engine::component::{
    Component, ComponentCollider, ComponentID, ComponentTransform, ReflectComponent,
};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::scene::{GameObject, GameObjectRef, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::accesskit::Role;
use egui::{Pos2, Ui, Vec2};
use nalgebra_glm::Vec3;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::*;
use crate::inspector::test_harness::InspectorHarness;
use crate::inspector::type_inspector::{InspectorContext, TypeInspector};
use crate::scene_history::{InspectorValueEdit, SceneHistory, SceneHistoryRestore};

const AWAY: Pos2 = Pos2::new(780.0, 580.0);

thread_local! {
    static SERIALIZATIONS: Cell<usize> = const { Cell::new(0) };
}

fn counted<S: serde::Serializer>(value: &f32, serializer: S) -> Result<S::Ok, S::Error> {
    SERIALIZATIONS.with(|count| count.set(count.get() + 1));
    serializer.serialize_f32(*value)
}

fn serializations() -> usize {
    SERIALIZATIONS.with(Cell::get)
}

/// Counts how often it is serialized.
#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "5d1c3f7e-2a8b-4e6f-9c0d-7b3a1e5f8c24"]
#[reflect(Default, TypeUuidDynamic, Component)]
struct CountedComponent {
    #[serde(serialize_with = "counted")]
    value: f32,
}

impl Component for CountedComponent {}

/// The scene and tracker of one test, kept outside the harness so the
/// tracker can write the scene.
struct Session {
    tracker: InspectorEditTracker,
    scene: Scene,
    game_object: GameObject,
    child: GameObject,
    commits: Vec<CommittedEdit>,
    /// Frames on which the tracker reported a change, committed or not.
    changed_frames: usize,
    /// Edits that completed, with or without an undo step.
    completed: usize,
    track: bool,
}

impl Session {
    fn new(h: &InspectorHarness) -> Self {
        let mut scene = h.registries.scene();
        let game_object = scene.create(None, None);
        let child = scene.create(None, Some(game_object));
        Self {
            tracker: InspectorEditTracker::default(),
            scene,
            game_object,
            child,
            commits: Vec::new(),
            changed_frames: 0,
            completed: 0,
            track: true,
        }
    }

    /// Draws `inspector` on the part of the component `project` picks,
    /// through the tracker, as the inspector panel does.
    fn draw<'s>(
        &'s mut self,
        component: &'s dyn Component,
        inspector: &'s dyn TypeInspector,
        project: fn(&mut dyn Component) -> &mut dyn Reflect,
    ) -> impl FnMut(&mut Ui, &InspectorContext) -> InspectorResponse + 's {
        move |ui: &mut Ui, harness_ctx: &InspectorContext| {
            let pointer_down = ui.input(|input| input.pointer.any_down());
            let game_object = self.game_object;
            let shown = self
                .tracker
                .show_component(
                    &mut self.scene,
                    game_object,
                    component,
                    self.track,
                    pointer_down,
                    |instance, scene| {
                        let ctx = InspectorContext {
                            scene,
                            game_object,
                            ..*harness_ctx
                        };
                        let response = inspector.show_inspector(ui, &ctx, project(instance));
                        (response, response)
                    },
                )
                .expect("the component should exist");
            if shown.changed {
                self.changed_frames += 1;
            }
            if shown.completed {
                self.completed += 1;
            }
            self.commits.extend(shown.committed);
            shown.value
        }
    }
}

fn whole(component: &mut dyn Component) -> &mut dyn Reflect {
    component.as_reflect_mut()
}

fn name(component: &mut dyn Component) -> &mut dyn Reflect {
    &mut component
        .as_reflect_mut()
        .downcast_mut::<ComponentID>()
        .expect("the component should be an id")
        .name
}

fn counted_value(component: &mut dyn Component) -> &mut dyn Reflect {
    &mut component
        .as_reflect_mut()
        .downcast_mut::<CountedComponent>()
        .expect("the component should be counted")
        .value
}

#[test]
fn a_drag_across_frames_records_one_undo_step() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = registry
        .type_inspector_lookup(ComponentTransform::type_uuid())
        .unwrap();
    let component = ComponentTransform::default();
    let mut session = Session::new(&h);
    let child_before = session.scene.world_transform(session.child).position;

    {
        let mut draw = session.draw(&component, inspector, whole);
        h.idle(3, &mut draw);
        let position_x = h.widget(Role::SpinButton, 0);
        let responses = h.drag(position_x, Vec2::new(40.0, 0.0), &mut draw);
        let changed_frames = responses.iter().filter(|r| r.changed).count();
        assert!(
            changed_frames > 1,
            "the drag should span frames: {responses:?}"
        );
        h.idle(3, &mut draw);
    }

    assert_eq!(session.commits.len(), 1, "{:?}", session.commits);
    let commit = session.commits.pop().unwrap();
    assert!(commit.changes.iter().all(|change| change
        .value_path
        .iter()
        .any(|segment| segment == "position")));
    // The scene saw the write: the child's cached world transform moved.
    assert_ne!(
        session.scene.world_transform(session.child).position,
        child_before
    );

    let mut history = SceneHistory::default();
    assert!(history.push_inspector_value(InspectorValueEdit {
        label: "Edit Transform".into(),
        game_object: commit.key.game_object,
        component: commit.key.component,
        changes: commit.changes,
    }));
    assert!(matches!(
        history.undo(),
        Some(SceneHistoryRestore::InspectorValue { .. })
    ));
    assert!(!history.can_undo());
}

#[test]
fn each_discrete_edit_records_one_undo_step() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = registry
        .type_inspector_lookup(ComponentCollider::type_uuid())
        .unwrap();
    let mut session = Session::new(&h);
    session
        .scene
        .bind_component_dyn(session.game_object, ComponentCollider::type_uuid());
    let component = ComponentCollider::default();

    {
        let mut draw = session.draw(&component, inspector, whole);
        h.idle(3, &mut draw);
        let enabled = h.widget(Role::CheckBox, 0);
        h.click(enabled, &mut draw);
        h.click(enabled, &mut draw);
    }

    assert_eq!(session.commits.len(), 2, "{:?}", session.commits);
    for commit in &session.commits {
        assert!(commit
            .changes
            .iter()
            .any(|change| change.value_path.last().map(String::as_str) == Some("enabled")));
    }
}

#[test]
fn typing_in_a_text_field_records_one_undo_step() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = registry.type_inspector_lookup(String::type_uuid()).unwrap();
    let component = ComponentID::default();
    let mut session = Session::new(&h);

    {
        let mut draw = session.draw(&component, inspector, name);
        h.idle(3, &mut draw);
        let field = h.widget(Role::TextInput, 0);
        h.click(field, &mut draw);
        h.type_text("x", &mut draw);
        h.type_text("y", &mut draw);
    }
    // While the field keeps focus the step is pending, but the tracker has
    // already reported each change, so the editor can mark the scene dirty.
    assert!(session.commits.is_empty());
    assert_eq!(session.changed_frames, 2);
    assert!(session.tracker.has_pending());
    {
        let mut draw = session.draw(&component, inspector, name);
        h.click(AWAY, &mut draw);
        h.idle(2, &mut draw);
    }

    assert_eq!(session.commits.len(), 1, "{:?}", session.commits);
    assert_eq!(session.commits[0].changes[0].value_path, vec!["name"]);
}

#[test]
fn a_drag_back_to_its_start_completes_without_an_undo_step() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = registry
        .type_inspector_lookup(ComponentTransform::type_uuid())
        .unwrap();
    let component = ComponentTransform::default();
    let mut session = Session::new(&h);

    {
        let mut draw = session.draw(&component, inspector, whole);
        h.idle(2, &mut draw);
        let position_x = h.widget(Role::SpinButton, 0);
        h.drag_through(
            position_x,
            &[Vec2::new(40.0, 0.0), Vec2::new(0.0, 0.0)],
            &mut draw,
        );
    }

    assert!(
        session.changed_frames > 0,
        "the drag should change the value"
    );
    assert!(session.commits.is_empty(), "{:?}", session.commits);
    // The editor reconciles the scene's dirty state when an edit completes,
    // so a no-op drag must still complete.
    assert_eq!(session.completed, 1);
    assert!(!session.tracker.has_pending());
}

#[test]
fn flushing_a_pending_session_keeps_a_later_outside_write_out_of_it() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = registry.type_inspector_lookup(String::type_uuid()).unwrap();
    let component = ComponentID::default();
    let mut session = Session::new(&h);
    let id_component = h
        .registries
        .components
        .read()
        .component(ComponentID::type_uuid())
        .map(|component| component.uuid())
        .unwrap();

    {
        let mut draw = session.draw(&component, inspector, name);
        h.idle(2, &mut draw);
        let field = h.widget(Role::TextInput, 0);
        h.click(field, &mut draw);
        h.type_text("x", &mut draw);
    }
    // The editor flushes pending edits on a pointer press, before another
    // panel writes the scene.
    let components = h.registries.components.read();
    for key in session.tracker.pending() {
        let current = serialize_component(&session.scene, &components, key);
        session.commits.extend(session.tracker.commit(key, current));
    }
    drop(components);
    let game_object = session.game_object;
    session
        .scene
        .write_component::<ComponentID, _>(game_object, |id| id.visible = !id.visible);
    {
        let mut draw = session.draw(&component, inspector, name);
        h.click(AWAY, &mut draw);
    }

    assert_eq!(session.commits.len(), 1, "{:?}", session.commits);
    assert_eq!(session.commits[0].key.component, id_component);
    assert!(session.commits[0]
        .changes
        .iter()
        .all(|change| change.value_path == ["name"]));
}

#[test]
fn frames_without_a_reported_edit_serialize_nothing() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = registry.type_inspector_lookup(f32::type_uuid()).unwrap();
    let mut session = Session::new(&h);
    session.tracker.skip_reported_check = true;
    session
        .scene
        .bind_component_dyn(session.game_object, CountedComponent::type_uuid());
    let component = CountedComponent::default();

    let mut draw = session.draw(&component, inspector, counted_value);
    let start = serializations();
    let _ = h.frame(&mut draw);
    assert_eq!(serializations() - start, 1, "selection takes the baseline");
    h.idle(5, &mut draw);
    assert_eq!(serializations() - start, 1, "idle frames serialize nothing");

    let field = h.widget(Role::SpinButton, 0);
    let responses = h.drag(field, Vec2::new(40.0, 0.0), &mut draw);
    assert!(responses.iter().filter(|r| r.changed).count() > 1);
    assert_eq!(
        serializations() - start,
        2,
        "the drag serializes once, to commit"
    );
    h.idle(5, &mut draw);
    assert_eq!(serializations() - start, 2);
    drop(draw);
    assert_eq!(session.commits.len(), 1);
}

#[test]
fn untracked_edits_write_the_scene_without_recording() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = registry
        .type_inspector_lookup(ComponentTransform::type_uuid())
        .unwrap();
    let component = ComponentTransform::default();
    let mut session = Session::new(&h);
    session.track = false;
    let child_before = session.scene.world_transform(session.child).position;

    {
        let mut draw = session.draw(&component, inspector, whole);
        h.idle(2, &mut draw);
        let position_x = h.widget(Role::SpinButton, 0);
        h.drag(position_x, Vec2::new(40.0, 0.0), &mut draw);
    }

    assert!(session.commits.is_empty());
    assert!(!session.tracker.has_pending());
    assert_ne!(
        session.scene.world_transform(session.child).position,
        child_before
    );
}

#[test]
fn a_refreshed_baseline_leaves_outside_writes_out_of_the_next_step() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = registry
        .type_inspector_lookup(ComponentTransform::type_uuid())
        .unwrap();
    let component = ComponentTransform::default();
    let mut session = Session::new(&h);

    {
        let mut draw = session.draw(&component, inspector, whole);
        h.idle(2, &mut draw);
    }
    let game_object = session.game_object;
    session
        .scene
        .write_component::<ComponentTransform, _>(game_object, |transform| {
            transform.transform.scale = Vec3::new(2.0, 2.0, 2.0);
        });
    session.tracker.clear();
    {
        let mut draw = session.draw(&component, inspector, whole);
        h.idle(1, &mut draw);
        let position_x = h.widget(Role::SpinButton, 0);
        h.drag(position_x, Vec2::new(40.0, 0.0), &mut draw);
    }

    assert_eq!(session.commits.len(), 1);
    assert!(session.commits[0]
        .changes
        .iter()
        .all(|change| !change.value_path.iter().any(|segment| segment == "scale")));
}

#[test]
fn an_outside_write_moves_the_baseline_before_the_next_step() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = registry
        .type_inspector_lookup(ComponentTransform::type_uuid())
        .unwrap();
    let component = ComponentTransform::default();
    let mut session = Session::new(&h);

    {
        let mut draw = session.draw(&component, inspector, whole);
        h.idle(2, &mut draw);
    }
    // Gameplay, physics or an undo writes the component; the inspector is
    // not told, but the scene's write version moves.
    let game_object = session.game_object;
    session
        .scene
        .write_component::<ComponentTransform, _>(game_object, |transform| {
            transform.transform.position.x = 5.0;
        });
    {
        let mut draw = session.draw(&component, inspector, whole);
        h.idle(1, &mut draw);
        let position_x = h.widget(Role::SpinButton, 0);
        h.drag(position_x, Vec2::new(40.0, 0.0), &mut draw);
    }

    assert_eq!(session.commits.len(), 1, "{:?}", session.commits);
    let change = &session.commits[0].changes[0];
    assert_eq!(change.value_path.last().map(String::as_str), Some("0"));
    assert_eq!(change.before, json!(5.0), "{change:?}");
    assert_ne!(change.after, json!(5.0));
}

#[test]
fn the_inspectors_own_writes_keep_its_baseline() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = registry
        .type_inspector_lookup(ComponentTransform::type_uuid())
        .unwrap();
    let component = ComponentTransform::default();
    let mut session = Session::new(&h);

    {
        let mut draw = session.draw(&component, inspector, whole);
        h.idle(1, &mut draw);
        let position_x = h.widget(Role::SpinButton, 0);
        h.drag(position_x, Vec2::new(40.0, 0.0), &mut draw);
        h.drag(position_x, Vec2::new(40.0, 0.0), &mut draw);
    }

    // The second step starts where the first ended, so undoing both
    // returns to the start.
    assert_eq!(session.commits.len(), 2, "{:?}", session.commits);
    assert_eq!(session.commits[0].changes[0].before, json!(0.0));
    assert_eq!(
        session.commits[1].changes[0].before,
        session.commits[0].changes[0].after
    );
}

#[test]
fn a_committed_id_edit_shows_the_id_the_scene_kept() {
    let h = InspectorHarness::new();
    let mut session = Session::new(&h);
    let game_object = session.game_object;
    let id = session.scene.uuid(game_object);
    let component = ComponentID::default();
    let shown = session
        .tracker
        .show_component(
            &mut session.scene,
            game_object,
            &component,
            true,
            false,
            |instance, _| {
                let c = instance
                    .as_reflect_mut()
                    .downcast_mut::<ComponentID>()
                    .unwrap();
                c.name = "Renamed".into();
                c.id = Uuid::new_v4();
                (InspectorResponse::CHANGED, ())
            },
        )
        .unwrap();

    let commit = shown.committed.expect("the rename should commit");
    assert_eq!(session.scene.uuid(game_object), id);
    assert!(
        commit
            .changes
            .iter()
            .all(|change| change.value_path == ["name"]),
        "{:?}",
        commit.changes
    );
}

#[test]
fn a_game_object_picker_shown_while_editing_an_id_lists_objects_by_id() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let picker = registry
        .type_inspector_lookup(GameObjectRef::type_uuid())
        .unwrap();
    let mut session = Session::new(&h);
    let game_object = session.game_object;
    let child_id = session.scene.uuid(session.child);
    let component = ComponentID::default();
    let mut value = GameObjectRef::default();

    // The picker reads the scene while the object's ComponentID is edited,
    // so it cannot read names and lists each object by its id.
    let mut draw = |ui: &mut Ui, harness_ctx: &InspectorContext| {
        session
            .tracker
            .show_component(
                &mut session.scene,
                game_object,
                &component,
                true,
                false,
                |_, scene| {
                    let ctx = InspectorContext {
                        scene,
                        game_object,
                        ..*harness_ctx
                    };
                    let response = picker.show_inspector(ui, &ctx, &mut value);
                    (response, response)
                },
            )
            .expect("every object has an id")
            .value
    };
    h.idle(2, &mut draw);
    let combo = h.widget(Role::ComboBox, 0);
    h.click(combo, &mut draw);
    let option = h.labelled(Role::Button, &child_id.to_string());
    h.click(option, &mut draw);

    assert_eq!(value.id(), child_id);
}

#[test]
#[should_panic(expected = "without reporting the edit")]
fn an_unreported_edit_fails() {
    let h = InspectorHarness::new();
    let mut session = Session::new(&h);
    let game_object = session.game_object;
    let component = ComponentTransform::default();
    let _ = session.tracker.show_component(
        &mut session.scene,
        game_object,
        &component,
        true,
        false,
        |instance, _| {
            instance
                .as_reflect_mut()
                .downcast_mut::<ComponentTransform>()
                .unwrap()
                .transform
                .position
                .x = 3.0;
            (InspectorResponse::NONE, ())
        },
    );
}

#[test]
fn unreported_edits_are_told_from_reported_ones() {
    let before = Some(json!({ "x": 1.0 }));
    let after = Some(json!({ "x": 2.0 }));
    assert!(is_unreported_edit(&before, &after, InspectorResponse::NONE));
    assert!(!is_unreported_edit(
        &before,
        &after,
        InspectorResponse::CHANGED
    ));
    assert!(!is_unreported_edit(
        &before,
        &before,
        InspectorResponse::NONE
    ));
}

fn tracked(value: Value) -> (InspectorEditTracker, ComponentKey) {
    let key = ComponentKey {
        game_object: Uuid::new_v4(),
        component: Uuid::new_v4(),
    };
    let mut tracker = InspectorEditTracker::default();
    tracker.set_baseline(key, Some(value));
    (tracker, key)
}

const fn response(changed: bool, drag_started: bool, drag_stopped: bool) -> InspectorResponse {
    InspectorResponse {
        changed,
        drag_started,
        drag_stopped,
    }
}

#[test]
fn a_drag_commits_when_it_stops() {
    let (mut tracker, key) = tracked(json!(0));
    assert!(!tracker.report(key, response(true, true, false), true));
    assert!(!tracker.report(key, response(true, false, false), true));
    assert!(!tracker.report(key, InspectorResponse::NONE, false));
    assert!(tracker.has_pending());
    assert!(tracker.report(key, response(false, false, true), false));
}

#[test]
fn a_discrete_edit_commits_once_the_pointer_is_released() {
    let (mut tracker, key) = tracked(json!(0));
    assert!(!tracker.report(key, InspectorResponse::CHANGED, true));
    assert!(!tracker.report(key, InspectorResponse::CHANGED, true));
    assert!(tracker.report(key, InspectorResponse::NONE, false));

    let (mut tracker, key) = tracked(json!(0));
    assert!(tracker.report(key, InspectorResponse::CHANGED, false));
}

#[test]
fn focus_moving_between_fields_commits_the_first() {
    let (mut tracker, key) = tracked(json!(0));
    assert!(!tracker.report(key, response(true, true, false), false));
    assert!(tracker.report(key, response(false, true, true), false));
    assert_eq!(
        tracker
            .commit(key, Some(json!(1)))
            .map(|edit| edit.changes.len()),
        Some(1)
    );
    // The second field's session is still open.
    assert!(!tracker.report(key, InspectorResponse::CHANGED, false));
}

#[test]
fn a_commit_moves_the_baseline() {
    let (mut tracker, key) = tracked(json!({ "a": 1, "b": [1, 2] }));
    let edit = tracker
        .commit(key, Some(json!({ "a": 1, "b": [1, 3] })))
        .expect("a value changed");
    assert_eq!(edit.changes.len(), 1);
    assert_eq!(edit.changes[0].value_path, vec!["b", "1"]);
    assert_eq!(edit.changes[0].before, json!(2));
    assert_eq!(edit.changes[0].after, json!(3));
    assert!(tracker
        .commit(key, Some(json!({ "a": 1, "b": [1, 3] })))
        .is_none());
}

#[test]
fn json_value_changes_use_stable_nested_paths() {
    let before = json!({
        "transform": {
            "position": [0.0, 0.0, 0.0],
            "scale": [1.0, 1.0, 1.0]
        }
    });
    let after = json!({
        "transform": {
            "position": [2.0, 0.0, 0.0],
            "scale": [1.0, 3.0, 1.0]
        }
    });

    let changes = changed_json_values(&before, &after);

    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].value_path, vec!["transform", "position", "0"]);
    assert_eq!(changes[0].before, json!(0.0));
    assert_eq!(changes[0].after, json!(2.0));
    assert_eq!(changes[1].value_path, vec!["transform", "scale", "1"]);
}
