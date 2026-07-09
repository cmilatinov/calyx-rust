use super::cache::ThumbnailCache;
use super::camera_fit::*;
use super::generator::SkyboxFrontFaceDownscaler;
use super::pipeline::*;
use super::service::ThumbnailService;
use super::*;

fn request(source_version: u64) -> ThumbnailRequest {
    ThumbnailRequest {
        asset_id: Uuid::from_u128(1),
        asset_type: Uuid::from_u128(2),
        source_path: Some(PathBuf::from("assets/texture.png")),
        source_version,
    }
}

#[test]
fn render_settings_sanitize_size_margin_and_cache_key() {
    let settings = ThumbnailRenderSettings::with_size_and_frame_margin(1, f32::NAN);
    assert_eq!(settings.size_px, THUMBNAIL_MIN_SIZE);
    assert_eq!(settings.frame_margin, THUMBNAIL_DEFAULT_FRAME_MARGIN);
    assert_eq!(settings.cache_key(), "frame-margin-25");

    let settings = ThumbnailRenderSettings::with_size_and_frame_margin(u32::MAX, f32::INFINITY);
    assert_eq!(settings.size_px, THUMBNAIL_MAX_SIZE);
    assert_eq!(settings.frame_margin, THUMBNAIL_DEFAULT_FRAME_MARGIN);

    let settings = ThumbnailRenderSettings::with_size_and_frame_margin(256, -1.0);
    assert_eq!(settings.size_px, 256);
    assert_eq!(settings.frame_margin, THUMBNAIL_MIN_FRAME_MARGIN);
    assert_eq!(settings.cache_key(), "frame-margin-0");

    let settings = ThumbnailRenderSettings::with_size_and_frame_margin(256, 1.0);
    assert_eq!(settings.frame_margin, THUMBNAIL_MAX_FRAME_MARGIN);
    assert_eq!(settings.cache_key(), "frame-margin-450");
}

#[test]
fn thumbnail_request_key_uses_asset_id_and_source_version_only() {
    let request = ThumbnailRequest {
        asset_id: Uuid::from_u128(0xabc),
        asset_type: Mesh::type_uuid(),
        source_path: Some(PathBuf::from("assets/meshes/tank.obj")),
        source_version: 42,
    };
    let same_key_different_type_and_path = ThumbnailRequest {
        asset_type: Texture::type_uuid(),
        source_path: Some(PathBuf::from("assets/textures/tank.png")),
        ..request.clone()
    };

    assert_eq!(
        request.key(),
        ThumbnailKey {
            asset_id: request.asset_id,
            source_version: request.source_version
        }
    );
    assert_eq!(request.key(), same_key_different_type_and_path.key());
}

#[test]
fn source_version_is_zero_for_missing_sources_and_changes_for_file_size() {
    let path = std::env::temp_dir().join(format!(
        "calyx-thumbnail-source-version-{}.tmp",
        Uuid::new_v4()
    ));
    assert_eq!(source_version(None), 0);
    assert_eq!(source_version(Some(&path)), 0);

    fs::write(&path, b"abc").expect("failed to write temp thumbnail source");
    let first = source_version(Some(&path));
    fs::write(&path, b"abcdef").expect("failed to rewrite temp thumbnail source");
    let second = source_version(Some(&path));
    let _ = fs::remove_file(&path);

    assert_ne!(first, 0);
    assert_ne!(second, 0);
    assert_ne!(first, second);
}

