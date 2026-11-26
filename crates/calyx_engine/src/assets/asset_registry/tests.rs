use super::*;

/// A scene's mesh whose file is gone resolves every frame, so the missing
/// asset is reported once, not every frame; an unset (nil) reference is
/// not an error.
#[test]
fn a_missing_asset_is_reported_once() {
    let registries = crate::test_support::test_registries();
    let missing = Uuid::new_v4();
    let assets = registries.assets.read();
    assert!(assets.load_by_id::<Mesh>(missing).is_err());
    assert!(
        !report_missing_asset(missing),
        "the load already reported it"
    );
    assert!(report_missing_asset(Uuid::new_v4()));
    assert!(!report_missing_asset(Uuid::nil()));
}
use crate::test_support::test_registries_with_assets;
use serde_json::json;

fn temp_asset_root(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&path).expect("failed to create temp asset root");
    path
}

/// Registers the created files that wait for a `.meta`, as the watcher
/// thread does once their settle window passes.
fn settle(registry: &AssetRegistry) {
    registry.register_settled_files(Instant::now() + meta_sync::CREATED_FILE_SETTLE);
}

fn write_parent_with_child_meta(root: &Path, child_name: &str) -> (Uuid, Uuid) {
    let parent_id = Uuid::new_v4();
    let child_id = Uuid::new_v4();
    std::fs::write(root.join("parent.fbx"), b"").expect("failed to write parent asset");
    std::fs::write(
        root.join("parent.meta"),
        serde_json::to_vec_pretty(&json!({
            "main": {
                "id": parent_id,
                "name": "parent",
                "display_name": "parent",
                "type_uuid": Prefab::type_uuid(),
            },
            "inner": [
                {
                    "id": child_id,
                    "name": child_name,
                    "display_name": child_name,
                    "type_uuid": Mesh::type_uuid(),
                }
            ]
        }))
        .expect("failed to encode test meta"),
    )
    .expect("failed to write parent meta");
    (parent_id, child_id)
}

fn write_scene_with_meta(root: &Path) -> (PathBuf, PathBuf, Uuid) {
    let asset_id = Uuid::new_v4();
    let asset_path = root.join("scene.cxscene");
    let meta_path = root.join("scene.meta");
    std::fs::write(&asset_path, b"{}").expect("failed to write scene asset");
    std::fs::write(
        &meta_path,
        serde_json::to_vec_pretty(&json!({
            "main": {
                "id": asset_id,
                "name": "scene",
                "display_name": "scene",
                "type_uuid": Scene::type_uuid(),
            },
            "inner": []
        }))
        .expect("failed to encode scene meta"),
    )
    .expect("failed to write scene meta");
    (asset_path, meta_path, asset_id)
}

fn written_meta_type_uuid(meta_path: &Path) -> Uuid {
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(meta_path).expect("failed to read written metadata"))
            .expect("failed to parse written metadata");
    meta["main"]["type_uuid"]
        .as_str()
        .and_then(|uuid| Uuid::parse_str(uuid).ok())
        .expect("written metadata has no type UUID")
}

