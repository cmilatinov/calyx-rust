use super::*;
use crate::assets::AssetRef;
use crate::context::ReadOnlyAssetContext;
use crate::render::water::WATER_SHADER;
use crate::render::{DrawCollection, Shader};
use crate::scene::ChangeJournal;
use crate::test_support::{SceneBuilder, TestAssets};
use nalgebra_glm::{self as glm, vec3};

fn material(context: &ReadOnlyAssetContext, name: &str, shader: &str) -> Ref<Material> {
    let registry = context.registries.assets.read();
    let shader = registry.load::<Shader>(shader).expect("shader loads");
    registry
        .create_or_update(name.into(), Material::from_shader(context, shader))
        .expect("material should be created")
}

fn spawn_mesh(
    builder: &mut SceneBuilder,
    parent: Option<GameObject>,
    mesh: &Ref<Mesh>,
    material: &Ref<Material>,
    position: Vec3,
) -> GameObject {
    let game_object = match parent {
        Some(parent) => builder.spawn_child(parent, "mesh"),
        None => builder.spawn("mesh"),
    };
    builder.add_component(
        game_object,
        ComponentMesh {
            mesh: AssetRef::from_id(mesh.id()),
            material: AssetRef::from_id(material.id()),
            ..Default::default()
        },
    );
    builder.set_transform(game_object, glm::translation(&position));
    game_object
}

fn batch_mesh(draws: &DrawCollection, key: BatchKey) -> Ref<Mesh> {
    draws
        .static_batches
        .batches()
        .find(|(other, _)| **other == key)
        .and_then(|(_, batch)| batch.mesh.clone())
        .expect("the batch is merged")
}

#[test]
fn static_pbr_meshes_merge_per_material_and_chunk_and_the_others_draw_alone() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let cube = context.registries.assets.read().cube().expect("cube");
    let first = material(&context, "test/static_first", "shaders/mesh/pbr");
    let second = material(&context, "test/static_second", "shaders/mesh/pbr");
    let water = material(&context, "test/static_water", WATER_SHADER);
    let mut builder = SceneBuilder::new(context.registries.clone());
    let a = spawn_mesh(&mut builder, None, &cube, &first, vec3(0.0, 0.0, 0.0));
    let b = spawn_mesh(&mut builder, None, &cube, &first, vec3(2.0, 0.0, 0.0));
    let far = spawn_mesh(&mut builder, None, &cube, &first, vec3(40.0, 0.0, 0.0));
    let other = spawn_mesh(&mut builder, None, &cube, &second, vec3(4.0, 0.0, 0.0));
    let unlit = spawn_mesh(&mut builder, None, &cube, &water, vec3(6.0, 0.0, 0.0));
    let fixed = builder.spawn("fixed");
    builder.add_component(
        fixed,
        ComponentRigidBody {
            ty: RigidBodyType::Fixed,
            ..Default::default()
        },
    );
    let on_fixed = spawn_mesh(&mut builder, Some(fixed), &cube, &first, Vec3::zeros());
    let tank = builder.spawn("tank");
    builder.add_component(tank, ComponentRigidBody::default());
    let on_tank = spawn_mesh(&mut builder, Some(tank), &cube, &first, Vec3::zeros());
    let hidden = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
    let _ = builder
        .scene_mut()
        .write_component::<ComponentID, _>(hidden, |id| id.visible = false);
    let scene = builder.finish();

    let mut draws = DrawCollection::new(&context).expect("white texture loads");
    draws.collect(&scene);
    let batches = &draws.static_batches;
    for batched in [a, b, far, other, on_fixed] {
        assert!(batches.is_batched(batched.entity));
    }
    for alone in [unlit, on_tank, hidden] {
        assert!(!batches.is_batched(alone.entity));
        assert!(batches.individual().contains(&alone.entity));
    }
    let key = batches.batch_of(a.entity).unwrap();
    assert_eq!(batches.batch_of(b.entity), Some(key));
    assert_eq!(batches.batch_of(on_fixed.entity), Some(key));
    assert_ne!(batches.batch_of(far.entity), Some(key), "another chunk");
    assert_ne!(
        batches.batch_of(other.entity),
        Some(key),
        "another material"
    );
    assert_eq!(batches.batches().count(), 3);

    // The merged mesh holds each member's cube in world space, each
    // vertex carrying its object's id, and draws as one instance.
    let merged = batch_mesh(&draws, key);
    let merged = merged.read();
    let cube = cube.read();
    let count = cube.vertices.len();
    assert_eq!(merged.vertices.len(), 3 * count);
    assert_eq!(merged.indices.len(), 3 * cube.indices.len());
    assert_eq!(
        merged.vertices[count],
        cube.vertices[0] + vec3(2.0, 0.0, 0.0)
    );
    assert_eq!(
        merged.indices[cube.indices.len()],
        cube.indices[0] + count as u32
    );
    let id_of = |object: GameObject| draws.object_id_lookup[&scene.uuid(object)] as f32;
    assert!(merged.uvs[3][..count].iter().all(|uv| uv.x == id_of(a)));
    assert!(merged.uvs[3][count..2 * count]
        .iter()
        .all(|uv| uv.x == id_of(b)));
    let batch_draws = draws
        .draw_list
        .iter()
        .filter(|draw| draw.object_id == BATCHED_OBJECT_ID)
        .count();
    assert_eq!(batch_draws, 3);
    assert_eq!(
        draws.draw_list.len(),
        3 + 2,
        "the batches, the water and the tank"
    );
}

