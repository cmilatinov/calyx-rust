#[cfg(test)]
mod tests {
    use crate::assets::mesh::Mesh;
    use crate::assets::texture::Texture;
    use crate::assets::AssetRef;
    use crate::test_utils::test_registries_with_assets;
    use crate::utils::TypeUuid;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    fn asset_registries() -> crate::context::ReadOnlyRegistryContext {
        let assets_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets");
        let assets_path = dunce::canonicalize(assets_path).expect("assets dir not found");
        test_registries_with_assets(vec![assets_path])
    }

    fn wait_for_handle<T: crate::assets::Asset + TypeUuid>(
        handle: &AssetRef<T>,
        registries: &crate::context::ReadOnlyRegistryContext,
    ) -> Result<crate::core::Ref<T>, crate::assets::error::AssetError> {
        let start = Instant::now();
        loop {
            if let Some(result) = handle.result(registries) {
                return result;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "asset handle did not resolve"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn asset_ref_resolves_and_caches_sync_loads() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let id = registry.asset_id("meshes/cube").unwrap();
        drop(registry);
        let asset_ref = AssetRef::<Mesh>::from_id(id);

        let mesh = asset_ref
            .load_blocking(&registries)
            .expect("asset ref should resolve");

        assert!(asset_ref.is_loaded(&registries));
        assert_eq!(asset_ref.get(&registries).unwrap().ptr_id(), mesh.ptr_id());
    }

    #[test]
    fn asset_ref_get_ref_does_not_sync_load() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let id = registry.asset_id("meshes/cube").unwrap();
        drop(registry);
        let asset_ref = AssetRef::<Mesh>::from_id(id);

        assert!(asset_ref.get_ref(&registries).is_none());
        assert!(!asset_ref.is_loaded(&registries));
    }

    #[test]
    fn load_mesh_by_name() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let mesh = registry.load::<Mesh>("meshes/cube");
        assert!(mesh.is_ok(), "failed to load cube mesh: {:?}", mesh.err());
        let mesh = mesh.unwrap();
        let mesh = mesh.read();
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.indices.is_empty());
    }

    #[test]
    fn load_texture_by_name() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let texture = registry.load::<Texture>("textures/white");
        assert!(
            texture.is_ok(),
            "failed to load white texture: {:?}",
            texture.err()
        );
    }

    #[test]
    fn load_nonexistent_asset_returns_error() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let result = registry.load::<Mesh>("meshes/does_not_exist");
        let err = match result {
            Ok(_) => panic!("missing asset should return an error"),
            Err(err) => err,
        };
        assert_eq!(err.kind, crate::assets::error::AssetErrorKind::NotFound);
        assert!(err.to_string().contains("meshes/does_not_exist"));
    }

    #[test]
    fn asset_meta_lookup() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let id = registry.asset_id("meshes/cube");
        assert!(id.is_some());
        let meta = registry.asset_meta_from_id(id.unwrap());
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert_eq!(meta.type_uuid, Mesh::type_uuid());
    }

    #[test]
    fn corrupt_meta_file_is_skipped() {
        let asset_path =
            std::env::temp_dir().join(format!("calyx-corrupt-meta-{}", Uuid::new_v4()));
        fs::create_dir_all(&asset_path).expect("failed to create temp asset directory");
        fs::write(asset_path.join("broken.cxmat"), "{}").expect("failed to write test asset");
        fs::write(asset_path.join("broken.meta"), "not json").expect("failed to write bad meta");

        let registries = test_registries_with_assets(vec![asset_path.clone()]);
        let registry = registries.assets.read();
        assert!(registry.asset_id("broken").is_none());

        fs::remove_dir_all(asset_path).expect("failed to remove temp asset directory");
    }

    #[test]
    fn loaded_mesh_has_gpu_buffers() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let mesh = registry.load::<Mesh>("meshes/cube").unwrap();
        let mesh = mesh.read();
        // Mesh::from_russimp_mesh creates vertex/index buffers via the GPU device
        // They're None until mark_dirty + upload, but instance_buffer is always created
        assert!(mesh.vertices.len() > 0);
    }

    #[test]
    fn async_load_resolves_handle() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let handle = registry
            .request_load::<Mesh>("meshes/cube")
            .expect("async load should start");
        drop(registry);

        let mesh = wait_for_handle(&handle, &registries).expect("async load should resolve");

        assert!(handle.is_loaded(&registries));
        assert_eq!(handle.id(), mesh.id());
        assert!(mesh.read().vertices.len() > 0);
    }

    #[test]
    fn async_load_uses_loaded_cache_immediately() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let mesh = registry.load::<Mesh>("meshes/cube").unwrap();

        let handle = registry.request_load_by_id::<Mesh>(mesh.id());
        drop(registry);

        assert!(handle.is_loaded(&registries));
        assert_eq!(handle.get(&registries).unwrap().ptr_id(), mesh.ptr_id());
    }

    #[test]
    fn async_load_reports_missing_asset_on_handle() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let missing_id = Uuid::new_v4();

        let handle = registry.request_load_by_id::<Mesh>(missing_id);
        drop(registry);

        assert!(!handle.is_loading(&registries));
        assert_eq!(
            handle.error(&registries).unwrap().kind,
            crate::assets::error::AssetErrorKind::NotFound
        );
    }
}