#[test]
fn thumbnail_request_source_version_includes_referenced_asset_sources() {
    let root = std::env::temp_dir().join(format!(
        "calyx-thumbnail-dependency-version-{}",
        Uuid::new_v4()
    ));
    let texture_dir = root.join("textures");
    let material_dir = root.join("materials");
    fs::create_dir_all(&texture_dir).expect("failed to create texture dir");
    fs::create_dir_all(&material_dir).expect("failed to create material dir");
    let texture_path = texture_dir.join("source.png");
    let material_path = material_dir.join("mat.cxmat");
    fs::write(&texture_path, b"a").expect("failed to write texture source");
    fs::write(&material_path, "{}").expect("failed to write placeholder material source");

    let context = engine::test_support::test_asset_context_with_assets(vec![root.clone()]);
    let registry = context.registries.assets.read();
    let texture_id = registry
        .asset_id("textures/source")
        .expect("texture asset should be registered");
    fs::write(
        &material_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "shader": Uuid::nil(),
            "variables": [
                {
                    "group": 3,
                    "binding": 0,
                    "offset": null,
                    "name": "texture_diffuse",
                    "span": null,
                    "value": {
                        "Texture2D": {
                            "Asset": texture_id
                        }
                    }
                }
            ]
        }))
        .expect("failed to encode material source"),
    )
    .expect("failed to write material source");

    let first = ThumbnailRequest::from_asset_path(&registry, &material_path)
        .expect("material should support thumbnails")
        .source_version;
    fs::write(&texture_path, b"changed").expect("failed to rewrite texture source");
    let second = ThumbnailRequest::from_asset_path(&registry, &material_path)
        .expect("material should support thumbnails")
        .source_version;

    assert_ne!(first, second);
    drop(registry);
    fs::remove_dir_all(root).expect("failed to remove temp asset root");
}

#[test]
fn thumbnail_type_names_cover_supported_and_unknown_assets() {
    assert_eq!(thumbnail_asset_type_name(Texture::type_uuid()), "texture");
    assert_eq!(thumbnail_asset_type_name(Material::type_uuid()), "material");
    assert_eq!(thumbnail_asset_type_name(Mesh::type_uuid()), "mesh");
    assert_eq!(thumbnail_asset_type_name(Prefab::type_uuid()), "prefab");
    assert_eq!(thumbnail_asset_type_name(Skybox::type_uuid()), "skybox");
    assert_eq!(thumbnail_asset_type_name(Uuid::nil()), "unknown");

    assert!(ThumbnailRequest::is_supported_asset_type(Mesh::type_uuid()));
    assert!(!ThumbnailRequest::is_supported_asset_type(Uuid::nil()));
}

#[test]
fn thumbnail_cache_path_includes_size_settings_type_and_request_identity() {
    let assets_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets");
    let assets_path = dunce::canonicalize(assets_path).expect("assets dir not found");
    let context = engine::test_support::test_asset_context_with_assets(vec![assets_path]);
    let context = context.lock_read();
    let settings = ThumbnailRenderSettings::with_size_and_frame_margin(256, 0.123);
    let cache = ThumbnailCache::new(&context, settings);
    let request = ThumbnailRequest {
        asset_id: Uuid::from_u128(0x1234),
        asset_type: Prefab::type_uuid(),
        source_path: Some(PathBuf::from("assets/prefabs/tank.cxprefab")),
        source_version: 0xfeed_beef,
    };

    let path = cache.path(&request);
    let path_text = path.to_string_lossy().replace('\\', "/");
    assert!(path_text.contains("/Calyx/Editor/thumbnails/"));
    assert!(path_text.contains("/256px/frame-margin-123/prefab/"));
    assert!(path_text.ends_with("/00000000-0000-0000-0000-000000001234-00000000feedbeef.png"));
}

#[test]
fn camera_fit_from_points_tracks_bounds_center_and_radius() {
    let points = vec![
        vec3(-1.0, -2.0, 0.0),
        vec3(3.0, 4.0, 2.0),
        vec3(1.0, -1.0, -2.0),
    ];
    let camera_fit = CameraFit::from_points(points.clone()).expect("points should produce fit");

    assert_eq!(camera_fit.bounds.min, vec3(-1.0, -2.0, -2.0));
    assert_eq!(camera_fit.bounds.max, vec3(3.0, 4.0, 2.0));
    assert_eq!(camera_fit.center(), vec3(1.0, 1.0, 0.0));
    assert_eq!(camera_fit.points, points);
    let expected_radius = camera_fit
        .points
        .iter()
        .map(|point| (*point - camera_fit.center()).norm())
        .fold(0.5, f32::max);
    assert_eq!(camera_fit.radius(), expected_radius);
    assert!(CameraFit::from_points(Vec::new()).is_none());
}

