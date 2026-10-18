#[cfg(test)]
mod tests {
    use crate::assets::animation::Animation;
    use crate::assets::error::AssetErrorKind;
    use crate::assets::mesh::Mesh;
    use crate::assets::texture::Texture;
    use crate::assets::Asset;
    use crate::test_utils::{test_game_context, test_registries_with_assets};
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

    /// Builds a file name that is not valid UTF-8 on the current platform.
    fn non_utf8_file_name(stem: &str, ext: &str) -> std::ffi::OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let mut bytes = stem.as_bytes().to_vec();
            bytes.push(0xff);
            bytes.extend_from_slice(format!(".{ext}").as_bytes());
            std::ffi::OsString::from_vec(bytes)
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            let mut wide: Vec<u16> = stem.encode_utf16().collect();
            wide.push(0xD800);
            wide.extend(format!(".{ext}").encode_utf16());
            std::ffi::OsString::from_wide(&wide)
        }
    }

    #[test]
    fn texture_from_file_reports_non_utf8_file_name() {
        let context = test_game_context().assets.lock_read();
        let dir = std::env::temp_dir().join(format!("texture-name-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("failed to create temp directory");
        let path = dir.join(non_utf8_file_name("texture", "png"));
        image::RgbaImage::new(1, 1)
            .save_with_format(&path, image::ImageFormat::Png)
            .expect("failed to write temp texture");

        let result = Texture::from_file(&context, &path);
        fs::remove_dir_all(&dir).expect("failed to remove temp directory");

        let Err(err) = result else {
            panic!("non-UTF-8 file name should fail to load");
        };
        assert_eq!(err.kind, AssetErrorKind::LoadError);
        assert_eq!(err.asset_type, Some(Texture::asset_name()));
        assert_eq!(err.path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn mesh_from_file_reports_non_utf8_path() {
        let context = test_game_context().assets.lock_read();
        let dir = std::env::temp_dir().join(format!("mesh-path-{}", Uuid::new_v4()));
        let path = dir.join(non_utf8_file_name("mesh", "obj"));

        let result = Mesh::from_file(&context, &path);

        let Err(err) = result else {
            panic!("non-UTF-8 path should fail to load");
        };
        assert_eq!(err.kind, AssetErrorKind::LoadError);
        assert_eq!(err.asset_type, Some(Mesh::asset_name()));
        assert_eq!(err.path.as_deref(), Some(path.as_path()));
    }
}
