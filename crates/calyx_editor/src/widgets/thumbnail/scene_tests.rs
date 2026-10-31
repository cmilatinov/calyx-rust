//! Headless scene thumbnail tests: each one renders on the shared test GPU.

use super::cache::read_texture_rgba8;
use super::camera_fit::*;
use super::generator::{scene_has_light, scene_thumbnail_view, ThumbnailGenerator};
use super::pipeline::*;
use super::*;
use crate::scene_file::write_scene_file;
use calyx_engine::component::ComponentCamera;
use calyx_engine::test_support::TestAssets;

const SIZE_PX: u32 = 64;

/// A copy of the engine assets with the scenes a test writes into it.
struct SceneFixture {
    assets: TestAssets,
    context: calyx_engine::context::AssetContext,
}

impl SceneFixture {
    fn new() -> Self {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        Self { assets, context }
    }

    fn context(&self) -> ReadOnlyAssetContext {
        self.context.lock_read()
    }

    fn scene(&self) -> Scene {
        self.context().scene()
    }

    fn asset_id(&self, name: &str) -> Uuid {
        self.context()
            .registries
            .assets
            .read()
            .asset_id(name)
            .unwrap_or_else(|| panic!("engine asset {name} should be registered"))
    }

    fn path(&self, file: &str) -> PathBuf {
        self.assets
            .engine_root()
            .join("thumbnail_scenes")
            .join(file)
    }

    /// Writes `scene` to `file` and registers it, returning its asset id.
    fn write(&self, file: &str, scene: &Scene) -> Uuid {
        let path = self.path(file);
        fs::create_dir_all(path.parent().unwrap()).expect("scene dir should be created");
        write_scene_file(&path, scene).expect("scene should be written");
        self.context()
            .registries
            .assets
            .read()
            .register_file(&path)
            .expect("scene should register")
    }

    fn request(&self, asset_id: Uuid) -> ThumbnailRequest {
        let context = self.context();
        let registry = context.registries.assets.read();
        ThumbnailRequest::from_asset_id(&registry, asset_id)
            .expect("scenes should support thumbnails")
    }

    /// Renders the scene `asset_id`'s thumbnail and reads its pixels back.
    fn render(&self, asset_id: Uuid) -> Vec<u8> {
        let context = self.context();
        let request = self.request(asset_id);
        let mut generator = ThumbnailGenerator::with_render_settings(
            ThumbnailRenderSettings::with_size_px(SIZE_PX),
        );
        let texture = generator
            .generate(&context, context.render_context.as_ref(), &request)
            .expect("scene thumbnail should render");
        assert_eq!(texture.descriptor.size.width, SIZE_PX);
        assert_eq!(texture.descriptor.size.height, SIZE_PX);
        read_texture_rgba8(context.render_context.as_ref(), &texture, SIZE_PX)
            .expect("scene thumbnail should read back")
    }

    /// Adds a sphere at `position` with the default material.
    fn add_sphere(&self, scene: &mut Scene, position: Vec3) -> GameObject {
        let sphere = scene.create(None, None);
        scene.set_world_transform(
            sphere,
            Transform::from_xyz(position.x, position.y, position.z).matrix(),
        );
        scene.add_component(
            sphere,
            ComponentMesh {
                mesh: AssetRef::from_id(self.asset_id("meshes/sphere")),
                material: AssetRef::from_id(self.asset_id("materials/default")),
            },
        );
        sphere
    }
}

fn add_directional_light(scene: &mut Scene) {
    let light = scene.create(None, None);
    let mut transform = Transform::from_xyz(2.0, 4.0, -3.0);
    transform.look_at(&vec3(0.0, 0.0, 0.0));
    scene.set_world_transform(light, transform.matrix());
    scene.add_component(
        light,
        ComponentDirectionalLight {
            active: true,
            color: Color32::WHITE,
            intensity: 4.0,
        },
    );
}

/// Adds a camera at `position` looking along +Z, the way an unrotated
/// camera looks.
fn add_camera(scene: &mut Scene, position: Vec3, clear_color: Color32) -> GameObject {
    let camera = scene.create(None, None);
    scene.set_world_transform(
        camera,
        Transform::from_xyz(position.x, position.y, position.z).matrix(),
    );
    scene.add_component(
        camera,
        ComponentCamera {
            clear_color,
            ..Default::default()
        },
    );
    camera
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * SIZE_PX + x) * 4) as usize;
    pixels[offset..offset + 4].try_into().unwrap()
}

fn is_uniform(pixels: &[u8]) -> bool {
    pixels.chunks_exact(4).all(|texel| texel == &pixels[0..4])
}

