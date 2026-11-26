use super::*;
use crate::assets::error::AssetError;
use crate::assets::{Asset, LoadedAsset};
use crate::context::ReadOnlyAssetContext;
use crate::scene::Scene;
use crate::test_support::test_registries_with_assets;
use crate::utils::TypeUuid;
use notify::event::{CreateKind, DataChange, ModifyKind};
use notify::{Event, EventKind};
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;

struct MetaTestText {
    text: String,
}

impl TypeUuid for MetaTestText {
    fn type_uuid() -> Uuid {
        Uuid::from_u128(0x2f6c_93d1_0b5e_4a77_8c12_6e4d_a9f0_7b33)
    }
}

impl Asset for MetaTestText {
    fn file_extensions() -> &'static [&'static str] {
        &["cxmetatext"]
    }

    fn from_file(
        _assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?;
        Ok(LoadedAsset::new(Self { text }))
    }
}

fn temp_asset_root(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&path).expect("failed to create temp asset root");
    path
}

/// Writes the `.meta` an external tool would write for `name`.
fn write_meta(path: &Path, id: Uuid, name: &str, type_uuid: Uuid) {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&json!({
            "main": {
                "id": id,
                "name": name,
                "display_name": name,
                "type_uuid": type_uuid,
            },
            "inner": []
        }))
        .expect("failed to encode test meta"),
    )
    .expect("failed to write test meta");
}

fn meta_id_on_disk(path: &Path) -> Uuid {
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("failed to read the meta"))
            .expect("failed to parse the meta");
    meta["main"]["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("the meta has no id")
}

fn created(path: &Path) -> Event {
    Event::new(EventKind::Create(CreateKind::File)).add_path(path.to_path_buf())
}

fn modified(path: &Path) -> Event {
    Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
        .add_path(path.to_path_buf())
}

fn settle(registry: &AssetRegistry) {
    registry.register_settled_files(Instant::now() + CREATED_FILE_SETTLE);
}

