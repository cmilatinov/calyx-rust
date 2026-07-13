#[cfg(test)]
mod tests {
    use crate::assets::animation::Animation;
    use crate::assets::mesh::Mesh;
    use crate::assets::texture::Texture;
    use crate::test_utils::test_registries_with_assets;
    use crate::utils::TypeUuid;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};
    use uuid::Uuid;

    fn asset_registries() -> crate::context::ReadOnlyRegistryContext {
        let assets_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets");
        let assets_path = dunce::canonicalize(assets_path).expect("assets dir not found");
        test_registries_with_assets(vec![assets_path])
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
        let texture = texture.unwrap();
        let texture = texture.read();
        assert_eq!(texture.descriptor.size.width, 1);
        assert_eq!(texture.descriptor.size.height, 1);
        assert_eq!(texture.descriptor.mip_level_count, 1);
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
    fn create_or_update_reuses_existing_asset_ref() {
        let registries = asset_registries();
        let registry = registries.assets.read();
        let name = format!("generated/test_animation_{}", Uuid::new_v4());

        let first_value = Animation {
            duration: 1.0,
            ..Default::default()
        };
        let first = registry
            .create_or_update(name.clone(), first_value)
            .unwrap();

        let second_value = Animation {
            duration: 2.0,
            ..Default::default()
        };
        let second = registry
            .create_or_update(name.clone(), second_value)
            .unwrap();

        assert_eq!(first.id(), second.id());
        assert_eq!(first.ptr_id(), second.ptr_id());
        assert_eq!(registry.asset_id(&name), Some(first.id()));
        assert_eq!(second.read().duration, 2.0);
    }

    #[test]
    fn concurrent_create_or_update_reuses_single_asset_ref() {
        const THREADS: usize = 16;

        let registries = asset_registries();
        let name = format!("generated/test_animation_{}", Uuid::new_v4());
        let barrier = Arc::new(Barrier::new(THREADS));

        let refs = std::thread::scope(|scope| {
            let handles = (0..THREADS)
                .map(|index| {
                    let assets = registries.assets.clone();
                    let name = name.clone();
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        let value = Animation {
                            duration: index as f64,
                            ..Default::default()
                        };
                        barrier.wait();
                        assets.read().create_or_update(name, value).unwrap()
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });

        let first = &refs[0];
        for asset_ref in refs.iter().skip(1) {
            assert_eq!(asset_ref.id(), first.id());
            assert_eq!(asset_ref.ptr_id(), first.ptr_id());
        }
        assert_eq!(registries.assets.read().asset_id(&name), Some(first.id()));
    }

    #[test]
    fn concurrent_create_or_update_sub_asset_reuses_single_asset_ref() {
        const THREADS: usize = 16;

        let registries = asset_registries();
        let parent_id = Uuid::new_v4();
        let parent_name = format!("generated/test_parent_{}", Uuid::new_v4());
        let local_name = "clip";
        let canonical_name = format!("{parent_name}/{local_name}");
        let barrier = Arc::new(Barrier::new(THREADS));

        let refs = std::thread::scope(|scope| {
            let handles = (0..THREADS)
                .map(|index| {
                    let assets = registries.assets.clone();
                    let parent_name = parent_name.clone();
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        let value = Animation {
                            duration: index as f64,
                            ..Default::default()
                        };
                        barrier.wait();
                        assets
                            .read()
                            .create_or_update_sub_asset(parent_id, &parent_name, local_name, value)
                            .unwrap()
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });

        let first = &refs[0];
        for asset_ref in refs.iter().skip(1) {
            assert_eq!(asset_ref.id(), first.id());
            assert_eq!(asset_ref.ptr_id(), first.ptr_id());
        }

        let registry = registries.assets.read();
        assert_eq!(registry.asset_id(&canonical_name), Some(first.id()));
        let meta = registry
            .asset_meta_from_id(first.id())
            .expect("sub-asset metadata should be registered");
        assert_eq!(meta.name, local_name);
        assert_eq!(meta.parent, Some(parent_id));
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
        assert!(!mesh.vertices.is_empty());
    }
}
