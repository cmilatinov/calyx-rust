use std::fs;
use std::path::{Path, PathBuf};

use calyx_engine::component::{
    ComponentCamera, ComponentID, ComponentMesh, ComponentPointLight, ComponentRigidBody,
    ComponentTransform,
};
use calyx_engine::reflect::type_registry::TypeRegistry;
use calyx_engine::utils::TypeUuid;
use calyx_project::{PinnedComponent, Project};
use uuid::Uuid;

use super::{PinEdit, PinState, PinnedComponents};

const MANIFEST: &str = "name = \"Pins\"\n\
    creation_date = \"2026-01-01T00:00:00Z\"\n\
    startup_scene = \"assets/scene.cxscene\"\n";

/// A project directory whose `project.toml` is `MANIFEST` plus `extra`.
fn project_dir(test: &str, extra: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "calyx-pinned-components-{test}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("project.toml"), format!("{MANIFEST}{extra}")).unwrap();
    dir
}

fn pins_manifest(entries: &[(Uuid, &str)]) -> String {
    let mut manifest = String::from("[inspector]\npinned = [\n");
    for (uuid, name) in entries {
        manifest.push_str(&format!("  {{ uuid = \"{uuid}\", name = \"{name}\" }},\n"));
    }
    manifest.push_str("]\n");
    manifest
}

/// The pinned list `project.toml` holds now.
fn saved(dir: &Path) -> Option<Vec<(Uuid, String)>> {
    Project::load(dir.to_path_buf())
        .unwrap()
        .inspector()
        .pinned
        .clone()
        .map(|entries| {
            entries
                .into_iter()
                .map(|PinnedComponent { uuid, name }| (uuid, name))
                .collect()
        })
}

fn named(uuids: &[(Uuid, &str)]) -> Vec<(Uuid, String)> {
    uuids
        .iter()
        .map(|(uuid, name)| (*uuid, name.to_string()))
        .collect()
}

/// The components of a typical object, in no particular order.
fn components() -> Vec<(Uuid, String)> {
    named(&[
        (ComponentRigidBody::type_uuid(), "Rigid Body"),
        (ComponentTransform::type_uuid(), "Transform"),
        (ComponentMesh::type_uuid(), "Mesh Renderer"),
        (ComponentID::type_uuid(), "Game Object"),
        (ComponentCamera::type_uuid(), "Camera"),
    ])
}