#[test]
fn new_meta_written_at_startup_records_the_extension_type() {
    let root = temp_asset_root("calyx-new-meta-startup-type");
    std::fs::write(root.join("scene.cxscene"), b"{}").expect("failed to write scene asset");
    let registries = test_registries_with_assets(vec![root.clone()]);

    assert_eq!(
        written_meta_type_uuid(&root.join("scene.meta")),
        Scene::type_uuid()
    );
    drop(registries);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn new_meta_written_for_created_file_records_the_extension_type() {
    let root = temp_asset_root("calyx-new-meta-created-type");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let asset_path = root.join("scene.cxscene");
    std::fs::write(&asset_path, b"{}").expect("failed to write scene asset");
    let event = Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(asset_path);

    registry.recv_notify_event(event);
    settle(&registry);

    assert_eq!(
        written_meta_type_uuid(&root.join("scene.meta")),
        Scene::type_uuid()
    );
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn startup_writes_no_meta_beside_a_file_of_an_unregistered_type() {
    let root = temp_asset_root("calyx-startup-unregistered-ext");
    let path = root.join("notes.txt");
    std::fs::write(&path, b"notes").expect("failed to write text file");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();

    assert!(!root.join("notes.meta").exists(), "no .meta is written");
    assert_eq!(registry.asset_id_from_path(&path), None);
    assert_eq!(registry.asset_id("notes"), None);
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn created_file_of_an_unregistered_type_gets_no_meta() {
    let root = temp_asset_root("calyx-created-unregistered-ext");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let path = root.join("notes.txt");
    std::fs::write(&path, b"notes").expect("failed to write text file");

    registry.recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(path.clone()),
    );

    assert!(!root.join("notes.meta").exists(), "no .meta is written");
    assert_eq!(registry.asset_id_from_path(&path), None);
    assert_eq!(registry.asset_id("notes"), None);
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn registering_a_file_of_an_unregistered_type_is_refused() {
    let root = temp_asset_root("calyx-register-unregistered-ext");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let path = root.join("notes.txt");
    std::fs::write(&path, b"notes").expect("failed to write text file");

    let error = registry
        .register_file(&path)
        .expect_err("a .txt file has no registered asset type");

    assert!(
        error.to_string().contains("no registered asset type"),
        "{error}"
    );
    assert_eq!(registry.asset_id_from_path(&path), None);
    assert!(!root.join("notes.meta").exists(), "no .meta is written");
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn deleting_a_file_of_an_unregistered_type_keeps_a_same_stem_assets_meta() {
    let root = temp_asset_root("calyx-remove-unregistered-ext");
    let (asset_path, meta_path, asset_id) = write_scene_with_meta(&root);
    let text_path = root.join("scene.txt");
    std::fs::write(&text_path, b"notes").expect("failed to write text file");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();

    std::fs::remove_file(&text_path).expect("failed to delete text file");
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(text_path),
    );

    assert!(meta_path.exists(), "the scene keeps its .meta");
    assert_eq!(registry.asset_id_from_path(&asset_path), Some(asset_id));
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn remove_event_for_replaced_asset_preserves_metadata() {
    let root = temp_asset_root("calyx-asset-replacement-event");
    let (asset_path, meta_path, asset_id) = write_scene_with_meta(&root);
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let original_meta = std::fs::read(&meta_path).expect("failed to read original metadata");
    let replacement_path = root.join("scene.cxscene.tmp");
    std::fs::write(&replacement_path, b"replacement").expect("failed to stage replacement asset");
    if std::fs::rename(&replacement_path, &asset_path).is_err() {
        let backup_path = root.join("scene.cxscene.backup");
        std::fs::rename(&asset_path, &backup_path).expect("failed to stage original asset");
        std::fs::rename(&replacement_path, &asset_path)
            .expect("failed to install replacement asset");
        std::fs::remove_file(backup_path).expect("failed to remove original asset backup");
    }
    let event =
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path.clone());

    registry.recv_notify_event(event);

    assert_eq!(
        std::fs::read(&asset_path).expect("failed to read replacement asset"),
        b"replacement"
    );
    assert_eq!(
        std::fs::read(&meta_path).expect("failed to read preserved metadata"),
        original_meta
    );
    assert!(registry.asset_data().dirty.contains_key(&asset_id));
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

/// A glTF model reads its external images when it imports, so creating, editing or
/// deleting one marks the model dirty, as an edited include does a shader.
#[test]
fn external_image_events_mark_the_gltf_model_dirty() {
    let root = temp_asset_root("calyx-gltf-image-dependency");
    let model_path = root.join("model.gltf");
    std::fs::write(
        &model_path,
        r#"{"asset":{"version":"2.0"},"images":[{"uri":"textures/base%20color.png"}]}"#,
    )
    .expect("failed to write the model");
    std::fs::create_dir_all(root.join("textures")).expect("failed to create textures");
    let image_path = root.join("textures/base color.png");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let model_id = registry
        .asset_id_from_path(&model_path)
        .expect("the model is registered");
    let take_dirty = || registry.asset_data_mut().dirty.remove(&model_id).is_some();

    std::fs::write(&image_path, b"png").expect("failed to create the image");
    registry.recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(image_path.clone()),
    );
    assert!(take_dirty(), "creating the missing image marks the model");

    registry.recv_notify_event(
        Event::new(EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(image_path.clone()),
    );
    assert!(take_dirty(), "editing the image marks the model");

    std::fs::remove_file(&image_path).expect("failed to delete the image");
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(image_path.clone()),
    );
    assert!(take_dirty(), "deleting the image marks the model");

    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

/// A deleted folder arrives as one event for the folder, so the models
/// reading images inside it are marked dirty by that path alone.
#[test]
fn deleting_the_folder_of_an_external_image_marks_the_gltf_model_dirty() {
    let root = temp_asset_root("calyx-gltf-image-folder");
    let model_path = root.join("model.gltf");
    std::fs::write(
        &model_path,
        r#"{"asset":{"version":"2.0"},"images":[{"uri":"textures/nested/base.png"}]}"#,
    )
    .expect("failed to write the model");
    let textures = root.join("textures");
    std::fs::create_dir_all(textures.join("nested")).expect("failed to create textures");
    std::fs::write(textures.join("nested/base.png"), b"png").expect("failed to write image");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let model_id = registry
        .asset_id_from_path(&model_path)
        .expect("the model is registered");

    std::fs::remove_dir_all(&textures).expect("failed to delete the folder");
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::Folder)).add_path(textures.clone()),
    );

    assert!(registry.asset_data().dirty.contains_key(&model_id));
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

/// An image in a sibling folder that does not exist yet is keyed through
/// the nearest existing ancestor, so creating the folder and the image
/// marks the model dirty.
#[test]
fn creating_a_missing_image_folder_outside_the_model_folder_marks_the_gltf_model_dirty() {
    let root = temp_asset_root("calyx-gltf-image-missing-folder");
    std::fs::create_dir_all(root.join("models")).expect("failed to create models");
    let model_path = root.join("models/model.gltf");
    std::fs::write(
        &model_path,
        r#"{"asset":{"version":"2.0"},"images":[{"uri":"../textures/base.png"}]}"#,
    )
    .expect("failed to write the model");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let model_id = registry
        .asset_id_from_path(&model_path)
        .expect("the model is registered");

    std::fs::create_dir_all(root.join("textures")).expect("failed to create textures");
    let image_path = root.join("textures/base.png");
    std::fs::write(&image_path, b"png").expect("failed to create the image");
    registry.recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(image_path),
    );

    assert!(registry.asset_data().dirty.contains_key(&model_id));
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn remove_event_for_deleted_asset_removes_metadata() {
    let root = temp_asset_root("calyx-asset-deletion-event");
    let (asset_path, meta_path, _) = write_scene_with_meta(&root);
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    std::fs::remove_file(&asset_path).expect("failed to delete scene asset");
    let event = Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path);

    registry.recv_notify_event(event);

    assert!(!meta_path.exists());
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn create_event_in_second_root_registers_relative_to_that_root() {
    let first_root = temp_asset_root("calyx-create-event-first-root");
    let second_root = temp_asset_root("calyx-create-event-second-root");
    let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
    let registry = registries.assets.read();
    std::fs::create_dir_all(second_root.join("nested")).expect("failed to create nested dir");
    let asset_path = second_root.join("nested").join("created.cxscene");
    std::fs::write(&asset_path, b"{}").expect("failed to write created asset");
    let event =
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(asset_path.clone());

    registry.recv_notify_event(event);
    settle(&registry);

    let id = registry
        .asset_id("nested/created")
        .expect("created asset is registered under the second root");
    let meta = registry
        .asset_meta_from_id(id)
        .expect("created asset has metadata");
    assert_eq!(meta.name, "nested/created");
    assert_eq!(registry.asset_id_from_path(&asset_path), Some(id));
    assert!(second_root.join("nested").join("created.meta").exists());
    drop(registry);
    std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
    std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
}

#[test]
fn registering_a_file_without_an_extension_is_refused() {
    let root = temp_asset_root("calyx-register-extensionless");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    // A scene saved as `new_level`, with no extension to type it by.
    let path = root.join("new_level");
    std::fs::write(&path, b"{}").expect("failed to write extensionless file");

    let error = registry
        .register_file(&path)
        .expect_err("a file without an extension has no asset type");

    assert!(error.to_string().contains("no file extension"), "{error}");
    assert_eq!(registry.asset_id_from_path(&path), None);
    assert!(!root.join("new_level.meta").exists(), "no .meta is written");
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn a_root_named_in_another_form_owns_the_files_under_it() {
    let root = temp_asset_root("calyx-register-root-form");
    std::fs::create_dir_all(root.join("sub")).expect("failed to create sub directory");
    // The same directory as `root`, named through `sub/..`, as an 8.3
    // short name or a symlink would name it differently on disk.
    let alternate_root = root.join("sub").join("..");
    let registries = test_registries_with_assets(vec![alternate_root]);
    let registry = registries.assets.read();
    let path = root.join("new_level.cxscene");
    std::fs::write(&path, b"{}").expect("failed to write asset file");

    assert!(registry.is_in_asset_root(&path));
    let id = registry
        .register_file(&path)
        .expect("a file under the root registers");

    assert_eq!(registry.asset_id("new_level"), Some(id));
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn rename_onto_a_new_asset_file_registers_it_with_its_meta() {
    let root = temp_asset_root("calyx-rename-event");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    // How scenes and prefabs are saved: a temporary file renamed onto
    // the target, which the watcher reports as a rename, not a create.
    // On Windows the target arrives alone, as the rename's new name.
    let temp_path = root.join("saved.cxscene.0123.tmp");
    let asset_path = root.join("saved.cxscene");
    std::fs::write(&temp_path, b"{}").expect("failed to write temp file");
    std::fs::rename(&temp_path, &asset_path).expect("failed to rename temp file");
    let event = Event::new(EventKind::Modify(notify::event::ModifyKind::Name(
        notify::event::RenameMode::To,
    )))
    .add_path(asset_path.clone());

    registry.recv_notify_event(event);
    settle(&registry);

    let id = registry
        .asset_id_from_path(&asset_path)
        .expect("the renamed-to file is registered");
    assert_eq!(registry.asset_id("saved"), Some(id));
    assert!(root.join("saved.meta").exists(), "its .meta is written");
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn path_lookup_uses_deepest_root_when_roots_nest() {
    let outer_root = temp_asset_root("calyx-nested-roots");
    let inner_root = outer_root.join("inner");
    std::fs::create_dir_all(&inner_root).expect("failed to create inner asset root");
    let registries = test_registries_with_assets(vec![outer_root.clone(), inner_root.clone()]);
    let registry = registries.assets.read();
    let asset_path = inner_root.join("created.cxscene");
    std::fs::write(&asset_path, b"{}").expect("failed to write created asset");
    let event =
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(asset_path.clone());

    registry.recv_notify_event(event);
    settle(&registry);

    let id = registry
        .asset_id("created")
        .expect("created asset is registered under the inner root");
    assert_eq!(registry.asset_id_from_path(&asset_path), Some(id));
    drop(registry);
    std::fs::remove_dir_all(outer_root).expect("failed to remove temp asset root");
}

#[test]
fn startup_scan_names_nested_assets_relative_to_deepest_root() {
    let outer_root = temp_asset_root("calyx-nested-startup");
    let inner_root = outer_root.join("inner");
    std::fs::create_dir_all(&inner_root).expect("failed to create inner asset root");
    let asset_path = inner_root.join("existing.cxscene");
    std::fs::write(&asset_path, b"{}").expect("failed to write existing asset");
    let registries = test_registries_with_assets(vec![outer_root.clone(), inner_root.clone()]);
    let registry = registries.assets.read();

    let id = registry
        .asset_id("existing")
        .expect("existing asset is registered under the inner root");
    assert_eq!(
        registry.asset_meta_from_id(id).map(|meta| meta.name),
        Some("existing".into())
    );
    assert_eq!(registry.asset_id_from_path(&asset_path), Some(id));
    assert_eq!(registry.asset_id("inner/existing"), None);
    drop(registry);
    std::fs::remove_dir_all(outer_root).expect("failed to remove temp asset root");
}

#[test]
fn remove_event_forgets_only_the_asset_at_the_removed_path() {
    let first_root = temp_asset_root("calyx-same-name-first-root");
    let second_root = temp_asset_root("calyx-same-name-second-root");
    let (first_path, _, first_id) = write_scene_with_meta(&first_root);
    let (_, _, second_id) = write_scene_with_meta(&second_root);
    let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
    let registry = registries.assets.read();
    assert_eq!(registry.asset_id("scene"), Some(second_id));

    std::fs::remove_file(&first_path).expect("failed to delete first scene");
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(first_path),
    );

    assert!(registry.asset_meta_from_id(first_id).is_none());
    assert!(registry.asset_meta_from_id(second_id).is_some());
    assert_eq!(registry.asset_id("scene"), Some(second_id));
    drop(registry);
    std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
    std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
}

#[test]
fn remove_event_for_name_owner_restores_same_name_asset_from_other_root() {
    let first_root = temp_asset_root("calyx-same-name-restore-first-root");
    let second_root = temp_asset_root("calyx-same-name-restore-second-root");
    let (_, _, first_id) = write_scene_with_meta(&first_root);
    let (second_path, _, second_id) = write_scene_with_meta(&second_root);
    let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
    let registry = registries.assets.read();
    assert_eq!(registry.asset_id("scene"), Some(second_id));

    std::fs::remove_file(&second_path).expect("failed to delete second scene");
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(second_path),
    );

    assert!(registry.asset_meta_from_id(second_id).is_none());
    assert_eq!(registry.asset_id("scene"), Some(first_id));
    drop(registry);
    std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
    std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
}

#[test]
fn remove_event_restores_same_name_asset_sharing_a_generated_id() {
    let first_root = temp_asset_root("calyx-shared-id-first-root");
    let second_root = temp_asset_root("calyx-shared-id-second-root");
    let first_path = first_root.join("scene.cxscene");
    let second_path = second_root.join("scene.cxscene");
    std::fs::write(&first_path, b"{}").expect("failed to write first scene");
    std::fs::write(&second_path, b"{}").expect("failed to write second scene");
    let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
    let registry = registries.assets.read();
    let id = registry.asset_id("scene").expect("scene is registered");
    assert_eq!(
        registry.asset_meta_from_id(id).and_then(|meta| meta.path),
        Some(second_path.clone())
    );

    std::fs::remove_file(&second_path).expect("failed to delete second scene");
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(second_path),
    );

    let restored = registry
        .asset_id("scene")
        .expect("the first root's scene takes the name back");
    assert_eq!(
        registry
            .asset_meta_from_id(restored)
            .and_then(|meta| meta.path),
        Some(first_path)
    );
    drop(registry);
    std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
    std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
}

#[test]
fn remove_event_clears_reload_errors_of_deleted_asset() {
    let root = temp_asset_root("calyx-remove-clears-reload-error");
    let (asset_path, _, asset_id) = write_scene_with_meta(&root);
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let generation = registry
        .asset_data()
        .generations
        .get(&asset_id)
        .map(|(generation, _)| *generation);
    registry.set_current_reload_error(
        generation,
        AssetReloadError {
            id: asset_id,
            path: asset_path.clone(),
            error: AssetError::LoadError.with_source("test reload failure"),
        },
    );
    assert_eq!(registry.reload_errors().len(), 1);

    std::fs::remove_file(&asset_path).expect("failed to delete scene asset");
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path),
    );

    assert!(registry.reload_errors().is_empty());
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn folder_remove_event_forgets_assets_below_the_folder() {
    let root = temp_asset_root("calyx-folder-remove");
    let folder = root.join("sub");
    std::fs::create_dir_all(&folder).expect("failed to create asset folder");
    let (_, _, asset_id) = write_scene_with_meta(&folder);
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    assert_eq!(registry.asset_id("sub/scene"), Some(asset_id));

    std::fs::remove_dir_all(&folder).expect("failed to delete asset folder");
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::Folder)).add_path(folder),
    );

    assert_eq!(registry.asset_id("sub/scene"), None);
    assert!(registry.asset_meta_from_id(asset_id).is_none());
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn legacy_outer_relative_meta_is_rebased_to_owning_root() {
    let outer_root = temp_asset_root("calyx-legacy-nested-meta");
    let inner_root = outer_root.join("inner");
    std::fs::create_dir_all(&inner_root).expect("failed to create inner asset root");
    let parent_id = Uuid::new_v4();
    let child_id = Uuid::new_v4();
    std::fs::write(inner_root.join("parent.fbx"), b"").expect("failed to write parent asset");
    std::fs::write(
        inner_root.join("parent.meta"),
        serde_json::to_vec_pretty(&json!({
            "main": {
                "id": parent_id,
                "name": "inner/parent",
                "display_name": "parent",
                "type_uuid": Prefab::type_uuid(),
            },
            "inner": [
                {
                    "id": child_id,
                    "name": "inner/parent/Body",
                    "display_name": "Body",
                    "type_uuid": Mesh::type_uuid(),
                }
            ]
        }))
        .expect("failed to encode test meta"),
    )
    .expect("failed to write parent meta");
    let registries = test_registries_with_assets(vec![outer_root.clone(), inner_root.clone()]);
    let registry = registries.assets.read();

    assert_eq!(registry.asset_id("parent"), Some(parent_id));
    assert_eq!(registry.asset_id("parent/Body"), Some(child_id));
    assert_eq!(registry.asset_name(parent_id), "parent");
    assert_eq!(registry.asset_name(child_id), "Body");
    drop(registry);
    std::fs::remove_dir_all(outer_root).expect("failed to remove temp asset root");
}

