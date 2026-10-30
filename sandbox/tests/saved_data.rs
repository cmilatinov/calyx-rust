//! Saved scenes, prefabs and `.meta` files name types by UUID, never by a
//! Rust path, so renaming or moving a crate leaves them loading unchanged.
//! These tests read every committed one and fail if a crate-qualified type
//! name appears in them, or if a component they hold has no registered type.

// Matches the crates under test: the particle system component nests deep
// enough that proving it `Send`/`Sync` passes the default limit.
#![recursion_limit = "256"]

use std::path::{Path, PathBuf};

use calyx_engine::context::AssetContext;
use calyx_engine::scene::{Prefab, Scene};
use calyx_engine::test_support::TestAssets;
use calyx_engine::utils::Uuid;
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
        let scene = if is_prefab { &source["data"] } else { &source };
        for type_uuid in component_types(scene) {
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
