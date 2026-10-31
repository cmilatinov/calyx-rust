//! Saved scenes, prefabs and `.meta` files name types by UUID, never by a
//! Rust path, so renaming or moving a crate leaves them loading unchanged.
//! These tests read every committed one and fail if a crate-qualified type
//! name appears in them, if a component they hold has no registered type, or
//! if a component that the scene now switches on and off still carries its
//! own `enabled` field, or if a post-process volume still holds the combined
//! settings its effect components replaced.

// Matches the crates under test: the particle system component nests deep
// enough that proving it `Send`/`Sync` passes the default limit.
#![recursion_limit = "256"]

use std::path::{Path, PathBuf};

use calyx_engine::component::{
    ComponentCamera, ComponentCollider, ComponentGlobalPostProcessVolume, ComponentRigidBody,
};
use calyx_engine::context::AssetContext;
use calyx_engine::render::postprocess::{
    AmbientOcclusionSettings, AntiAliasingSettings, BloomSettings, ColorGradingSettings,
    DepthOfFieldSettings, TonemapSettings, VignetteSettings,
};
use calyx_engine::scene::{Prefab, Scene};
use calyx_engine::test_support::TestAssets;
use calyx_engine::utils::{TypeUuid, Uuid};
use serde_json::Value;

/// The asset roots with saved data, relative to the repository root.
const ASSET_ROOTS: [&str; 2] = ["assets", "sandbox/assets"];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the sandbox sits in the repository root")
        .to_path_buf()
}

fn files_with_extensions(dir: &Path, extensions: &[&str], found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_with_extensions(&path, extensions, found);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extensions.contains(&extension))
        {
            found.push(path);
        }
    }
}

/// Every committed saved file with one of `extensions`, as its path relative
/// to its asset root, paired with that root's index in [`ASSET_ROOTS`].
fn saved_files(extensions: &[&str]) -> Vec<(usize, PathBuf)> {
    let root = repository_root();
    let mut saved = Vec::new();
    for (index, asset_root) in ASSET_ROOTS.iter().enumerate() {
        let asset_root = root.join(asset_root);
        let mut found = Vec::new();
        files_with_extensions(&asset_root, extensions, &mut found);
        saved.extend(
            found
                .into_iter()
                .map(|path| (index, path.strip_prefix(&asset_root).unwrap().to_path_buf())),
        );
    }
    assert!(
        !saved.is_empty(),
        "no saved {extensions:?} files were found"
    );
    saved
}

/// Whether `text` holds a Rust path such as `calyx_engine::component::X`:
/// a lowercase identifier, `::`, then another identifier.
fn crate_qualified_path(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut search = 0;
    while let Some(offset) = text[search..].find("::") {
        let at = search + offset;
        let start = text[..at]
            .bytes()
            .rposition(|byte| !ident(byte))
            .map_or(0, |index| index + 1);
        let next = bytes.get(at + 2).copied();
        if start < at
            && bytes[start].is_ascii_lowercase()
            && next.is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        {
            let end = text[at + 2..]
                .bytes()
                .position(|byte| !ident(byte) && byte != b':')
                .map_or(text.len(), |index| at + 2 + index);
            return Some(&text[start..end]);
        }
        search = at + 2;
    }
    None
}