#[test]
fn remove_event_for_deleted_asset_forgets_metadata_and_names() {
    let first_root = temp_asset_root("calyx-remove-event-first-root");
    let second_root = temp_asset_root("calyx-remove-event-second-root");
    let (asset_path, meta_path, asset_id) = write_scene_with_meta(&second_root);
    let (parent_id, child_id) = write_parent_with_child_meta(&second_root, "Body");
    let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
    let registry = registries.assets.read();
    assert_eq!(registry.asset_id("scene"), Some(asset_id));
    assert_eq!(registry.asset_id("parent/Body"), Some(child_id));
    std::fs::remove_file(&asset_path).expect("failed to delete scene asset");
    let parent_path = second_root.join("parent.fbx");
    std::fs::remove_file(&parent_path).expect("failed to delete parent asset");
    let event = Event::new(EventKind::Remove(notify::event::RemoveKind::File))
        .add_path(asset_path)
        .add_path(parent_path);

    registry.recv_notify_event(event);

    assert!(!meta_path.exists());
    assert!(!second_root.join("parent.meta").exists());
    assert_eq!(registry.asset_id("scene"), None);
    assert!(registry.asset_meta_from_id(asset_id).is_none());
    assert_eq!(registry.asset_id("parent"), None);
    assert_eq!(registry.asset_id("parent/Body"), None);
    assert!(registry.asset_meta_from_id(parent_id).is_none());
    assert!(registry.asset_meta_from_id(child_id).is_none());
    drop(registry);
    std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
    std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
}