#[test]
fn request_deduplicates_and_promotes_priority() {
    let mut pipeline = ThumbnailPipeline::default();
    let request = request(7);
    let key = request.key();

    pipeline.request(request.clone(), ThumbnailPriority::Low);
    pipeline.request(request, ThumbnailPriority::High);

    assert_eq!(pipeline.queued_len(), 1);
    assert_eq!(
        pipeline.status(key),
        ThumbnailStatus::Queued {
            priority: ThumbnailPriority::High
        }
    );
}

#[test]
fn start_next_picks_highest_priority_job() {
    let mut pipeline = ThumbnailPipeline::default();
    let low = request(1);
    let high = request(2);

    pipeline.request(low.clone(), ThumbnailPriority::Low);
    pipeline.request(high.clone(), ThumbnailPriority::High);

    let job = pipeline.start_next().unwrap();

    assert_eq!(job.key(), high.key());
    assert_eq!(pipeline.status(high.key()), ThumbnailStatus::InProgress);
    assert_eq!(
        pipeline.status(low.key()),
        ThumbnailStatus::Queued {
            priority: ThumbnailPriority::Low
        }
    );
}

#[test]
fn complete_and_fail_only_update_in_progress_jobs() {
    let mut pipeline = ThumbnailPipeline::default();
    let request = request(1);
    let key = request.key();

    assert!(!pipeline.complete(key));
    assert!(!pipeline.fail(key, "not started"));

    pipeline.request(request.clone(), ThumbnailPriority::Normal);
    assert!(!pipeline.complete(key));

    pipeline.start_next();
    assert!(pipeline.complete(key));
    assert_eq!(pipeline.status(key), ThumbnailStatus::Ready);

    pipeline.request(request, ThumbnailPriority::Normal);
    assert!(!pipeline.fail(key, "already ready"));
    assert_eq!(pipeline.status(key), ThumbnailStatus::Ready);
}

#[test]
fn failed_jobs_retry_until_failure_threshold() {
    let mut pipeline = ThumbnailPipeline::default();
    let request = request(1);
    let key = request.key();

    pipeline.request(request.clone(), ThumbnailPriority::Normal);

    for attempt in 1..=THUMBNAIL_MAX_FAILURES {
        pipeline.start_next();
        assert!(pipeline.fail(key, "bad source"));
        assert_eq!(pipeline.failure_count(key), attempt);

        let status = pipeline.request(request.clone(), ThumbnailPriority::High);
        if attempt < THUMBNAIL_MAX_FAILURES {
            assert_eq!(pipeline.queued_len(), 1);
            assert_eq!(
                status,
                ThumbnailStatus::Queued {
                    priority: ThumbnailPriority::High
                }
            );
        } else {
            assert_eq!(pipeline.queued_len(), 0);
            assert_eq!(
                status,
                ThumbnailStatus::Failed {
                    message: "bad source".into()
                }
            );
        }
    }
}

#[test]
fn complete_and_clear_reset_failure_count() {
    let mut pipeline = ThumbnailPipeline::default();
    let request = request(1);
    let key = request.key();

    pipeline.request(request.clone(), ThumbnailPriority::Normal);
    pipeline.start_next();
    assert!(pipeline.fail(key, "bad source"));
    assert_eq!(pipeline.failure_count(key), 1);

    pipeline.request(request.clone(), ThumbnailPriority::Normal);
    pipeline.start_next();
    assert!(pipeline.complete(key));
    assert_eq!(pipeline.failure_count(key), 0);

    pipeline.request(request, ThumbnailPriority::Normal);
    pipeline.clear(key);
    assert_eq!(pipeline.failure_count(key), 0);
    assert_eq!(pipeline.status(key), ThumbnailStatus::Missing);
}