#[test]
fn a_meta_written_shortly_after_its_file_keeps_its_id() {
    let root = temp_asset_root("calyx-meta-after-file");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let asset_path = root.join("level.cxscene");
    let meta_path = root.join("level.meta");
    let chosen = Uuid::new_v4();

    std::fs::write(&asset_path, b"{}").expect("failed to write the scene");
    registry.recv_notify_event(created(&asset_path));
    write_meta(&meta_path, chosen, "level", Scene::type_uuid());
    registry.recv_notify_event(created(&meta_path));
    settle(&registry);

    assert_eq!(registry.asset_id_from_path(&asset_path), Some(chosen));
    assert_eq!(registry.asset_id("level"), Some(chosen));
    assert_eq!(meta_id_on_disk(&meta_path), chosen);
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn a_meta_written_before_its_file_keeps_its_id() {
    let root = temp_asset_root("calyx-meta-before-file");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let asset_path = root.join("level.cxscene");
    let meta_path = root.join("level.meta");
    let chosen = Uuid::new_v4();

    write_meta(&meta_path, chosen, "level", Scene::type_uuid());
    registry.recv_notify_event(created(&meta_path));
    std::fs::write(&asset_path, b"{}").expect("failed to write the scene");
    registry.recv_notify_event(created(&asset_path));
    settle(&registry);

    assert_eq!(registry.asset_id_from_path(&asset_path), Some(chosen));
    assert_eq!(meta_id_on_disk(&meta_path), chosen);
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn a_created_file_gets_a_generated_meta_only_once_the_window_passes() {
    let root = temp_asset_root("calyx-meta-settle-window");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let asset_path = root.join("level.cxscene");
    let meta_path = root.join("level.meta");

    std::fs::write(&asset_path, b"{}").expect("failed to write the scene");
    registry.recv_notify_event(created(&asset_path));

    assert!(registry.is_pending_file(&asset_path));
    assert_eq!(registry.asset_id_from_path(&asset_path), None);
    assert!(
        !meta_path.exists(),
        "no .meta is generated inside the window"
    );
    registry.register_settled_files(Instant::now());
    assert_eq!(registry.asset_id_from_path(&asset_path), None);

    settle(&registry);

    let id = registry
        .asset_id_from_path(&asset_path)
        .expect("the file is registered once the window passes");
    assert_eq!(meta_id_on_disk(&meta_path), id);
    assert!(!registry.is_pending_file(&asset_path));
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn writing_a_pending_file_again_restarts_its_window() {
    let root = temp_asset_root("calyx-meta-settle-restart");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let asset_path = root.join("level.cxscene");

    std::fs::write(&asset_path, b"{}").expect("failed to write the scene");
    registry.recv_notify_event(created(&asset_path));
    let first_seen = Instant::now();
    std::thread::sleep(Duration::from_millis(50));
    registry.recv_notify_event(modified(&asset_path));

    registry.register_settled_files(first_seen + CREATED_FILE_SETTLE);

    assert!(registry.is_pending_file(&asset_path));
    assert_eq!(registry.asset_id_from_path(&asset_path), None);
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn a_meta_written_after_the_window_rekeys_the_asset() {
    let root = temp_asset_root("calyx-meta-rekey");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    registry.register_asset_type::<MetaTestText>();
    let asset_path = root.join("note.cxmetatext");
    let meta_path = root.join("note.meta");
    std::fs::write(&asset_path, "imported").expect("failed to write the asset");
    registry.recv_notify_event(created(&asset_path));
    settle(&registry);
    let generated = registry
        .asset_id_from_path(&asset_path)
        .expect("the file is registered under a generated id");
    let held = registry
        .load_by_id::<MetaTestText>(generated)
        .expect("the asset loads under its generated id");

    let chosen = Uuid::new_v4();
    write_meta(&meta_path, chosen, "note", MetaTestText::type_uuid());
    registry.recv_notify_event(modified(&meta_path));

    assert_eq!(registry.asset_id_from_path(&asset_path), Some(chosen));
    assert_eq!(registry.asset_id("note"), Some(chosen));
    assert!(registry.asset_meta_from_id(generated).is_none());
    assert!(registry.loaded_by_id::<MetaTestText>(generated).is_none());
    let rekeyed = registry
        .load_by_id::<MetaTestText>(chosen)
        .expect("the asset loads under its chosen id");
    assert_eq!(rekeyed.id(), chosen);
    assert_eq!(rekeyed.read().text, "imported");
    // A reference saved with the generated id resolves to the chosen one.
    let resolved = registry
        .load_by_id::<MetaTestText>(generated)
        .expect("the generated id resolves to the chosen one");
    assert_eq!(resolved.ptr_id(), rekeyed.ptr_id());
    assert_eq!(held.read().text, "imported");
    assert_eq!(meta_id_on_disk(&meta_path), chosen);
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

/// A file deleted and recreated without a `.meta` registers only once
/// its window passes, after its create event; the failed background load
/// of its id is forgotten then, so the next poll retries.
#[test]
fn a_settled_file_retries_its_failed_background_load() {
    let root = temp_asset_root("calyx-meta-settle-retry");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let asset_path = root.join("level.cxscene");
    std::fs::write(&asset_path, b"{}").expect("failed to write the scene");
    registry.recv_notify_event(created(&asset_path));
    settle(&registry);
    let id = registry
        .asset_id_from_path(&asset_path)
        .expect("the file is registered");
    registry.background_loads.lock().unwrap().insert(
        id,
        super::super::BackgroundLoadState::Failed(AssetError::LoadError.with_source("test")),
    );

    std::fs::remove_file(&asset_path).expect("failed to delete the scene");
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path.clone()),
    );
    std::fs::write(&asset_path, b"{}").expect("failed to recreate the scene");
    registry.recv_notify_event(created(&asset_path));
    settle(&registry);

    assert_eq!(registry.asset_id_from_path(&asset_path), Some(id));
    assert!(!registry.background_loads.lock().unwrap().contains_key(&id));
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

/// A model's sub-assets are registered from its `.meta`, or when it
/// loads. A tool's `.meta` that lists none keeps the ones the model had,
/// under the new parent, so loading one by its id still finds the model.
#[test]
fn rekeying_keeps_sub_assets_the_new_meta_does_not_list() {
    let root = temp_asset_root("calyx-meta-rekey-children");
    let parent_path = root.join("parent.fbx");
    let meta_path = root.join("parent.meta");
    let (old_parent, child) = (Uuid::new_v4(), Uuid::new_v4());
    std::fs::write(&parent_path, b"").expect("failed to write the model");
    std::fs::write(
        &meta_path,
        serde_json::to_vec_pretty(&json!({
            "main": {
                "id": old_parent,
                "name": "parent",
                "display_name": "parent",
                "type_uuid": crate::scene::Prefab::type_uuid(),
            },
            "inner": [{
                "id": child,
                "name": "Body",
                "display_name": "Body",
                "type_uuid": crate::assets::mesh::Mesh::type_uuid(),
            }]
        }))
        .expect("failed to encode the meta"),
    )
    .expect("failed to write the meta");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    assert_eq!(registry.asset_id("parent/Body"), Some(child));

    let chosen = Uuid::new_v4();
    write_meta(
        &meta_path,
        chosen,
        "parent",
        crate::scene::Prefab::type_uuid(),
    );
    registry.recv_notify_event(modified(&meta_path));

    assert_eq!(registry.asset_id_from_path(&parent_path), Some(chosen));
    assert_eq!(registry.asset_id("parent/Body"), Some(child));
    let child_meta = registry
        .asset_meta_from_id(child)
        .expect("the sub-asset keeps its metadata");
    assert_eq!(child_meta.parent, Some(chosen));
    assert_eq!(
        registry
            .asset_meta_from_id(chosen)
            .map(|meta| meta.children),
        Some(vec![child])
    );
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

/// A tool that gives the asset a fresh id on every import re-keys it
/// again and again; the first id still resolves to the last.
#[test]
fn an_id_from_before_many_rekeys_resolves_to_the_latest() {
    let root = temp_asset_root("calyx-meta-rekey-chain");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    registry.register_asset_type::<MetaTestText>();
    let asset_path = root.join("note.cxmetatext");
    let meta_path = root.join("note.meta");
    std::fs::write(&asset_path, "imported").expect("failed to write the asset");
    registry.recv_notify_event(created(&asset_path));
    settle(&registry);
    let first = registry
        .asset_id_from_path(&asset_path)
        .expect("the file is registered under a generated id");

    let mut latest = first;
    for _ in 0..12 {
        latest = Uuid::new_v4();
        write_meta(&meta_path, latest, "note", MetaTestText::type_uuid());
        registry.recv_notify_event(modified(&meta_path));
    }

    assert_eq!(registry.asset_id_from_path(&asset_path), Some(latest));
    let resolved = registry
        .load_by_id::<MetaTestText>(first)
        .expect("the first id resolves to the latest");
    assert_eq!(resolved.id(), latest);
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn loading_an_asset_never_rewrites_the_id_of_its_meta_on_disk() {
    let root = temp_asset_root("calyx-meta-no-rewrite");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    registry.register_asset_type::<MetaTestText>();
    let asset_path = root.join("note.cxmetatext");
    let meta_path = root.join("note.meta");
    std::fs::write(&asset_path, "imported").expect("failed to write the asset");
    registry.recv_notify_event(created(&asset_path));
    settle(&registry);
    let generated = registry
        .asset_id_from_path(&asset_path)
        .expect("the file is registered under a generated id");

    // The tool's `.meta` lands before the watcher reports it, and the
    // asset loads under the id the registry still holds.
    let chosen = Uuid::new_v4();
    write_meta(&meta_path, chosen, "note", MetaTestText::type_uuid());
    registry
        .load_by_id::<MetaTestText>(generated)
        .expect("the asset loads under its generated id");

    assert_eq!(meta_id_on_disk(&meta_path), chosen);
    registry.recv_notify_event(modified(&meta_path));
    assert_eq!(registry.asset_id_from_path(&asset_path), Some(chosen));
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn the_registrys_own_meta_write_leaves_the_asset_keyed_as_it_is() {
    let root = temp_asset_root("calyx-meta-own-write");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let asset_path = root.join("level.cxscene");
    let meta_path = root.join("level.meta");
    std::fs::write(&asset_path, b"{}").expect("failed to write the scene");
    registry.recv_notify_event(created(&asset_path));
    settle(&registry);
    let id = registry
        .asset_id_from_path(&asset_path)
        .expect("the file is registered");
    let generation = registry
        .asset_data()
        .generations
        .get(&id)
        .map(|(generation, _)| *generation);

    registry.recv_notify_event(created(&meta_path));
    registry.recv_notify_event(modified(&meta_path));

    assert_eq!(registry.asset_id_from_path(&asset_path), Some(id));
    assert_eq!(
        registry
            .asset_data()
            .generations
            .get(&id)
            .map(|(generation, _)| *generation),
        generation
    );
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}