#[test]
fn relative_asset_path_rejects_path_outside_root() {
    let root = temp_asset_root("calyx-relative-path-root");
    let other = temp_asset_root("calyx-relative-path-other");

    assert!(AssetRegistry::relative_asset_path(&root, &other.join("asset.obj")).is_err());
    assert_eq!(
        AssetRegistry::relative_asset_path(&root, &root.join("dir").join("asset.obj"))
            .expect("path inside root resolves")
            .as_str(),
        "dir/asset"
    );
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    std::fs::remove_dir_all(other).expect("failed to remove other temp dir");
}

#[test]
fn create_or_update_rejects_disk_asset_name() {
    let root = temp_asset_root("calyx-disk-create-or-update");
    std::fs::write(root.join("existing.obj"), b"").expect("failed to write test asset");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();

    let result = registry.create_or_update("existing".into(), Mesh::new(&registry.render_context));

    assert!(matches!(
        result,
        Err(AssetError {
            kind: AssetErrorKind::AlreadyExists,
            ..
        })
    ));
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn sub_asset_meta_uses_local_names_and_canonical_lookup() {
    let root = temp_asset_root("calyx-local-subasset-meta");
    let (parent_id, child_id) = write_parent_with_child_meta(&root, "Body");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();

    assert_eq!(registry.asset_id("parent/Body"), Some(child_id));
    assert_eq!(
        registry.asset_meta_from_id(child_id).map(|meta| meta.name),
        Some("Body".into())
    );
    assert_eq!(
        registry
            .asset_meta_from_id(child_id)
            .and_then(|meta| meta.parent),
        Some(parent_id)
    );
    assert_eq!(
        registry
            .asset_meta_from_id(parent_id)
            .map(|meta| meta.children),
        Some(vec![child_id])
    );
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

/// Plain-text asset used to observe what the registry loaded.
struct TextTestAsset {
    text: String,
}

impl TypeUuid for TextTestAsset {
    fn type_uuid() -> Uuid {
        Uuid::from_u128(0x5d1f_0c2a_7b44_4f0e_9a61_3c8e_2b17_d401)
    }
}

impl Asset for TextTestAsset {
    fn file_extensions() -> &'static [&'static str] {
        &["cxtesttext"]
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

static SLOW_TEST_ASSET_LOADS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Asset whose load is slow enough for two threads to race on it.
struct SlowTestAsset;

impl TypeUuid for SlowTestAsset {
    fn type_uuid() -> Uuid {
        Uuid::from_u128(0x9e3b_61d4_02a7_4c55_b8f2_71e0_4d6a_c902)
    }
}

impl Asset for SlowTestAsset {
    fn file_extensions() -> &'static [&'static str] {
        &["cxtestslow"]
    }

    fn from_file(
        _assets: &ReadOnlyAssetContext,
        _path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError> {
        SLOW_TEST_ASSET_LOADS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(200));
        Ok(LoadedAsset::new(Self))
    }
}

type TestGate = Arc<(std::sync::Barrier, std::sync::Barrier)>;

/// Start and release barriers for [`GatedTestAsset`] loads, per source
/// path so tests running in parallel do not share a gate.
static GATED_TEST_ASSET_GATES: std::sync::OnceLock<Mutex<HashMap<PathBuf, TestGate>>> =
    std::sync::OnceLock::new();

/// How many times a [`GatedTestAsset`] load started, per source path.
static GATED_TEST_ASSET_LOADS: std::sync::OnceLock<Mutex<HashMap<PathBuf, usize>>> =
    std::sync::OnceLock::new();

fn gated_test_asset_loads(path: &Path) -> usize {
    GATED_TEST_ASSET_LOADS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .get(path)
        .copied()
        .unwrap_or_default()
}

fn gated_test_asset_gate(path: &Path) -> TestGate {
    GATED_TEST_ASSET_GATES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new((std::sync::Barrier::new(2), std::sync::Barrier::new(2))))
        .clone()
}

/// Asset whose load, when its file contains `gate`, signals that it
/// started and then waits to be released, so a test can act while the
/// load is in flight. A file containing `fail` makes the load fail once
/// released. Like an imported prefab, it reads its metadata first and
/// creates a `Child` sub-asset once released.
struct GatedTestAsset;

impl TypeUuid for GatedTestAsset {
    fn type_uuid() -> Uuid {
        Uuid::from_u128(0x2c84_f1e9_5a30_4d7b_a6c1_08f3_9e52_b713)
    }
}

impl Asset for GatedTestAsset {
    fn file_extensions() -> &'static [&'static str] {
        &["cxtestgated"]
    }

    fn from_file(
        assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError> {
        let registry = assets.registries.assets.read();
        let meta = registry
            .asset_meta_from_path(path)
            .ok_or_else(|| AssetError::NotFound.with_path(path))?;
        let content = std::fs::read_to_string(path)
            .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?;
        *GATED_TEST_ASSET_LOADS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .entry(path.to_path_buf())
            .or_default() += 1;
        if content.contains("gate") {
            let gate = gated_test_asset_gate(path);
            gate.0.wait();
            gate.1.wait();
        }
        if content.contains("fail") {
            return Err(AssetError::LoadError
                .with_path(path)
                .with_source("test load failure"));
        }
        let child = registry.create_or_update_sub_asset(
            meta.id,
            &meta.name,
            "Child",
            TextTestAsset {
                text: "child".into(),
            },
        )?;
        Ok(LoadedAsset {
            asset: Self,
            sub_assets: vec![child.id()],
        })
    }
}

/// A registry over `root` that loads [`TextTestAsset`] and
/// [`GatedTestAsset`] files.
fn registry_with_test_assets(root: &Path) -> ReadOnlyRegistryContext {
    let registries = test_registries_with_assets(vec![root.to_path_buf()]);
    {
        let registry = registries.assets.read();
        registry.register_asset_type::<TextTestAsset>();
        registry.register_asset_type::<GatedTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
    }
    registries
}

#[test]
fn eviction_drops_only_what_the_record_loaded_and_nothing_holds() {
    let root = temp_asset_root("calyx-evict-unheld");
    std::fs::write(root.join("before.cxtesttext"), "before").unwrap();
    std::fs::write(root.join("during.cxtesttext"), "during").unwrap();
    let registries = registry_with_test_assets(&root);
    let registry = registries.assets.read();
    let before = registry.asset_id("before").unwrap();
    let during = registry.asset_id("during").unwrap();
    let held_before = registry.load_by_id::<TextTestAsset>(before).unwrap();

    let record = AssetLoadRecord::start();
    registry.load_by_id::<TextTestAsset>(before).unwrap();
    registry.load_by_id::<TextTestAsset>(during).unwrap();
    let loaded = record.finish();

    assert_eq!(
        loaded,
        vec![during],
        "an asset already cached is not recorded"
    );
    let count = registry.loaded_count();
    assert_eq!(registry.evict_unheld(&loaded), 1);
    assert!(registry.loaded_by_id::<TextTestAsset>(during).is_none());
    assert_eq!(registry.loaded_count(), count - 1);
    assert_eq!(
        registry
            .loaded_by_id::<TextTestAsset>(before)
            .map(|asset| asset.ptr_id()),
        Some(held_before.ptr_id())
    );
    assert_eq!(
        registry
            .load_by_id::<TextTestAsset>(during)
            .unwrap()
            .read()
            .text,
        "during",
        "an evicted asset loads again"
    );
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn eviction_keeps_a_held_sub_asset_cached_and_loadable() {
    let root = temp_asset_root("calyx-evict-held-child");
    std::fs::write(root.join("parent.cxtestgated"), "plain").unwrap();
    let registries = registry_with_test_assets(&root);
    let registry = registries.assets.read();
    let parent = registry.asset_id("parent").unwrap();

    let record = AssetLoadRecord::start();
    registry.load_by_id::<GatedTestAsset>(parent).unwrap();
    let loaded = record.finish();
    let child = registry.asset_id("parent/Child").unwrap();
    assert_eq!(loaded.len(), 2, "the parent and the sub-asset it created");
    let held_child = registry.loaded_by_id::<TextTestAsset>(child).unwrap();

    assert_eq!(registry.evict_unheld(&loaded), 1);
    assert!(registry.loaded_by_id::<GatedTestAsset>(parent).is_none());
    assert_eq!(
        registry
            .load_by_id::<TextTestAsset>(child)
            .map(|asset| asset.ptr_id())
            .ok(),
        Some(held_child.ptr_id()),
        "the held sub-asset stays the cached one"
    );
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn eviction_drops_a_whole_family_when_a_held_parent_lost_a_sub_asset() {
    let root = temp_asset_root("calyx-evict-held-parent");
    std::fs::write(root.join("parent.cxtestgated"), "plain").unwrap();
    let registries = registry_with_test_assets(&root);
    let registry = registries.assets.read();
    let parent = registry.asset_id("parent").unwrap();

    let record = AssetLoadRecord::start();
    let held_parent = registry.load_by_id::<GatedTestAsset>(parent).unwrap();
    let loaded = record.finish();
    let child = registry.asset_id("parent/Child").unwrap();

    assert_eq!(registry.evict_unheld(&loaded), 2);
    assert!(registry.loaded_by_id::<GatedTestAsset>(parent).is_none());
    assert!(registry.loaded_by_id::<TextTestAsset>(child).is_none());
    assert!(
        registry.load_by_id::<TextTestAsset>(child).is_ok(),
        "the sub-asset loads again with a fresh parent"
    );
    assert_ne!(
        registry
            .loaded_by_id::<GatedTestAsset>(parent)
            .map(|asset| asset.ptr_id()),
        Some(held_parent.ptr_id())
    );
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn records_nest_and_stop_when_dropped() {
    let root = temp_asset_root("calyx-load-record-nesting");
    std::fs::write(root.join("a.cxtesttext"), "a").unwrap();
    std::fs::write(root.join("b.cxtesttext"), "b").unwrap();
    let registries = registry_with_test_assets(&root);
    let registry = registries.assets.read();
    let (a, b) = (
        registry.asset_id("a").unwrap(),
        registry.asset_id("b").unwrap(),
    );

    let outer = AssetLoadRecord::start();
    let inner = AssetLoadRecord::start();
    registry.load_by_id::<TextTestAsset>(a).unwrap();
    assert_eq!(inner.finish(), vec![a]);
    {
        let _dropped = AssetLoadRecord::start();
        registry.load_by_id::<TextTestAsset>(b).unwrap();
    }
    assert_eq!(outer.finish(), vec![a, b]);
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn load_in_flight_during_deletion_does_not_cache_the_deleted_asset() {
    let root = temp_asset_root("calyx-delete-during-load");
    let asset_path = root.join("gated.cxtestgated");
    std::fs::write(&asset_path, "gate").expect("failed to write test asset");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let id = {
        let registry = registries.assets.read();
        registry.register_asset_type::<GatedTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        registry
            .asset_id("gated")
            .expect("test asset should have metadata")
    };
    let gate = gated_test_asset_gate(&asset_path);
    let assets = registries.assets.clone();
    let loader = std::thread::spawn(move || {
        assets
            .read()
            .load_by_id::<GatedTestAsset>(id)
            .map(|_| ())
            .map_err(|err| err.to_string())
    });

    gate.0.wait();
    std::fs::remove_file(&asset_path).expect("failed to delete test asset");
    registries.assets.read().recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path),
    );
    gate.1.wait();
    let result = loader.join().expect("loader thread panicked");

    assert!(result.is_err(), "a load of a deleted source must fail");

    assert!(registries
        .assets
        .read()
        .loaded_by_id::<GatedTestAsset>(id)
        .is_none());
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn discarded_load_rolls_back_the_sub_assets_it_created() {
    let root = temp_asset_root("calyx-discarded-load-sub-assets");
    let asset_path = root.join("gated.cxtestgated");
    std::fs::write(&asset_path, "gate").expect("failed to write test asset");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let id = {
        let registry = registries.assets.read();
        registry.register_asset_type::<GatedTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        registry
            .asset_id("gated")
            .expect("test asset should have metadata")
    };
    let gate = gated_test_asset_gate(&asset_path);
    let assets = registries.assets.clone();
    let loader = std::thread::spawn(move || {
        assets
            .read()
            .load_by_id::<GatedTestAsset>(id)
            .map(|_| ())
            .map_err(|err| err.to_string())
    });

    gate.0.wait();
    std::fs::remove_file(&asset_path).expect("failed to delete test asset");
    registries.assets.read().recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path),
    );
    gate.1.wait();
    let _ = loader.join().expect("loader thread panicked");

    let registry = registries.assets.read();
    assert_eq!(registry.asset_id("gated/Child"), None);
    assert!(registry
        .asset_data()
        .meta
        .values()
        .all(|meta| meta.parent != Some(id)));
    assert!(!root.join("gated.meta").exists());
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn current_meta_files_keep_their_bytes_across_build_and_load() {
    let root = temp_asset_root("calyx-current-meta-bytes");
    let asset_path = root.join("plain.cxtestgated");
    let meta_path = root.join("plain.meta");
    std::fs::write(&asset_path, "plain").expect("failed to write test asset");
    // A first session creates the metadata, sub-asset included.
    {
        let registries = test_registries_with_assets(vec![root.clone()]);
        let registry = registries.assets.read();
        registry.register_asset_type::<GatedTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        registry
            .load_by_path::<GatedTestAsset>(&asset_path)
            .expect("test asset should load");
    }
    // A Windows checkout with `core.autocrlf=true` has CRLF line endings.
    let written = std::fs::read_to_string(&meta_path).expect("failed to read test meta");
    assert!(
        written.contains("Child"),
        "the meta should list the sub-asset"
    );
    let checked_out = written.replace("\r\n", "\n").replace('\n', "\r\n");
    std::fs::write(&meta_path, &checked_out).expect("failed to write test meta");

    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    registry.register_asset_type::<GatedTestAsset>();
    registry.build_meta().expect("failed to rebuild metadata");
    registry
        .load_by_path::<GatedTestAsset>(&asset_path)
        .expect("test asset should load");

    assert_eq!(
        std::fs::read_to_string(&meta_path).expect("failed to read test meta"),
        checked_out,
        "unchanged metadata must not be rewritten"
    );
    drop(registry);
    drop(registries);

    // A real change keeps the file's line endings, so only changed lines differ.
    let stale = checked_out.replace("\"name\": \"plain\"", "\"name\": \"stale\"");
    std::fs::write(&meta_path, &stale).expect("failed to write test meta");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    registry.register_asset_type::<GatedTestAsset>();
    registry.build_meta().expect("failed to rebuild metadata");
    registry
        .load_by_path::<GatedTestAsset>(&asset_path)
        .expect("test asset should load");
    assert_eq!(
        std::fs::read_to_string(&meta_path).expect("failed to read test meta"),
        checked_out,
        "changed metadata should be rewritten with the file's line endings"
    );
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn create_event_for_registered_file_keeps_its_generation() {
    let root = temp_asset_root("calyx-create-keeps-generation");
    let (asset_path, _, asset_id) = write_scene_with_meta(&root);
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let before = registry
        .asset_data()
        .generations
        .get(&asset_id)
        .map(|(generation, _)| *generation);
    assert!(before.is_some());

    registry.recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(asset_path),
    );

    assert_eq!(
        registry
            .asset_data()
            .generations
            .get(&asset_id)
            .map(|(generation, _)| *generation),
        before
    );
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn the_change_epoch_moves_on_a_load_and_a_file_event_but_not_on_reads() {
    let root = temp_asset_root("calyx-change-epoch");
    let (asset_path, _, asset_id) = write_scene_with_meta(&root);
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    let (epoch, files) = (registry.change_epoch(), registry.file_change_count());

    registry.reload_assets();
    let _ = registry.asset_id_from_path(&asset_path);
    let _ = registry.loaded_by_id::<Scene>(asset_id);
    assert_eq!(registry.change_epoch(), epoch, "reads change nothing");
    assert_eq!(registry.file_change_count(), files);

    let created = registry
        .create("epoch_graph".to_string(), AnimationGraph::default())
        .expect("the graph is created");
    let cached = registry.change_epoch();
    assert!(cached > epoch, "creating an asset caches it");
    registry
        .load_by_id::<AnimationGraph>(created.id())
        .expect("the graph is cached");
    assert_eq!(
        registry.change_epoch(),
        cached,
        "a cached load changes nothing"
    );

    registry.recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::Folder))
            .add_path(root.join("new_folder")),
    );
    assert!(
        registry.file_change_count() > files,
        "a created folder is a file change"
    );
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn the_meta_epoch_moves_on_a_create_a_rename_and_a_delete_but_not_on_a_load_or_an_eviction() {
    let root = temp_asset_root("calyx-meta-epoch");
    std::fs::write(root.join("kept.cxtesttext"), "kept").unwrap();
    let registries = registry_with_test_assets(&root);
    let registry = registries.assets.read();
    let kept = registry.asset_id("kept").unwrap();
    let epoch = registry.meta_epoch();

    let record = AssetLoadRecord::start();
    registry.load_by_id::<TextTestAsset>(kept).unwrap();
    let loaded = record.finish();
    assert_eq!(registry.evict_unheld(&loaded), 1);
    assert!(registry.change_epoch() > 0);
    assert_eq!(registry.meta_epoch(), epoch, "a load and an eviction");

    let created = root.join("created.cxtesttext");
    std::fs::write(&created, "created").unwrap();
    registry
        .register_file(&created)
        .expect("the file registers");
    let after_create = registry.meta_epoch();
    assert!(after_create > epoch, "a create moves it");

    let renamed = root.join("renamed.cxtesttext");
    std::fs::rename(&created, &renamed).unwrap();
    std::fs::rename(
        created.with_extension("meta"),
        renamed.with_extension("meta"),
    )
    .unwrap();
    registry.recv_notify_event(
        Event::new(EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::To,
        )))
        .add_path(renamed.clone()),
    );
    let after_rename = registry.meta_epoch();
    assert!(after_rename > after_create, "a rename moves it");
    assert!(registry.asset_id_from_path(&renamed).is_some());

    std::fs::remove_file(&renamed).unwrap();
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(renamed),
    );
    assert!(registry.meta_epoch() > after_rename, "a delete moves it");
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn reload_failing_after_deletion_records_no_reload_error() {
    let root = temp_asset_root("calyx-failed-reload-after-delete");
    let asset_path = root.join("gated.cxtestgated");
    std::fs::write(&asset_path, "plain").expect("failed to write test asset");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let id = {
        let registry = registries.assets.read();
        registry.register_asset_type::<GatedTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        let id = registry
            .asset_id("gated")
            .expect("test asset should have metadata");
        registry
            .load_by_id::<GatedTestAsset>(id)
            .expect("test asset should load");
        id
    };
    std::fs::write(&asset_path, "gate fail").expect("failed to rewrite test asset");
    registries
        .assets
        .read()
        .asset_data_mut()
        .dirty
        .insert(id, Instant::now() - Duration::from_secs(1));
    let gate = gated_test_asset_gate(&asset_path);
    let assets = registries.assets.clone();
    let reloader = std::thread::spawn(move || assets.read().reload_assets());

    gate.0.wait();
    std::fs::remove_file(&asset_path).expect("failed to delete test asset");
    registries.assets.read().recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path),
    );
    gate.1.wait();
    reloader.join().expect("reloader thread panicked");

    assert!(registries.assets.read().reload_errors().is_empty());
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn load_spanning_delete_and_recreate_does_not_cache_the_old_file() {
    let root = temp_asset_root("calyx-recreate-during-load");
    let asset_path = root.join("gated.cxtestgated");
    std::fs::write(&asset_path, "gate").expect("failed to write test asset");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let id = {
        let registry = registries.assets.read();
        registry.register_asset_type::<GatedTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        registry
            .asset_id("gated")
            .expect("test asset should have metadata")
    };
    let gate = gated_test_asset_gate(&asset_path);
    let assets = registries.assets.clone();
    let loader = std::thread::spawn(move || {
        assets
            .read()
            .load_by_id::<GatedTestAsset>(id)
            .map(|_| ())
            .map_err(|err| err.to_string())
    });

    gate.0.wait();
    std::fs::remove_file(&asset_path).expect("failed to delete test asset");
    registries.assets.read().recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path.clone()),
    );
    std::fs::write(&asset_path, "gate").expect("failed to recreate test asset");
    registries.assets.read().recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(asset_path.clone()),
    );
    settle(&registries.assets.read());
    assert_eq!(registries.assets.read().asset_id("gated"), Some(id));
    gate.1.wait();
    let _ = loader.join().expect("loader thread panicked");

    assert!(registries
        .assets
        .read()
        .loaded_by_id::<GatedTestAsset>(id)
        .is_none());
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn reload_by_id_updates_existing_reference_in_place() {
    let root = temp_asset_root("calyx-reload-in-place");
    let asset_path = root.join("note.cxtesttext");
    std::fs::write(&asset_path, "first").expect("failed to write test asset");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    registry.register_asset_type::<TextTestAsset>();
    registry.build_meta().expect("failed to rebuild metadata");
    let id = registry
        .asset_id("note")
        .expect("test asset should have metadata");

    let held = registry
        .load_by_id::<TextTestAsset>(id)
        .expect("test asset should load");
    std::fs::write(&asset_path, "second").expect("failed to rewrite test asset");
    let reloaded = registry
        .reload_by_id::<TextTestAsset>(id)
        .expect("test asset should reload");

    assert_eq!(held.ptr_id(), reloaded.ptr_id());
    assert_eq!(held.read().text, "second");
    let cached = registry
        .loaded_by_id::<TextTestAsset>(id)
        .expect("reloaded asset should stay cached");
    assert_eq!(cached.ptr_id(), held.ptr_id());
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn id_moving_to_another_source_loads_the_new_source() {
    let first_root = temp_asset_root("calyx-id-moves-first-root");
    let second_root = temp_asset_root("calyx-id-moves-second-root");
    std::fs::write(first_root.join("note.cxtesttext"), "first")
        .expect("failed to write first asset");
    let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
    let registry = registries.assets.read();
    registry.register_asset_type::<TextTestAsset>();
    registry.build_meta().expect("failed to rebuild metadata");
    let id = registry
        .asset_id("note")
        .expect("first asset is registered");
    let held = registry
        .load_by_id::<TextTestAsset>(id)
        .expect("first asset should load");
    assert_eq!(held.read().text, "first");

    let second_path = second_root.join("note.cxtesttext");
    std::fs::write(&second_path, "second").expect("failed to write second asset");
    registry.recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(second_path),
    );
    settle(&registry);

    assert_eq!(registry.asset_id("note"), Some(id));
    let loaded = registry
        .load_by_id::<TextTestAsset>(id)
        .expect("second asset should load");
    assert_eq!(loaded.read().text, "second");
    drop(registry);
    std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
    std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
}