fn assert_camera_encloses_with_margin(bounds: Bounds, frame_margin: f32) {
    let (camera, transform) = camera_for_bounds(bounds, frame_margin);
    let frame_margin = ThumbnailRenderSettings::with_frame_margin(frame_margin).frame_margin;
    let screen_fit = 1.0 - frame_margin * 2.0;

    for corner in bounds.corners() {
        let view = transform.inverse_transform_position(&corner);
        let depth = view.z;
        assert!(
            depth >= camera.near_plane,
            "corner {corner:?} is before the near plane at view-space {view:?}"
        );
        assert!(
            depth <= camera.far_plane,
            "corner {corner:?} is beyond the far plane at view-space {view:?}"
        );
        let projected = project_corner(&camera, &transform, corner)
            .unwrap_or_else(|| panic!("corner {corner:?} could not be projected"));
        assert!(
            projected.x.abs() <= screen_fit + 0.001,
            "corner {corner:?} is outside horizontal screen-space margin at NDC {projected:?}"
        );
        assert!(
            projected.y.abs() <= screen_fit + 0.001,
            "corner {corner:?} is outside vertical screen-space margin at NDC {projected:?}"
        );
    }
}

fn max_projected_extent(bounds: Bounds, frame_margin: f32) -> f32 {
    let (camera, transform) = camera_for_bounds(bounds, frame_margin);
    max_projected_extent_for_points(bounds.corners(), &camera, &transform)
}

fn max_fit_projected_extent(camera_fit: &CameraFit, frame_margin: f32) -> f32 {
    let (camera, transform) = camera_for_fit(camera_fit, frame_margin);
    max_projected_extent_for_points(camera_fit.points.iter().copied(), &camera, &transform)
}

