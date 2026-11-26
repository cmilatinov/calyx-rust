use super::*;
use crate::assets::AssetAccess;
use crate::component::WaterObstacle;
use crate::test_support::{SceneBuilder, TestAssets};
use approx::assert_abs_diff_eq;
use nalgebra_glm::vec2;

#[test]
fn the_waters_clock_runs_on_and_wraps() {
    assert_abs_diff_eq!(advance_water_time(1.0, 0.5), 1.5);
    assert_abs_diff_eq!(
        advance_water_time(TIME_WRAP - 0.25, 0.5),
        0.25,
        epsilon = 1e-9
    );
    assert_eq!(advance_water_time(2.0, -1.0), 2.0);
}

#[test]
fn cache_builds_the_meshes_once_and_fills_the_material_each_frame() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let mut water = ComponentWater::default();
    let owner = Uuid::new_v4();
    let mut scene = context.scene();
    let object = scene.create(None, None);
    let light = WaterLight {
        direction: -Vec3::y(),
        color: [1.0, 0.5, 0.25],
    };
    let mut cache = WaterCache::default();
    cache.begin_frame(&context, &scene);
    let first = cache
        .draw(&context, &scene, object, owner, &water, Some(&light))
        .unwrap();
    let vertices = first.mesh.read().vertices.len();
    assert_eq!(vertices, water.path().geometry().unwrap().positions.len());
    assert_eq!(
        first.banks.as_ref().unwrap().read().vertices.len(),
        water
            .path()
            .banks(&water.bank_profile())
            .unwrap()
            .positions
            .len()
    );
    let shader: Ref<Shader> = SceneRendererError::load(&context, WATER_SHADER).unwrap();
    assert_eq!(first.material.read().shader.id(), shader.id());
    cache.end_frame();

    // Colours and waves reach the same material without a rebuild.
    cache.begin_frame(&context, &scene);
    water.wave_amplitude = 0.3;
    water.deep_color = Color32::from_rgb(0x20, 0x40, 0x60);
    water.glint_strength = 2.0;
    let same = cache
        .draw(&context, &scene, object, owner, &water, Some(&light))
        .unwrap();
    assert_eq!(same.mesh.id(), first.mesh.id());
    assert_eq!(same.material.id(), first.material.id());
    let material = first.material.clone();
    let variable = |name: &str| {
        material
            .read()
            .variables
            .iter()
            .find(|variable| variable.name == name)
            .map(|variable| variable.value.as_slice().to_vec())
            .unwrap()
    };
    let bytes = |value: ShaderVariableValue| value.as_slice().to_vec();
    assert_eq!(
        variable("wave_amplitude"),
        bytes(ShaderVariableValue::Float(0.3))
    );
    assert_eq!(
        variable("deep_color"),
        bytes(ShaderVariableValue::Color(
            Rgba::from(water.deep_color).to_array()
        ))
    );
    assert_eq!(
        variable("glint_color"),
        bytes(ShaderVariableValue::Color([2.0, 1.0, 0.5, 1.0]))
    );
    assert_eq!(material.read().outline_style(), 0);

    // A new point, obstacle or bank rebuilds the meshes.
    water.points[1].position.x += 1.0;
    let moved = cache
        .draw(&context, &scene, object, owner, &water, None)
        .unwrap();
    assert_ne!(moved.mesh.id(), first.mesh.id());
    water.obstacles.push(WaterObstacle {
        position: vec2(0.0, 0.0),
        radius: 1.0,
    });
    let ringed = cache
        .draw(&context, &scene, object, owner, &water, None)
        .unwrap();
    assert_ne!(ringed.mesh.id(), moved.mesh.id());
    assert_eq!(
        variable("glint_color"),
        bytes(ShaderVariableValue::Color([0.0, 0.0, 0.0, 1.0])),
        "no light, no glints"
    );
    water.bank_height = 0.0;
    water.bank_depth = 0.0;
    assert!(cache
        .draw(&context, &scene, object, owner, &water, None)
        .unwrap()
        .banks
        .is_none());
    water.points.truncate(1);
    assert!(cache
        .draw(&context, &scene, object, owner, &water, None)
        .is_none());
    cache.end_frame();

    cache.begin_frame(&context, &scene);
    cache.end_frame();
    assert!(cache.waters.is_empty());
}

/// Whether any facet of `mesh`, a water's, knows a footprint.
fn rings_anything(mesh: &Ref<Mesh>) -> bool {
    mesh.read().tangents.iter().any(|extra| extra.w >= 0.0)
}

/// A mesh standing in the water is ringed, one clear of its surface or
/// away from it is not, and moving it out of the water takes its ring
/// away.
#[test]
fn the_water_rings_the_meshes_that_cross_its_surface() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let cube = context.registries.assets.read().cube().expect("cube");
    let material = context
        .registries
        .assets
        .read()
        .create(
            "test/water_rock".into(),
            Material::from_shader(
                &context,
                context
                    .registries
                    .assets
                    .read()
                    .load::<Shader>("shaders/mesh/pbr")
                    .unwrap(),
            ),
        )
        .unwrap();
    let mut builder = SceneBuilder::new(context.registries.clone());
    let river = builder.spawn("river");
    let water = ComponentWater::default();
    builder.add_component(river, water.clone());
    let mut spawn = |name: &str, position: Vec3| {
        let object = builder.spawn(name);
        builder.add_component(
            object,
            crate::component::ComponentMesh {
                mesh: crate::assets::AssetRef::from_id(cube.id()),
                material: crate::assets::AssetRef::from_id(material.id()),
                ..Default::default()
            },
        );
        builder.set_transform(object, nalgebra_glm::translation(&position));
        object
    };
    // A rock in the middle of the river, a deck above it and a cube
    // crossing the plane far from the water.
    let rock = spawn("rock", Vec3::new(0.0, 0.0, 0.0));
    spawn("deck", Vec3::new(0.0, 2.0, 4.0));
    spawn("far", Vec3::new(40.0, 0.0, 0.0));
    let mut scene = builder.finish();
    let owner = scene.uuid(river);
    let mut cache = WaterCache::default();
    cache.begin_frame(&context, &scene);
    let ringed = cache
        .draw(&context, &scene, river, owner, &water, None)
        .unwrap();
    cache.end_frame();
    let outlines = &cache.waters[&owner].waterlines.outlines;
    assert_eq!(outlines.len(), 1, "only the rock: {outlines:?}");
    assert!(outlines[0].iter().all(|p| p.norm() < 1.5));
    assert!(rings_anything(&ringed.mesh));

    // Nothing changed: the same mesh.
    cache.begin_frame(&context, &scene);
    let same = cache
        .draw(&context, &scene, river, owner, &water, None)
        .unwrap();
    cache.end_frame();
    assert_eq!(same.mesh.id(), ringed.mesh.id());

    // Lifted clear of the water, the rock is ringed no more.
    scene.set_transform(rock, &nalgebra_glm::translation(&Vec3::new(0.0, 3.0, 0.0)));
    cache.begin_frame(&context, &scene);
    let clear = cache
        .draw(&context, &scene, river, owner, &water, None)
        .unwrap();
    cache.end_frame();
    assert_ne!(clear.mesh.id(), ringed.mesh.id());
    assert!(cache.waters[&owner].waterlines.outlines.is_empty());
    assert!(!rings_anything(&clear.mesh));
}