#[test]
fn id_moving_to_another_source_drops_the_old_sources_sub_assets() {
    let first_root = temp_asset_root("calyx-id-moves-children-first-root");
    let second_root = temp_asset_root("calyx-id-moves-children-second-root");
    let parent_id = Uuid::new_v4();
    let (body_id, arm_id) = (Uuid::new_v4(), Uuid::new_v4());
    let write_parent = |root: &Path, child_id: Uuid, child_name: &str| {
        std::fs::write(root.join("parent.fbx"), b"").expect("failed to write parent asset");
        std::fs::write(
            root.join("parent.meta"),
            serde_json::to_vec_pretty(&json!({
                "main": {
                    "id": parent_id,
                    "name": "parent",
                    "display_name": "parent",
                    "type_uuid": Prefab::type_uuid(),
                },
                "inner": [{
                    "id": child_id,
                    "name": child_name,
                    "display_name": child_name,
                    "type_uuid": Mesh::type_uuid(),
                }]
            }))
            .expect("failed to encode test meta"),
        )
        .expect("failed to write parent meta");
    };
    write_parent(&first_root, body_id, "Body");
    let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
    let registry = registries.assets.read();
    assert_eq!(registry.asset_id("parent/Body"), Some(body_id));

    write_parent(&second_root, arm_id, "Arm");
    registry.recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::File))
            .add_path(second_root.join("parent.fbx")),
    );

    assert_eq!(registry.asset_id("parent/Arm"), Some(arm_id));
    assert_eq!(registry.asset_id("parent/Body"), None);
    assert!(registry.asset_meta_from_id(body_id).is_none());
    drop(registry);
    std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
    std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
}