fn projected_fit_center(camera_fit: &CameraFit, frame_margin: f32) -> Vec3 {
    let (camera, transform) = camera_for_fit(camera_fit, frame_margin);
    let (mut min_projected, mut max_projected) = (
        vec3(f32::INFINITY, f32::INFINITY, f32::INFINITY),
        vec3(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
    );
    for point in camera_fit.points.iter().copied() {
        let projected = project_corner(&camera, &transform, point)
            .unwrap_or_else(|| panic!("point {point:?} could not be projected"));
        min_projected.x = min_projected.x.min(projected.x);
        min_projected.y = min_projected.y.min(projected.y);
        max_projected.x = max_projected.x.max(projected.x);
        max_projected.y = max_projected.y.max(projected.y);
    }
    (min_projected + max_projected) * 0.5
}

fn max_projected_extent_for_points(
    points: impl IntoIterator<Item = Vec3>,
    camera: &Camera,
    transform: &Transform,
) -> f32 {
    points
        .into_iter()
        .map(|corner| {
            let projected = project_corner(&camera, &transform, corner)
                .unwrap_or_else(|| panic!("corner {corner:?} could not be projected"));
            projected.x.abs().max(projected.y.abs())
        })
        .fold(0.0, f32::max)
}

fn assert_camera_encloses(bounds: Bounds) {
    assert_camera_encloses_with_margin(bounds, ThumbnailRenderSettings::default().frame_margin);
}

#[test]
fn thumbnail_camera_encloses_bounds() {
    assert_camera_encloses(Bounds {
        min: vec3(-5.0, -0.25, -0.25),
        max: vec3(5.0, 0.25, 0.25),
    });
    assert_camera_encloses(Bounds {
        min: vec3(-0.25, -6.0, -0.25),
        max: vec3(0.25, 6.0, 0.25),
    });
    assert_camera_encloses(Bounds {
        min: vec3(-0.5, -0.5, -4.0),
        max: vec3(0.5, 0.5, 4.0),
    });
}

#[test]
fn thumbnail_camera_default_margin_encloses_asymmetric_prefab_bounds() {
    assert_camera_encloses(Bounds {
        min: vec3(-4.6, -0.8, -2.2),
        max: vec3(5.9, 2.4, 1.7),
    });
    assert_camera_encloses(Bounds {
        min: vec3(-1.3, -1.1, -6.5),
        max: vec3(3.8, 3.6, 2.1),
    });
}

#[test]
fn thumbnail_frame_margin_is_configurable_and_clamped() {
    assert_eq!(
        ThumbnailRenderSettings::default().size_px,
        THUMBNAIL_DEFAULT_SIZE
    );
    assert_eq!(
        ThumbnailRenderSettings::default().frame_margin,
        THUMBNAIL_DEFAULT_FRAME_MARGIN
    );
    assert_eq!(
        ThumbnailRenderSettings::with_size_px(1).size_px,
        THUMBNAIL_MIN_SIZE
    );
    assert_eq!(
        ThumbnailRenderSettings::with_size_px(u32::MAX).size_px,
        THUMBNAIL_MAX_SIZE
    );
    assert_eq!(
        ThumbnailRenderSettings::with_frame_margin(0.0).frame_margin,
        THUMBNAIL_MIN_FRAME_MARGIN
    );
    assert_eq!(
        ThumbnailRenderSettings::with_frame_margin(1.0).frame_margin,
        THUMBNAIL_MAX_FRAME_MARGIN
    );
    assert_eq!(
        ThumbnailRenderSettings::with_frame_margin(f32::NAN).frame_margin,
        THUMBNAIL_DEFAULT_FRAME_MARGIN
    );

    let bounds = Bounds {
        min: vec3(-2.0, -1.0, -0.25),
        max: vec3(2.0, 1.0, 0.25),
    };
    let (_, default_transform) =
        camera_for_bounds(bounds, ThumbnailRenderSettings::default().frame_margin);
    let (_, wider_transform) = camera_for_bounds(bounds, 1.0);
    let center = bounds.center();
    let default_distance = (default_transform.position - center).norm();
    let wider_distance = (wider_transform.position - center).norm();

    assert!(wider_distance > default_distance);
    assert_camera_encloses_with_margin(bounds, ThumbnailRenderSettings::default().frame_margin);
    assert_camera_encloses_with_margin(bounds, 1.0);
}

#[test]
fn thumbnail_frame_margin_is_screen_space_padding() {
    let bounds = Bounds {
        min: vec3(-5.0, -1.5, -2.0),
        max: vec3(6.0, 2.5, 1.0),
    };

    let no_margin_extent = max_projected_extent(bounds, 0.0);
    let ten_percent_extent = max_projected_extent(bounds, 0.1);
    let twenty_five_percent_extent = max_projected_extent(bounds, 0.25);

    assert!(
        (no_margin_extent - 1.0).abs() <= 0.001,
        "expected no-margin extent near full viewport, got {no_margin_extent}"
    );
    assert!(
        (ten_percent_extent - 0.8).abs() <= 0.001,
        "expected 10% per-side margin to fit central 80%, got {ten_percent_extent}"
    );
    assert!(
        (twenty_five_percent_extent - 0.5).abs() <= 0.001,
        "expected 25% per-side margin to fit central 50%, got {twenty_five_percent_extent}"
    );
}

#[test]
fn thumbnail_camera_fits_actual_points_instead_of_empty_bounds_corners() {
    let points = vec![vec3(-5.0, -5.0, -5.0), vec3(5.0, 5.0, 5.0)];
    let camera_fit = CameraFit::from_points(points).expect("test points should produce bounds");

    let (_, bounds_transform) = camera_for_bounds(
        camera_fit.bounds,
        ThumbnailRenderSettings::default().frame_margin,
    );
    let (_, points_transform) =
        camera_for_fit(&camera_fit, ThumbnailRenderSettings::default().frame_margin);
    let center = camera_fit.center();
    let bounds_distance = (bounds_transform.position - center).norm();
    let points_distance = (points_transform.position - center).norm();

    assert!(
            points_distance < bounds_distance,
            "point-fit distance {points_distance} should be closer than AABB-fit distance {bounds_distance}"
        );
    assert!(
        (max_fit_projected_extent(&camera_fit, ThumbnailRenderSettings::default().frame_margin)
            - 0.95)
            .abs()
            <= 0.001
    );
}

#[test]
fn thumbnail_camera_centers_projected_points() {
    let points = vec![
        vec3(-4.0, -0.5, -2.0),
        vec3(6.0, 2.0, 1.0),
        vec3(2.5, -3.0, 2.0),
        vec3(1.0, 0.75, -1.5),
    ];
    let camera_fit = CameraFit::from_points(points).expect("test points should produce bounds");
    let projected_center =
        projected_fit_center(&camera_fit, ThumbnailRenderSettings::default().frame_margin);

    assert!(
        projected_center.x.abs() <= 0.001,
        "expected projected x center near zero, got {projected_center:?}"
    );
    assert!(
        projected_center.y.abs() <= 0.001,
        "expected projected y center near zero, got {projected_center:?}"
    );
    assert!(
        (max_fit_projected_extent(&camera_fit, ThumbnailRenderSettings::default().frame_margin)
            - 0.95)
            .abs()
            <= 0.001
    );
}

#[test]
fn material_thumbnail_camera_uses_configured_frame_margin() {
    let bounds = Bounds::unit();
    let (_, tight_transform) = camera_for_bounds(
        bounds,
        ThumbnailRenderSettings::with_frame_margin(0.1).frame_margin,
    );
    let (_, wider_transform) = camera_for_bounds(
        bounds,
        ThumbnailRenderSettings::with_frame_margin(1.0).frame_margin,
    );
    let center = bounds.center();
    let tight_distance = (tight_transform.position - center).norm();
    let wider_distance = (wider_transform.position - center).norm();

    assert!(wider_distance > tight_distance);
    assert_camera_encloses_with_margin(bounds, 0.1);
    assert_camera_encloses_with_margin(bounds, 1.0);
}

#[test]
fn changing_render_settings_invalidates_in_flight_results() {
    let mut service = ThumbnailService::default();
    let original_epoch = service.shared.state.lock().unwrap().cache_epoch;

    service.set_render_settings(ThumbnailRenderSettings::with_frame_margin(0.25));

    let state = service.shared.state.lock().unwrap();
    assert_eq!(state.render_settings.frame_margin, 0.25);
    assert_eq!(state.cache_epoch, original_epoch.wrapping_add(1));
}

#[test]
fn service_methods_recover_from_poisoned_state_lock() {
    let mut service = ThumbnailService::default();
    let shared = service.shared.clone();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _state = shared.state.lock().unwrap();
        panic!("poison thumbnail state");
    }));

    let request = request(1);
    let status = service.request(request.clone(), ThumbnailPriority::Normal);

    assert_eq!(
        status,
        ThumbnailStatus::Queued {
            priority: ThumbnailPriority::Normal
        }
    );
    assert_eq!(service.status(request.key()), status);
}