#[test]
fn defaults_pin_game_object_then_transform() {
    let dir = project_dir("defaults", "");
    let pins = PinnedComponents::load(&dir);

    assert_eq!(
        pins.order(components()),
        vec![
            ComponentID::type_uuid(),
            ComponentTransform::type_uuid(),
            ComponentCamera::type_uuid(),
            ComponentMesh::type_uuid(),
            ComponentRigidBody::type_uuid(),
        ]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pinned_components_come_first_in_list_order_then_by_name() {
    let dir = project_dir(
        "list-order",
        &pins_manifest(&[
            (ComponentRigidBody::type_uuid(), "Rigid Body"),
            (ComponentTransform::type_uuid(), "Transform"),
        ]),
    );
    let pins = PinnedComponents::load(&dir);

    assert_eq!(
        pins.order(components()),
        vec![
            ComponentRigidBody::type_uuid(),
            ComponentTransform::type_uuid(),
            ComponentCamera::type_uuid(),
            ComponentID::type_uuid(),
            ComponentMesh::type_uuid(),
        ]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unknown_entries_are_skipped() {
    let unknown = Uuid::from_u128(0x5eed);
    let dir = project_dir(
        "unknown",
        &pins_manifest(&[
            (unknown, "Removed Component"),
            (ComponentMesh::type_uuid(), "Mesh Renderer"),
        ]),
    );
    let pins = PinnedComponents::load(&dir);

    let order = pins.order(components());

    assert_eq!(order[0], ComponentMesh::type_uuid());
    assert!(!order.contains(&unknown));
    assert_eq!(order.len(), components().len());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn empty_list_pins_nothing() {
    let dir = project_dir("empty", "[inspector]\npinned = []\n");
    let pins = PinnedComponents::load(&dir);

    assert_eq!(
        pins.order(components()),
        vec![
            ComponentCamera::type_uuid(),
            ComponentID::type_uuid(),
            ComponentMesh::type_uuid(),
            ComponentRigidBody::type_uuid(),
            ComponentTransform::type_uuid(),
        ]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn only_pinned_components_move() {
    let dir = project_dir("states", "");
    let pins = PinnedComponents::load(&dir);
    let shown = pins.order(components());

    let states = pins.states(&shown);

    let pinned = |up, down| PinState {
        pinned: true,
        can_move_up: up,
        can_move_down: down,
    };
    assert_eq!(
        states,
        vec![
            pinned(false, true),
            pinned(true, false),
            PinState::default(),
            PinState::default(),
            PinState::default(),
        ]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn first_pin_while_defaults_apply_writes_the_full_list() {
    let dir = project_dir("pin", "");
    let registry = TypeRegistry::new();
    let mut pins = PinnedComponents::load(&dir);
    let shown = pins.order(components());

    pins.edit(
        &registry,
        ComponentCamera::type_uuid(),
        PinEdit::Pin,
        &shown,
    );

    assert_eq!(
        saved(&dir),
        Some(named(&[
            (ComponentID::type_uuid(), "Game Object"),
            (ComponentTransform::type_uuid(), "Transform"),
            (ComponentCamera::type_uuid(), "Camera"),
        ]))
    );
    assert_eq!(pins.order(components())[2], ComponentCamera::type_uuid());
    let manifest = fs::read_to_string(dir.join("project.toml")).unwrap();
    assert!(manifest.contains("startup_scene"), "{manifest}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unpin_writes_the_list_without_it() {
    let dir = project_dir("unpin", "");
    let registry = TypeRegistry::new();
    let mut pins = PinnedComponents::load(&dir);
    let shown = pins.order(components());

    pins.edit(&registry, ComponentID::type_uuid(), PinEdit::Unpin, &shown);

    assert_eq!(
        saved(&dir),
        Some(named(&[(ComponentTransform::type_uuid(), "Transform")]))
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn moves_write_the_new_order_and_keep_unknown_entries() {
    let unknown = Uuid::from_u128(0x5eed);
    let dir = project_dir(
        "move",
        &pins_manifest(&[
            (ComponentTransform::type_uuid(), "Old Name"),
            (unknown, "Removed Component"),
            (ComponentCamera::type_uuid(), "Camera"),
            (ComponentPointLight::type_uuid(), "Point Light"),
        ]),
    );
    let registry = TypeRegistry::new();
    let mut pins = PinnedComponents::load(&dir);
    // The object has no point light, so only Transform and Camera show.
    let shown = pins.order(components());

    pins.edit(
        &registry,
        ComponentCamera::type_uuid(),
        PinEdit::MoveUp,
        &shown,
    );

    assert_eq!(
        saved(&dir),
        Some(named(&[
            (ComponentCamera::type_uuid(), "Camera"),
            (ComponentTransform::type_uuid(), "Transform"),
            (unknown, "Removed Component"),
            (ComponentPointLight::type_uuid(), "Point Light"),
        ]))
    );

    let shown = pins.order(components());
    pins.edit(
        &registry,
        ComponentCamera::type_uuid(),
        PinEdit::MoveDown,
        &shown,
    );

    assert_eq!(
        saved(&dir),
        Some(named(&[
            (ComponentTransform::type_uuid(), "Transform"),
            (ComponentCamera::type_uuid(), "Camera"),
            (unknown, "Removed Component"),
            (ComponentPointLight::type_uuid(), "Point Light"),
        ]))
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn edits_drop_duplicate_entries() {
    let dir = project_dir(
        "duplicates",
        &pins_manifest(&[
            (ComponentCamera::type_uuid(), "Camera"),
            (ComponentCamera::type_uuid(), "Camera"),
            (ComponentMesh::type_uuid(), "Mesh Renderer"),
        ]),
    );
    let registry = TypeRegistry::new();
    let mut pins = PinnedComponents::load(&dir);
    let shown = pins.order(components());

    pins.edit(
        &registry,
        ComponentCamera::type_uuid(),
        PinEdit::MoveDown,
        &shown,
    );

    assert_eq!(
        saved(&dir),
        Some(named(&[
            (ComponentMesh::type_uuid(), "Mesh Renderer"),
            (ComponentCamera::type_uuid(), "Camera"),
        ]))
    );
    assert_eq!(
        &pins.order(components())[..2],
        &[ComponentMesh::type_uuid(), ComponentCamera::type_uuid()]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_save_keeps_the_new_order() {
    let dir = project_dir("failed-save", "");
    let registry = TypeRegistry::new();
    let mut pins = PinnedComponents::load(&dir);
    fs::remove_file(dir.join("project.toml")).unwrap();
    let shown = pins.order(components());

    pins.edit(
        &registry,
        ComponentID::type_uuid(),
        PinEdit::MoveDown,
        &shown,
    );

    assert_eq!(
        &pins.order(components())[..2],
        &[ComponentTransform::type_uuid(), ComponentID::type_uuid()]
    );
    assert!(!dir.join("project.toml").exists());
    fs::remove_dir_all(dir).unwrap();
}