#[test]
fn id_moving_to_another_source_clears_its_reload_error() {
    let first_root = temp_asset_root("calyx-id-moves-reload-error-first-root");
    let second_root = temp_asset_root("calyx-id-moves-reload-error-second-root");
    let first_path = first_root.join("note.cxtesttext");
    std::fs::write(&first_path, "first").expect("failed to write first asset");
    let registries = test_registries_with_assets(vec![first_root.clone(), second_root.clone()]);
    let registry = registries.assets.read();
    registry.register_asset_type::<TextTestAsset>();
    registry.build_meta().expect("failed to rebuild metadata");
    let id = registry
        .asset_id("note")
        .expect("first asset is registered");
    let generation = registry
        .asset_data()
        .generations
        .get(&id)
        .map(|(generation, _)| *generation);
    registry.set_current_reload_error(
        generation,
        AssetReloadError {
            id,
            path: registry
                .asset_meta_from_id(id)
                .and_then(|meta| meta.path)
                .expect("first asset has a source path"),
            error: AssetError::LoadError.with_source("test reload failure"),
        },
    );
    assert_eq!(registry.reload_errors().len(), 1);

    let second_path = second_root.join("note.cxtesttext");
    std::fs::write(&second_path, "second").expect("failed to write second asset");
    registry.recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(second_path),
    );
    settle(&registry);

    assert_eq!(registry.asset_id("note"), Some(id));
    assert!(registry.reload_errors().is_empty());
    drop(registry);
    std::fs::remove_dir_all(first_root).expect("failed to remove first temp asset root");
    std::fs::remove_dir_all(second_root).expect("failed to remove second temp asset root");
}