#[test]
fn a_box_is_outside_the_view_only_when_wholly_past_one_side_of_the_frustum() {
    let camera = crate::render::Camera::new(1.0, 90f32.to_radians(), 0.1, 100.0);
    // The camera looks down positive z from the origin.
    let clip_from_world = camera.projection;
    let ahead = [vec3(-1.0, -1.0, 4.0), vec3(1.0, 1.0, 6.0)];
    let straddling = [vec3(-20.0, -1.0, 4.0), vec3(-4.0, 1.0, 6.0)];
    let left = [vec3(-20.0, -1.0, 4.0), vec3(-8.0, 1.0, 6.0)];
    let above = [vec3(-1.0, 8.0, 4.0), vec3(1.0, 9.0, 6.0)];
    let around = [vec3(-1.0, -1.0, -1.0), vec3(1.0, 1.0, 1.0)];
    assert!(!outside_view(&clip_from_world, &ahead));
    assert!(!outside_view(&clip_from_world, &straddling));
    assert!(!outside_view(&clip_from_world, &around));
    assert!(outside_view(&clip_from_world, &left));
    assert!(outside_view(&clip_from_world, &above));
}

#[test]
fn moving_or_deleting_a_batched_object_merges_its_batch_again_without_it() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let cube = context.registries.assets.read().cube().expect("cube");
    let first = material(&context, "test/static_moving", "shaders/mesh/pbr");
    let vertices = cube.read().vertices.len();
    let mut builder = SceneBuilder::new(context.registries.clone());
    let a = spawn_mesh(&mut builder, None, &cube, &first, vec3(0.0, 0.0, 0.0));
    let b = spawn_mesh(&mut builder, None, &cube, &first, vec3(2.0, 0.0, 0.0));
    let parent = builder.spawn("parent");
    let c = spawn_mesh(
        &mut builder,
        Some(parent),
        &cube,
        &first,
        vec3(4.0, 0.0, 0.0),
    );
    let mut scene = builder.finish();
    let mut draws = DrawCollection::new(&context).expect("white texture loads");
    draws.collect(&scene);
    let key = draws.static_batches.batch_of(a.entity).unwrap();
    let merges = draws.static_batches.merges();
    let merged = batch_mesh(&draws, key);
    assert_eq!(merged.read().vertices.len(), 3 * vertices);

    // Frames that change nothing, or write a transform the object
    // already had, keep the merged mesh and the draw list.
    let draw_list = draws.draw_list.len();
    draws.collect(&scene);
    scene.set_transform(a, &glm::translation(&vec3(0.0, 0.0, 0.0)));
    draws.collect(&scene);
    assert_eq!(draws.static_batches.merges(), merges);
    assert_eq!(batch_mesh(&draws, key).id(), merged.id());
    assert!(draws.static_batches.is_batched(a.entity));
    assert_eq!(draws.draw_list.len(), draw_list);

    // A moved object leaves its batch, which merges again without it,
    // and draws on its own where it moved to, under the same object id.
    let a_id = draws.object_id_lookup[&scene.uuid(a)];
    scene.set_transform(a, &glm::translation(&vec3(1.0, 5.0, 0.0)));
    draws.collect(&scene);
    assert!(!draws.static_batches.is_batched(a.entity));
    assert!(draws.static_batches.individual().contains(&a.entity));
    assert_eq!(draws.static_batches.merges(), merges + 1);
    assert_eq!(batch_mesh(&draws, key).read().vertices.len(), 2 * vertices);
    assert_eq!(draws.object_id_lookup[&scene.uuid(a)], a_id);
    let moved = draws
        .draw_list
        .iter()
        .find(|draw| draw.object_id == a_id)
        .expect("the moved object draws on its own");
    assert_eq!(moved.transform[3], [1.0, 5.0, 0.0, 1.0]);

    // Moving a parent moves its children out of their batches.
    scene.set_transform(parent, &glm::translation(&vec3(0.0, 1.0, 0.0)));
    draws.collect(&scene);
    assert!(!draws.static_batches.is_batched(c.entity));
    assert_eq!(batch_mesh(&draws, key).read().vertices.len(), vertices);

    // Deleting the last member drops the batch.
    scene.delete(b);
    scene.flush_deletes();
    draws.collect(&scene);
    assert!(!draws.static_batches.is_batched(b.entity));
    assert_eq!(draws.static_batches.batches().count(), 0);
    assert_eq!(
        draws.draw_list.len(),
        2,
        "the moved object and the moved child"
    );
}