#[test]
fn scenes_are_a_supported_thumbnail_type_cached_under_scene() {
    assert!(ThumbnailRequest::is_supported_asset_type(Scene::type_uuid()));
    assert_eq!(thumbnail_asset_type_name(Scene::type_uuid()), "scene");

    let fixture = SceneFixture::new();
    let context = fixture.context();
    let cache = super::cache::ThumbnailCache::new(&context, ThumbnailRenderSettings::default());
    let request = ThumbnailRequest {
        asset_id: Uuid::from_u128(0x5ce),
        asset_type: Scene::type_uuid(),
        source_path: Some(PathBuf::from("assets/scenes/level.cxscene")),
        source_version: 1,
    };
    let path = cache.path(&request).to_string_lossy().replace('\\', "/");
    assert!(path.contains("/scene/"), "{path}");
}

#[test]
fn a_scene_with_a_mesh_and_a_light_renders_the_mesh() {
    let fixture = SceneFixture::new();
    let mut scene = fixture.scene();
    fixture.add_sphere(&mut scene, Vec3::zeros());
    add_directional_light(&mut scene);
    let scene_id = fixture.write("lit.cxscene", &scene);

    let pixels = fixture.render(scene_id);

    let center = pixel(&pixels, SIZE_PX / 2, SIZE_PX / 2);
    let corner = pixel(&pixels, 0, 0);
    assert!(center[3] > 0, "the fitted sphere covers the center");
    assert_ne!(center, corner, "the sphere stands out from the background");
}

#[test]
fn a_scene_with_a_main_camera_renders_from_that_camera_not_the_fit() {
    let fixture = SceneFixture::new();
    let mut scene = fixture.scene();
    fixture.add_sphere(&mut scene, Vec3::zeros());
    add_directional_light(&mut scene);
    let fitted_id = fixture.write("fitted.cxscene", &scene);
    // In front of the sphere, looking away from it.
    let clear_color = Color32::from_rgb(40, 90, 160);
    add_camera(&mut scene, vec3(0.0, 0.0, 5.0), clear_color);
    let camera_id = fixture.write("camera.cxscene", &scene);

    let fitted = fixture.render(fitted_id);
    let from_camera = fixture.render(camera_id);

    assert_ne!(fitted, from_camera);
    assert!(
        is_uniform(&from_camera),
        "the camera faces away from the only mesh, so it sees only its clear color"
    );
    assert_eq!(
        pixel(&from_camera, 0, 0)[3],
        255,
        "the clear color is opaque"
    );

    let context = fixture.context();
    let view = scene_thumbnail_view(&context, &scene, THUMBNAIL_DEFAULT_FRAME_MARGIN);
    assert!(view.from_main_camera);
    assert_eq!(view.clear_color, clear_color);
    assert_eq!(view.camera_transform.position, vec3(0.0, 0.0, 5.0));
    let camera = ComponentCamera::default();
    assert_eq!(view.camera.near_plane, camera.near_plane);
    assert_eq!(view.camera.far_plane, camera.far_plane);
}

#[test]
fn a_scene_without_an_enabled_camera_falls_back_to_the_bounds_fit() {
    let fixture = SceneFixture::new();
    let mut scene = fixture.scene();
    fixture.add_sphere(&mut scene, vec3(10.0, 0.0, 0.0));
    let context = fixture.context();

    let view = scene_thumbnail_view(&context, &scene, THUMBNAIL_DEFAULT_FRAME_MARGIN);
    assert!(!view.from_main_camera);
    assert_eq!(view.clear_color, Color32::TRANSPARENT);
    let fit = scene_bounds_camera_fit(&context, &scene).expect("the scene has a mesh");
    assert!((fit.center() - vec3(10.0, 0.0, 0.0)).norm() < 0.001);
    let (_, expected) = camera_for_fit(&fit, THUMBNAIL_DEFAULT_FRAME_MARGIN);
    assert_eq!(view.camera_transform.position, expected.position);

    let camera = add_camera(&mut scene, vec3(0.0, 0.0, 5.0), Color32::BLACK);
    assert!(
        scene_thumbnail_view(&context, &scene, THUMBNAIL_DEFAULT_FRAME_MARGIN).from_main_camera
    );
    assert!(scene.set_component_enabled::<ComponentCamera>(camera, false));
    assert!(
        !scene_thumbnail_view(&context, &scene, THUMBNAIL_DEFAULT_FRAME_MARGIN).from_main_camera,
        "a disabled camera is not the main camera"
    );
}