#[test]
fn asset_moved_with_its_meta_drops_its_old_names() {
    let root = temp_asset_root("calyx-moved-with-meta");
    let (old_path, old_meta_path, asset_id) = write_scene_with_meta(&root);
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    assert_eq!(registry.asset_id("scene"), Some(asset_id));

    std::fs::create_dir_all(root.join("moved")).expect("failed to create target folder");
    let new_path = root.join("moved").join("scene.cxscene");
    std::fs::rename(&old_path, &new_path).expect("failed to move asset");
    std::fs::rename(&old_meta_path, root.join("moved").join("scene.meta"))
        .expect("failed to move asset metadata");
    registry.recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(new_path),
    );
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(old_path),
    );

    assert_eq!(registry.asset_id("moved/scene"), Some(asset_id));
    assert_eq!(registry.asset_id("scene"), None);
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn deleted_asset_is_evicted_and_recreated_file_loads_fresh() {
    let root = temp_asset_root("calyx-delete-evicts-cache");
    let asset_path = root.join("note.cxtesttext");
    std::fs::write(&asset_path, "first").expect("failed to write test asset");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();
    registry.register_asset_type::<TextTestAsset>();
    registry.build_meta().expect("failed to rebuild metadata");
    let id = registry
        .asset_id("note")
        .expect("test asset should have metadata");
    let held = registry
        .load_by_id::<TextTestAsset>(id)
        .expect("test asset should load");

    std::fs::remove_file(&asset_path).expect("failed to delete test asset");
    registry.recv_notify_event(
        Event::new(EventKind::Remove(notify::event::RemoveKind::File)).add_path(asset_path.clone()),
    );

    assert!(registry.loaded_by_id::<TextTestAsset>(id).is_none());
    std::fs::write(&asset_path, "second").expect("failed to recreate test asset");
    registry.recv_notify_event(
        Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(asset_path.clone()),
    );
    settle(&registry);
    let recreated_id = registry
        .asset_id("note")
        .expect("recreated asset should have metadata");
    let recreated = registry
        .load_by_id::<TextTestAsset>(recreated_id)
        .expect("recreated asset should load");
    assert_eq!(recreated.read().text, "second");
    assert_eq!(held.read().text, "first");
    drop(registry);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn concurrent_loads_of_one_id_share_one_reference() {
    let root = temp_asset_root("calyx-concurrent-load");
    std::fs::write(root.join("slow.cxtestslow"), "").expect("failed to write test asset");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let id = {
        let registry = registries.assets.read();
        registry.register_asset_type::<SlowTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        registry
            .asset_id("slow")
            .expect("test asset should have metadata")
    };
    SLOW_TEST_ASSET_LOADS.store(0, std::sync::atomic::Ordering::SeqCst);

    let barrier = Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let assets = registries.assets.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                assets
                    .read()
                    .load_by_id::<SlowTestAsset>(id)
                    .expect("test asset should load")
                    .ptr_id()
            })
        })
        .collect::<Vec<_>>();
    let ptrs = handles
        .into_iter()
        .map(|handle| handle.join().expect("loader thread panicked"))
        .collect::<Vec<_>>();

    assert_eq!(ptrs[0], ptrs[1]);
    assert_eq!(
        SLOW_TEST_ASSET_LOADS.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

/// Asset whose file names another asset of this type that it loads, slowly
/// enough for two threads to each hold one slot of a cycle.
struct CyclicTestAsset;

impl TypeUuid for CyclicTestAsset {
    fn type_uuid() -> Uuid {
        Uuid::from_u128(0x2c7a_94e1_5f30_4b8d_a1c6_0e9f_37b2_d815)
    }
}

impl Asset for CyclicTestAsset {
    fn file_extensions() -> &'static [&'static str] {
        &["cxtestcycle"]
    }

    fn from_file(
        assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError> {
        let other = std::fs::read_to_string(path)
            .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?;
        std::thread::sleep(Duration::from_millis(200));
        assets
            .registries
            .assets
            .read()
            .load::<CyclicTestAsset>(other.trim())?;
        Ok(LoadedAsset::new(Self))
    }
}

#[test]
fn concurrent_mutually_dependent_loads_fail_instead_of_deadlocking() {
    let root = temp_asset_root("calyx-concurrent-cycle");
    std::fs::write(root.join("a.cxtestcycle"), "b").expect("failed to write test asset");
    std::fs::write(root.join("b.cxtestcycle"), "a").expect("failed to write test asset");
    let registries = test_registries_with_assets(vec![root.clone()]);
    {
        let registry = registries.assets.read();
        registry.register_asset_type::<CyclicTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
    }

    let barrier = Arc::new(std::sync::Barrier::new(2));
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    for name in ["a", "b"] {
        let assets = registries.assets.clone();
        let barrier = barrier.clone();
        let done_tx = done_tx.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let result = assets.read().load::<CyclicTestAsset>(name).map(|_| ());
            let _ = done_tx.send(result);
        });
    }
    drop(done_tx);

    for _ in 0..2 {
        let result = done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("concurrent cyclic loads deadlocked");
        assert!(result.is_err());
    }
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn sub_asset_meta_normalizes_legacy_full_child_names() {
    let root = temp_asset_root("calyx-legacy-subasset-meta");
    let (_, child_id) = write_parent_with_child_meta(&root, "parent/Body");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let registry = registries.assets.read();

    assert_eq!(registry.asset_id("parent/Body"), Some(child_id));
    assert_eq!(
        registry.asset_meta_from_id(child_id).map(|meta| meta.name),
        Some("Body".into())
    );
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

/// Registers [`GatedTestAsset`] over a temp root holding `gated.cxtestgated`
/// with `content`, returning the root, the file path, the registries and
/// the asset id.
fn gated_background_fixture(
    prefix: &str,
    content: &str,
) -> (PathBuf, PathBuf, ReadOnlyRegistryContext, Uuid) {
    let root = temp_asset_root(prefix);
    let asset_path = root.join("gated.cxtestgated");
    std::fs::write(&asset_path, content).expect("failed to write test asset");
    let registries = test_registries_with_assets(vec![root.clone()]);
    let id = {
        let registry = registries.assets.read();
        registry.register_asset_type::<GatedTestAsset>();
        registry.build_meta().expect("failed to rebuild metadata");
        registry
            .asset_id("gated")
            .expect("test asset should have metadata")
    };
    (root, asset_path, registries, id)
}

/// Polls the background load of `id` until it stops reporting `Loading`.
fn poll_until_settled<A: Asset + TypeUuid>(
    registries: &ReadOnlyRegistryContext,
    id: Uuid,
) -> BackgroundLoad<A> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = registries.assets.read().load_by_id_in_background::<A>(id);
        if !matches!(status, BackgroundLoad::Loading) {
            return status;
        }
        assert!(Instant::now() < deadline, "background load never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn background_load_returns_at_once_and_loads_once_on_another_thread() {
    let (root, asset_path, registries, id) =
        gated_background_fixture("calyx-background-load", "gate");
    let gate = gated_test_asset_gate(&asset_path);

    // The load blocks on the gate, so a blocking call would hang here.
    assert!(matches!(
        registries
            .assets
            .read()
            .load_by_id_in_background::<GatedTestAsset>(id),
        BackgroundLoad::Loading
    ));
    gate.0.wait();
    for _ in 0..3 {
        assert!(matches!(
            registries
                .assets
                .read()
                .load_by_id_in_background::<GatedTestAsset>(id),
            BackgroundLoad::Loading
        ));
    }
    gate.1.wait();

    let BackgroundLoad::Loaded(loaded) = poll_until_settled::<GatedTestAsset>(&registries, id)
    else {
        panic!("background load should have succeeded");
    };
    let cached = registries
        .assets
        .read()
        .load_by_id::<GatedTestAsset>(id)
        .expect("asset should be cached");
    assert_eq!(loaded.ptr_id(), cached.ptr_id());
    assert_eq!(gated_test_asset_loads(&asset_path), 1);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn failed_background_load_is_kept_until_its_source_changes() {
    let (root, asset_path, registries, id) =
        gated_background_fixture("calyx-background-load-failure", "gate fail");
    let gate = gated_test_asset_gate(&asset_path);

    assert!(matches!(
        registries
            .assets
            .read()
            .load_by_id_in_background::<GatedTestAsset>(id),
        BackgroundLoad::Loading
    ));
    gate.0.wait();
    gate.1.wait();
    assert!(matches!(
        poll_until_settled::<GatedTestAsset>(&registries, id),
        BackgroundLoad::Failed(_)
    ));
    // A retry would start another gated load and report `Loading`.
    for _ in 0..3 {
        assert!(matches!(
            registries
                .assets
                .read()
                .load_by_id_in_background::<GatedTestAsset>(id),
            BackgroundLoad::Failed(_)
        ));
    }
    assert_eq!(gated_test_asset_loads(&asset_path), 1);

    std::fs::write(&asset_path, "fixed").expect("failed to rewrite test asset");
    registries.assets.read().recv_notify_event(
        Event::new(EventKind::Modify(notify::event::ModifyKind::Any)).add_path(asset_path.clone()),
    );
    assert!(matches!(
        poll_until_settled::<GatedTestAsset>(&registries, id),
        BackgroundLoad::Loaded(_)
    ));
    assert_eq!(gated_test_asset_loads(&asset_path), 2);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn source_change_during_a_background_load_retries_its_failure() {
    let (root, asset_path, registries, id) =
        gated_background_fixture("calyx-background-load-change", "gate fail");
    let gate = gated_test_asset_gate(&asset_path);

    assert!(matches!(
        registries
            .assets
            .read()
            .load_by_id_in_background::<GatedTestAsset>(id),
        BackgroundLoad::Loading
    ));
    // The load has read the failing source; fix it before it finishes.
    gate.0.wait();
    std::fs::write(&asset_path, "fixed").expect("failed to rewrite test asset");
    registries.assets.read().recv_notify_event(
        Event::new(EventKind::Modify(notify::event::ModifyKind::Any)).add_path(asset_path.clone()),
    );
    gate.1.wait();

    assert!(matches!(
        poll_until_settled::<GatedTestAsset>(&registries, id),
        BackgroundLoad::Loaded(_)
    ));
    assert_eq!(gated_test_asset_loads(&asset_path), 2);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn read_access_events_keep_a_background_load_failure() {
    let (root, asset_path, registries, id) =
        gated_background_fixture("calyx-background-load-access", "gate fail");
    let gate = gated_test_asset_gate(&asset_path);
    let open_event = || {
        Event::new(EventKind::Access(notify::event::AccessKind::Open(
            notify::event::AccessMode::Any,
        )))
        .add_path(asset_path.clone())
    };

    assert!(matches!(
        registries
            .assets
            .read()
            .load_by_id_in_background::<GatedTestAsset>(id),
        BackgroundLoad::Loading
    ));
    // inotify reports the loader opening its own source as an access.
    gate.0.wait();
    registries.assets.read().recv_notify_event(open_event());
    gate.1.wait();
    assert!(matches!(
        poll_until_settled::<GatedTestAsset>(&registries, id),
        BackgroundLoad::Failed(_)
    ));

    registries.assets.read().recv_notify_event(open_event());
    // A retry would start another gated load and report `Loading`.
    assert!(matches!(
        registries
            .assets
            .read()
            .load_by_id_in_background::<GatedTestAsset>(id),
        BackgroundLoad::Failed(_)
    ));
    assert_eq!(gated_test_asset_loads(&asset_path), 1);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn source_change_during_a_successful_background_load_reloads_it() {
    let (root, asset_path, registries, id) =
        gated_background_fixture("calyx-background-load-replace", "gate");
    let gate = gated_test_asset_gate(&asset_path);

    assert!(matches!(
        registries
            .assets
            .read()
            .load_by_id_in_background::<GatedTestAsset>(id),
        BackgroundLoad::Loading
    ));
    // The load has read the old source; replace it before it finishes
    // and let the debounce consume the dirty mark while it is uncached.
    gate.0.wait();
    std::fs::write(&asset_path, "replaced").expect("failed to rewrite test asset");
    registries.assets.read().recv_notify_event(
        Event::new(EventKind::Modify(notify::event::ModifyKind::Any)).add_path(asset_path.clone()),
    );
    std::thread::sleep(HOT_RELOAD_DEBOUNCE * 2);
    registries.assets.read().reload_assets();
    gate.1.wait();
    assert!(matches!(
        poll_until_settled::<GatedTestAsset>(&registries, id),
        BackgroundLoad::Loaded(_)
    ));
    assert_eq!(gated_test_asset_loads(&asset_path), 1);

    // The stale load must leave a pending reload of the replacement.
    std::thread::sleep(HOT_RELOAD_DEBOUNCE * 2);
    registries.assets.read().reload_assets();
    assert_eq!(gated_test_asset_loads(&asset_path), 2);
    std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
}
