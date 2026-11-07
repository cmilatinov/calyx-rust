//! The demos project's checked-in scenes load with every component the
//! engine and this assembly register, and save back byte for byte. The
//! sandbox's `saved_data` tests check the engine's and sandbox's scenes.

use std::path::{Path, PathBuf};

use calyx_engine::scene::{Scene, SceneData};
use calyx_engine::test_support::TestAssets;

fn scenes(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scenes(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "cxscene")
        {
            found.push(path);
        }
    }
}

/// To migrate the scenes after an intentional format change, run this test
/// with CALYX_UPDATE_SCENES=1 and review the resulting asset-only diff.
#[test]
fn checked_in_scenes_load_and_save_byte_for_byte() {
    let copy = TestAssets::engine();
    let assets = copy.asset_context();
    demos::plugin_main(&mut assets.registries.types.write());
    assets
        .registries
        .components
        .write()
        .refresh_class_lists(&assets.registries.types.read());
    let registries = assets.registries.lock_read();
    let mut paths = Vec::new();
    scenes(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"),
        &mut paths,
    );
    paths.sort();
    assert!(!paths.is_empty());
    let update = std::env::var("CALYX_UPDATE_SCENES").as_deref() == Ok("1");
    let mut changed = Vec::new();
    for path in paths {
        let source = std::fs::read_to_string(&path).unwrap();
        let data: SceneData = serde_json::from_str(&source).unwrap();
        let (scene, dropped) = Scene::load(&registries, data);
        assert!(dropped.is_empty(), "{}: {dropped:?}", path.display());
        let saved = serde_json::to_string_pretty(&scene).unwrap();
        let (reloaded, dropped) = Scene::load(&registries, serde_json::from_str(&saved).unwrap());
        assert!(dropped.is_empty(), "{}: {dropped:?}", path.display());
        assert!(
            saved == serde_json::to_string_pretty(&reloaded).unwrap(),
            "{} changes on its second save",
            path.display()
        );
        if update {
            std::fs::write(&path, &saved).unwrap();
        } else if source != saved {
            changed.push(path.display().to_string());
        }
    }
    assert!(changed.is_empty(), "scenes changed on save: {changed:#?}");
}