#[test]
fn crate_qualified_path_finds_rust_paths_only() {
    assert_eq!(
        crate_qualified_path(r#"{"type": "calyx_engine::component::ComponentTransform"}"#),
        Some("calyx_engine::component::ComponentTransform")
    );
    assert_eq!(
        crate_qualified_path("engine::scene::Scene"),
        Some("engine::scene::Scene")
    );
    assert_eq!(crate_qualified_path(r#"{"time": "12::30"}"#), None);
    assert_eq!(crate_qualified_path("C::\\path"), None);
}

#[test]
fn saved_files_name_no_rust_paths() {
    let root = repository_root();
    for (index, path) in saved_files(&["cxscene", "cxprefab", "meta"]) {
        let file = root.join(ASSET_ROOTS[index]).join(&path);
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            crate_qualified_path(&text),
            None,
            "{} names a type by its Rust path, which breaks when its crate is \
             renamed; saved data must name types by UUID",
            file.display()
        );
    }
}

/// Every component type UUID in a saved scene's `components` map.
fn component_types(scene: &Value) -> Vec<Uuid> {
    let mut types = Vec::new();
    for components in scene["components"]
        .as_object()
        .expect("a saved scene has a components map")
        .values()
    {
        for type_uuid in components.as_object().unwrap().keys() {
            types.push(Uuid::parse_str(type_uuid).expect("components are keyed by type UUID"));
        }
    }
    types
}

/// The saved scene's scene data: a prefab file nests it under `data`.
fn scene_data<'a>(source: &'a Value, path: &Path) -> &'a Value {
    let is_prefab = path
        .extension()
        .is_some_and(|extension| extension == "cxprefab");
    if is_prefab {
        &source["data"]
    } else {
        source
    }
}

/// The `(object, component type)` pairs in a saved scene whose payload still
/// holds a top-level `enabled` field, for the component types that carried
/// one before the scene kept the enabled state. Only the payload's own field
/// counts; a nested one, such as a post-process effect's, is not the
/// component's.
fn legacy_enabled_fields(scene: &Value) -> Vec<(String, Uuid)> {
    let legacy = [
        ComponentCamera::type_uuid(),
        ComponentCollider::type_uuid(),
        ComponentRigidBody::type_uuid(),
        ComponentGlobalPostProcessVolume::type_uuid(),
        // Effect components, whose fields were the volume's sub-structs.
        TonemapSettings::type_uuid(),
        AmbientOcclusionSettings::type_uuid(),
        DepthOfFieldSettings::type_uuid(),
        BloomSettings::type_uuid(),
        ColorGradingSettings::type_uuid(),
        VignetteSettings::type_uuid(),
        AntiAliasingSettings::type_uuid(),
    ];
    let mut found = Vec::new();
    for (object, components) in scene["components"]
        .as_object()
        .expect("a saved scene has a components map")
    {
        for type_uuid in legacy {
            let payload = &components[type_uuid.to_string()];
            if payload.get("enabled").is_some() {
                found.push((object.clone(), type_uuid));
            }
        }
    }
    found
}

#[test]
fn legacy_enabled_fields_flags_only_the_payloads_own_field() {
    let object = "07b17e3d-9b73-411b-9d08-c56698a37556";
    let camera = ComponentCamera::type_uuid().to_string();
    let volume = ComponentGlobalPostProcessVolume::type_uuid().to_string();
    let mut scene = serde_json::json!({ "components": { object: {
        camera.clone(): { "fov": 1.0 },
        volume.clone(): { "priority": 0, "settings": { "bloom": { "enabled": true } } },
        // A type that never had the field, holding one of its own.
        Uuid::new_v4().to_string(): { "enabled": true },
    } } });
    assert!(legacy_enabled_fields(&scene).is_empty());

    scene["components"][object][&camera]["enabled"] = Value::Bool(true);
    scene["components"][object][&volume]["enabled"] = Value::Bool(false);
    let mut found = legacy_enabled_fields(&scene);
    found.sort();
    let mut expected = vec![
        (object.to_string(), ComponentCamera::type_uuid()),
        (
            object.to_string(),
            ComponentGlobalPostProcessVolume::type_uuid(),
        ),
    ];
    expected.sort();
    assert_eq!(found, expected);
}

#[test]
fn saved_scenes_and_prefabs_hold_no_legacy_enabled_fields() {
    let root = repository_root();
    for (index, path) in saved_files(&["cxscene", "cxprefab"]) {
        let file = root.join(ASSET_ROOTS[index]).join(&path);
        let source: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap())
            .unwrap_or_else(|e| panic!("{} is not JSON: {e}", file.display()));
        let found = legacy_enabled_fields(scene_data(&source, &path));
        assert!(
            found.is_empty(),
            "{} holds `enabled` fields that the scene's disabled_components map \
             now keeps and loading no longer reads (object, component type): \
             {found:?}",
            file.display()
        );
    }
}