#[test]
fn preview_lighting_is_only_for_scenes_without_lights() {
    let fixture = SceneFixture::new();
    let mut scene = fixture.scene();
    fixture.add_sphere(&mut scene, Vec3::zeros());
    assert!(!scene_has_light(&scene));

    let unlit_id = fixture.write("unlit.cxscene", &scene);
    let pixels = fixture.render(unlit_id);
    let center = pixel(&pixels, SIZE_PX / 2, SIZE_PX / 2);
    assert!(
        center[0..3].iter().any(|channel| *channel > 8),
        "preview lighting keeps an unlit scene from rendering black, got {center:?}"
    );

    add_directional_light(&mut scene);
    assert!(scene_has_light(&scene));
}

#[test]
fn an_empty_scene_still_yields_a_valid_thumbnail() {
    let fixture = SceneFixture::new();
    let scene_id = fixture.write("empty.cxscene", &fixture.scene());

    let pixels = fixture.render(scene_id);

    assert_eq!(pixels.len(), (SIZE_PX * SIZE_PX * 4) as usize);
}

#[test]
fn changing_the_scene_file_or_an_asset_it_references_changes_its_key() {
    let fixture = SceneFixture::new();
    let material_dir = fixture.path("materials");
    fs::create_dir_all(&material_dir).unwrap();
    let material_path = material_dir.join("scene_material.cxmat");
    fs::copy(
        fixture.assets.engine_root().join("materials/default.cxmat"),
        &material_path,
    )
    .expect("the default material should copy");
    let material_id = fixture
        .context()
        .registries
        .assets
        .read()
        .register_file(&material_path)
        .expect("the material should register");

    let mut scene = fixture.scene();
    let sphere = fixture.add_sphere(&mut scene, Vec3::zeros());
    scene.write_component::<ComponentMesh, _>(sphere, |mesh| {
        mesh.material = AssetRef::from_id(material_id);
    });
    add_directional_light(&mut scene);
    let scene_id = fixture.write("referencing.cxscene", &scene);
    let first = fixture.request(scene_id);
    let first_pixels = fixture.render(scene_id);

    // The scene's own file changes: a second sphere joins the first.
    let other = scene.create(None, None);
    scene.set_world_transform(other, Transform::from_xyz(4.0, 0.0, 0.0).matrix());
    scene.add_component(
        other,
        ComponentMesh {
            mesh: AssetRef::from_id(fixture.asset_id("meshes/sphere")),
            material: AssetRef::from_id(material_id),
        },
    );
    fixture.write("referencing.cxscene", &scene);
    let second = fixture.request(scene_id);
    assert_ne!(
        first.key(),
        second.key(),
        "a changed scene file is a new key"
    );
    assert_ne!(
        first_pixels,
        fixture.render(scene_id),
        "the changed scene renders anew"
    );

    // An asset the scene references changes.
    let material_source = fs::read_to_string(&material_path).unwrap();
    fs::write(&material_path, format!("{material_source}\n")).unwrap();
    let third = fixture.request(scene_id);
    assert_ne!(
        second.key(),
        third.key(),
        "a changed referenced material is a new key"
    );

    let context = fixture.context();
    let cache = super::cache::ThumbnailCache::new(&context, ThumbnailRenderSettings::default());
    assert_ne!(
        cache.path(&second),
        cache.path(&third),
        "a new key is a new cache file, so the scene renders again"
    );
}

fn scene_request(source_version: u64) -> ThumbnailRequest {
    ThumbnailRequest {
        asset_id: Uuid::from_u128(0x5ce),
        asset_type: Scene::type_uuid(),
        source_path: Some(PathBuf::from("assets/scenes/level.cxscene")),
        source_version,
    }
}

#[test]
fn scene_jobs_wait_for_the_project_assemblies_instead_of_failing() {
    let mut pipeline = ThumbnailPipeline::default();
    let scene = scene_request(1);
    let texture = ThumbnailRequest {
        asset_id: Uuid::from_u128(0x7e),
        asset_type: Texture::type_uuid(),
        ..scene_request(1)
    };
    pipeline.request(scene.clone(), ThumbnailPriority::High);
    pipeline.request(texture.clone(), ThumbnailPriority::Low);

    let assemblies_loaded = false;
    let can_start =
        |job: &ThumbnailJob| assemblies_loaded || !job.request.needs_project_assemblies();
    let started = pipeline.start_next_where(can_start).unwrap();
    assert_eq!(started.key(), texture.key());
    assert!(pipeline.start_next_where(can_start).is_none());
    assert_eq!(
        pipeline.status(scene.key()),
        ThumbnailStatus::Queued {
            priority: ThumbnailPriority::High
        },
        "the scene stays queued, not failed"
    );

    let started = pipeline.start_next_where(|_| true).unwrap();
    assert_eq!(started.key(), scene.key());
}