#[test]
fn clear_prevents_stale_in_progress_completion() {
    let mut pipeline = ThumbnailPipeline::default();
    let request = request(1);
    let key = request.key();

    pipeline.request(request, ThumbnailPriority::Normal);
    pipeline.start_next();
    pipeline.clear(key);

    assert!(!pipeline.complete(key));
    assert_eq!(pipeline.status(key), ThumbnailStatus::Missing);
}

#[test]
fn clear_asset_versions_removes_old_versions() {
    let mut pipeline = ThumbnailPipeline::default();
    let old = request(1);
    let new = request(2);
    let old_key = old.key();
    let new_key = new.key();

    pipeline.request(old, ThumbnailPriority::Normal);
    pipeline.request(new, ThumbnailPriority::Normal);
    pipeline.clear_asset_versions_except(Uuid::from_u128(1), 2);

    assert_eq!(pipeline.status(old_key), ThumbnailStatus::Missing);
    assert_eq!(
        pipeline.status(new_key),
        ThumbnailStatus::Queued {
            priority: ThumbnailPriority::Normal
        }
    );
}

#[test]
fn material_and_skybox_assets_are_supported_for_thumbnails() {
    assert!(ThumbnailRequest::is_supported_asset_type(
        Material::type_uuid()
    ));
    assert_eq!(thumbnail_asset_type_name(Material::type_uuid()), "material");
    assert!(ThumbnailRequest::is_supported_asset_type(
        Skybox::type_uuid()
    ));
    assert_eq!(thumbnail_asset_type_name(Skybox::type_uuid()), "skybox");
}

#[test]
fn skybox_front_face_shader_builds() {
    let assets_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets");
    let assets_path = dunce::canonicalize(assets_path).expect("assets dir not found");
    let context = engine::test_support::test_asset_context_with_assets(vec![assets_path]);
    let context = context.lock_read();

    SkyboxFrontFaceDownscaler::new(&context)
        .expect("skybox front-face thumbnail shader should build");
}