#[test]
fn hiding_disabling_or_repainting_a_batched_object_takes_it_out() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let cube = context.registries.assets.read().cube().expect("cube");
    let first = material(&context, "test/static_edited", "shaders/mesh/pbr");
    let second = material(&context, "test/static_edited_second", "shaders/mesh/pbr");
    let mut builder = SceneBuilder::new(context.registries.clone());
    let hidden = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
    let disabled = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
    let repainted = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
    let kept = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
    let mut scene = builder.finish();
    let mut draws = DrawCollection::new(&context).expect("white texture loads");
    draws.collect(&scene);
    assert!(draws.static_batches.individual().is_empty());

    let _ = scene.write_component::<ComponentID, _>(hidden, |id| id.visible = false);
    scene.set_component_enabled::<ComponentMesh>(disabled, false);
    let _ = scene.write_component::<ComponentMesh, _>(repainted, |mesh| {
        mesh.material = AssetRef::from_id(second.id());
    });
    draws.collect(&scene);
    for edited in [hidden, disabled, repainted] {
        assert!(!draws.static_batches.is_batched(edited.entity));
    }
    assert!(draws.static_batches.is_batched(kept.entity));
    let drawn: Vec<_> = draws.draw_list.iter().map(|draw| draw.mat_id).collect();
    assert_eq!(drawn.len(), 2, "the batch and the repainted object");
    assert!(drawn.contains(&second.id()));
}

#[test]
fn another_scene_or_a_journal_overrun_merges_the_batches_from_scratch() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let cube = context.registries.assets.read().cube().expect("cube");
    let first = material(&context, "test/static_overrun", "shaders/mesh/pbr");
    let mut builder = SceneBuilder::new(context.registries.clone());
    let a = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
    let b = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
    let mut scene = builder.finish();
    let mut draws = DrawCollection::new(&context).expect("white texture loads");
    draws.collect(&scene);
    scene.set_transform(a, &glm::translation(&vec3(1.0, 0.0, 0.0)));
    draws.collect(&scene);
    assert!(!draws.static_batches.is_batched(a.entity));

    // More changes than the journal holds: the batches start over, and
    // the moved object, which now holds still, batches again.
    for _ in 0..=ChangeJournal::CAPACITY {
        let _ = scene.write_component::<ComponentID, _>(b, |_| ());
    }
    draws.collect(&scene);
    assert!(draws.static_batches.is_batched(a.entity));
    assert!(draws.static_batches.is_batched(b.entity));

    let copy = scene.restore_snapshot(scene.snapshot());
    draws.collect(&copy);
    let copied = copy.find(scene.uuid(a)).unwrap();
    assert!(draws.static_batches.is_batched(copied.entity));
    assert_eq!(draws.static_batches.batches().count(), 1);
}