#[test]
fn repeated_saves_collapse_into_one_job_for_the_newest_version() {
    let fixture = SceneFixture::new();
    let mut scene = fixture.scene();
    let scene_id = fixture.write("saved.cxscene", &scene);
    let service = ThumbnailService::default();
    let handle = service.handle();

    let mut keys = Vec::new();
    for _ in 0..3 {
        scene.create(None, None);
        fixture.write("saved.cxscene", &scene);
        let context = fixture.context();
        let registry = context.registries.assets.read();
        keys.push(
            handle
                .refresh_asset(&registry, scene_id, ThumbnailPriority::High)
                .expect("a scene has a thumbnail"),
        );
    }

    let mut state = service.shared.state.lock().unwrap();
    let newest = *keys.last().unwrap();
    assert_eq!(
        state.pipeline.status(newest),
        ThumbnailStatus::Queued {
            priority: ThumbnailPriority::High
        }
    );
    for key in &keys[..keys.len() - 1] {
        if *key != newest {
            assert_eq!(state.pipeline.status(*key), ThumbnailStatus::Missing);
        }
    }
    assert_eq!(state.pipeline.queued_len(), 1);
    assert_eq!(state.pipeline.start_next().unwrap().key(), newest);
}

#[test]
fn a_mesh_without_a_material_renders_with_the_default_material() {
    let fixture = SceneFixture::new();
    let mut scene = fixture.scene();
    let sphere = fixture.add_sphere(&mut scene, Vec3::zeros());
    scene.write_component::<ComponentMesh, _>(sphere, |mesh| {
        mesh.material = AssetRef::from_id(Uuid::nil());
    });
    add_directional_light(&mut scene);
    let scene_id = fixture.write("no_material.cxscene", &scene);

    let pixels = fixture.render(scene_id);

    let center = pixel(&pixels, SIZE_PX / 2, SIZE_PX / 2);
    assert!(
        center[3] > 0,
        "the sphere draws even though the scene gives it no material"
    );
}

#[test]
fn a_skinned_mesh_is_fitted_where_the_renderer_draws_it() {
    use calyx_engine::assets::mesh::BoneInfo;
    use calyx_engine::component::ComponentBone;
    use calyx_engine::scene::GameObjectRef;
    use nalgebra_glm as glm;

    // A model root, a node converting axes and units, and the skinned mesh
    // below it, as imported models are laid out. The bone lifts the mesh.
    let fixture = SceneFixture::new();
    let context = fixture.context();
    let mut scene = fixture.scene();
    let root = scene.create(None, None);
    scene.set_transform(root, &glm::translation(&vec3(5.0, 0.0, 0.0)));
    let converter = scene.create(None, Some(root));
    scene.set_transform(
        converter,
        &(glm::rotation(-90f32.to_radians(), &Vec3::x()) * glm::scaling(&Vec3::repeat(100.0))),
    );
    let bone = scene.create(None, Some(root));
    scene.set_transform(bone, &glm::translation(&vec3(0.0, 2.0, 0.0)));
    scene.add_component(
        bone,
        ComponentBone {
            name: "hip".into(),
            ..Default::default()
        },
    );
    let mesh_object = scene.create(None, Some(converter));
    scene.add_component(
        mesh_object,
        ComponentSkinnedMesh {
            root_bone: GameObjectRef::new(scene.uuid(root)),
            ..Default::default()
        },
    );
    let mut mesh = Mesh::new(context.render_context.as_ref());
    mesh.vertices = vec![vec3(-0.5, 0.0, 0.0), vec3(0.5, 0.0, 0.0)];
    mesh.bone_indices = vec![glm::IVec4::new(0, -1, -1, -1); 2];
    mesh.bone_weights = vec![glm::vec4(1.0, 0.0, 0.0, 0.0); 2];
    mesh.bones.insert(
        "hip".into(),
        BoneInfo {
            index: 0,
            inverse_bind_transform: glm::Mat4::identity(),
        },
    );

    let fit = CameraFit::from_points(skinned_mesh_fit_points(&scene, mesh_object, &mesh))
        .expect("the skinned mesh has vertices");

    // The renderer places the posed vertices by the root bone, at x = 5,
    // lifted 2 by the bone: not 100 times larger and turned by the converter.
    assert!(
        (fit.bounds.min - vec3(4.5, 2.0, 0.0)).norm() < 1e-4,
        "{:?}",
        fit.bounds
    );
    assert!(
        (fit.bounds.max - vec3(5.5, 2.0, 0.0)).norm() < 1e-4,
        "{:?}",
        fit.bounds
    );
}
