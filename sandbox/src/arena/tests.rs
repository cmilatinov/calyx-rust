use super::{cover_cells, generate_arena, perimeter_cells, ComponentArena};
use calyx_engine::assets::AssetAccess;
use calyx_engine::component::ComponentMesh;
use calyx_engine::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
use rapier3d::dynamics::RigidBodyType;
use std::collections::HashSet;

#[test]
fn arena_layout_uses_unique_grid_cells() {
    let perimeter = perimeter_cells(15, 16);
    let covers = cover_cells(15, 16);

    assert_eq!(perimeter.len(), 58);
    assert_eq!(perimeter.iter().copied().collect::<HashSet<_>>().len(), 58);
    assert_eq!(covers.len(), 9);
    assert!(covers.iter().all(|cell| !perimeter.contains(cell)));
}

#[test]
fn generator_creates_independent_fixed_grid_cubes() {
    let mut scene = calyx_engine::test_support::test_scene();
    let arena_object = scene.create(None, None);
    scene.add_component(arena_object, ComponentArena::default());
    scene.start();
    scene.start();

    let children: Vec<_> = scene.children(arena_object).collect();
    let wall_count = children
        .iter()
        .filter(|object| scene.name(**object).starts_with("Arena Wall"))
        .count();
    let cover_count = children
        .iter()
        .filter(|object| scene.name(**object).starts_with("Arena Cover"))
        .count();

    assert_eq!(children.len(), 68);
    assert_eq!(wall_count, 58);
    assert_eq!(cover_count, 9);
    for object in children
        .into_iter()
        .filter(|object| scene.name(*object) != "Arena Floor")
    {
        let rigid_body = scene
            .read_component::<ComponentRigidBody, _, _>(object, |body| body.ty)
            .expect("grid cube should have a rigid body");
        assert_eq!(rigid_body, RigidBodyType::Fixed);

        let shape = scene
            .read_component::<ComponentCollider, _, _>(object, |collider| collider.shape)
            .expect("grid cube should have a collider");
        let ColliderShape::Cuboid { half_extents } = shape else {
            panic!("grid cube collider should be a cuboid");
        };
        assert_eq!(half_extents, nalgebra_glm::vec3(1.0, 1.0, 1.0));
    }
}

#[test]
fn authored_scene_keeps_its_layout_when_started_and_reloaded() {
    let copy = calyx_engine::test_support::TestAssets::game();
    let assets = copy.asset_context();
    let scene_ref = assets
        .registries
        .assets
        .read()
        .reload_by_path::<calyx_engine::scene::Scene>(
            &copy.sandbox_root().join("game/scenes/arena.cxscene"),
        )
        .expect("arena should load");
    let mut scene = scene_ref.read().clone();
    let arena = scene
        .objects()
        .find(|object| scene.name(*object) == "Arena")
        .unwrap();
    assert_eq!(
        scene.read_component::<ComponentArena, _, _>(arena, |arena| arena.generate_on_start),
        Some(false)
    );
    let count = scene.objects().count();
    let colliders = scene.descendants_with::<ComponentCollider>(arena).count();
    assert!(colliders > 50, "the authored pen and cover have collision");
    // Saved before colliders had groups, every collider keeps the default
    // groups, stopping hulls, rounds and sight alike, and saves unchanged;
    // only the farmland creek's blockers, the bridge's rails and the
    // barn yard's paddock fence stop tanks alone.
    let mut tank_blockers = 0;
    // Brush volumes, which stop nothing, have their own.
    for object in scene.descendants_with::<ComponentCollider>(arena) {
        if scene
            .ancestor_with::<crate::brush::ComponentBrush>(object)
            .is_some()
        {
            scene.read_component::<ComponentCollider, _, _>(object, |collider| {
                assert_eq!(collider.groups, crate::collision::BRUSH);
            });
            continue;
        }
        let name = scene.name(object);
        scene.read_component::<ComponentCollider, _, _>(object, |collider| {
            if name.starts_with("Arena Creek Blocker")
                || name.starts_with("Arena Bridge Rail")
                || name.starts_with("Arena Paddock Fence")
            {
                assert_eq!(collider.groups, crate::collision::TANK_BLOCKER, "{name}");
                tank_blockers += 1;
            } else {
                assert!(collider.groups.is_all(), "{name}");
                let saved = serde_json::to_value(collider).unwrap();
                assert!(saved.get("groups").is_none(), "{saved}");
            }
        });
    }
    assert!(tank_blockers > 10, "the creek has blockers");
    let registries = assets.registries.lock_read();
    let mut checked = HashSet::new();
    for object in scene.descendants_with::<ComponentMesh>(arena) {
        scene.read_component::<ComponentMesh, _, _>(object, |renderer| {
            if checked.insert((renderer.mesh.id(), renderer.material.id())) {
                assert!(
                    renderer.mesh.get_ref(&registries).is_some(),
                    "missing mesh {}",
                    renderer.mesh.id()
                );
                assert!(
                    renderer.material.get_ref(&registries).is_some(),
                    "missing material {}",
                    renderer.material.id()
                );
            }
        });
    }
    scene.start();
    scene.start();
    assert_eq!(scene.objects().count(), count);
    assert_eq!(
        scene.descendants_with::<ComponentCollider>(arena).count(),
        colliders
    );
    let mut reloaded = scene.restore_snapshot(scene.snapshot());
    reloaded.start();
    assert_eq!(reloaded.objects().count(), count);
}

#[test]
fn a_decorative_child_does_not_disable_the_grid_generator() {
    let mut scene = calyx_engine::test_support::test_scene();
    let arena = scene.create(None, None);
    scene.create(None, Some(arena));
    generate_arena(&mut scene, arena, &ComponentArena::default());
    assert_eq!(scene.children(arena).count(), 69);
}