/// The objects in a saved scene whose global post-process volume still holds
/// the combined `settings` that the effect components on its game object
/// replaced, or FXAA's old `fxaa` switch. Loading ignores both, so such a
/// scene would silently lose its effects.
fn combined_post_process_settings(scene: &Value) -> Vec<String> {
    let volume = ComponentGlobalPostProcessVolume::type_uuid().to_string();
    let anti_aliasing = AntiAliasingSettings::type_uuid().to_string();
    scene["components"]
        .as_object()
        .expect("a saved scene has a components map")
        .iter()
        .filter(|(_, components)| {
            components[&volume].get("settings").is_some()
                || components[&anti_aliasing].get("fxaa").is_some()
        })
        .map(|(object, _)| object.clone())
        .collect()
}

#[test]
fn combined_post_process_settings_flags_only_the_old_layout() {
    let object = "07b17e3d-9b73-411b-9d08-c56698a37556";
    let volume = ComponentGlobalPostProcessVolume::type_uuid().to_string();
    let bloom = BloomSettings::type_uuid().to_string();
    let mut scene = serde_json::json!({ "components": { object: {
        volume.clone(): { "priority": 0 },
        bloom: { "intensity": 0.25 },
    } } });
    assert!(combined_post_process_settings(&scene).is_empty());

    scene["components"][object][&volume]["settings"] =
        serde_json::json!({ "bloom": { "enabled": true } });
    assert_eq!(combined_post_process_settings(&scene), [object]);
}

#[test]
fn saved_scenes_and_prefabs_hold_no_combined_post_process_settings() {
    let root = repository_root();
    for (index, path) in saved_files(&["cxscene", "cxprefab"]) {
        let file = root.join(ASSET_ROOTS[index]).join(&path);
        let source: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap())
            .unwrap_or_else(|e| panic!("{} is not JSON: {e}", file.display()));
        let found = combined_post_process_settings(scene_data(&source, &path));
        assert!(
            found.is_empty(),
            "{} holds a global post-process volume with combined `settings`,              which loading no longer reads; each effect is its own component              on the volume's game object (objects): {found:?}",
            file.display()
        );
    }
}

fn sandbox_assets() -> (TestAssets, AssetContext) {
    let copy = TestAssets::game();
    let assets = copy.asset_context();
    sandbox::plugin_main(&mut assets.registries.types.write());
    assets
        .registries
        .components
        .write()
        .refresh_class_lists(&assets.registries.types.read());
    (copy, assets)
}

#[test]
fn saved_scenes_and_prefabs_load_with_every_component_type_registered() {
    let (copy, assets) = sandbox_assets();
    let copied_roots = [copy.engine_root(), copy.sandbox_root()];
    for (index, path) in saved_files(&["cxscene", "cxprefab"]) {
        let file = copied_roots[index].join(&path);
        let source: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap())
            .unwrap_or_else(|e| panic!("{} is not JSON: {e}", file.display()));
        let is_prefab = path
            .extension()
            .is_some_and(|extension| extension == "cxprefab");
        for type_uuid in component_types(scene_data(&source, &path)) {
            assert!(
                assets
                    .registries
                    .components
                    .read()
                    .component(type_uuid)
                    .is_some(),
                "{} holds a component of unregistered type {type_uuid}",
                file.display()
            );
        }

        let registry = assets.registries.assets.read();
        if is_prefab {
            registry
                .reload_by_path::<Prefab>(&file)
                .unwrap_or_else(|e| panic!("{} does not load: {e:?}", file.display()));
        } else {
            registry
                .reload_by_path::<Scene>(&file)
                .unwrap_or_else(|e| panic!("{} does not load: {e:?}", file.display()));
        }
    }
}
