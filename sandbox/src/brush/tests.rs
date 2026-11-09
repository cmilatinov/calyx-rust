use super::*;
use crate::combat::ComponentCombatStats;
use crate::sentry::ComponentSentry;
use crate::tank::{ComponentProjectile, ComponentProjectileTarget, ComponentTankController};
use crate::vision::{apply_fog, FogState, TeamVision, FADE_TIME};
use calyx_engine::scene::GameObjectRef;
use calyx_engine::test_support::{test_scene, HeadlessSceneRunner};
use nalgebra_glm::vec2;

fn tank(scene: &mut Scene, name: &str, x: f32, z: f32, team: u32) -> GameObject {
    let tank = scene.create(
        Some(ComponentID {
            name: name.into(),
            ..Default::default()
        }),
        None,
    );
    scene.set_world_transform(tank, Transform::from_xyz(x, 0.0, z).matrix());
    scene.add_component(
        tank,
        ComponentTankController {
            vision_range: 20.0,
            ..Default::default()
        },
    );
    scene.add_component(
        tank,
        ComponentRespawnState {
            team,
            ..Default::default()
        },
    );
    scene.add_component(tank, ComponentHealth::default());
    scene.add_component(
        tank,
        ComponentProjectileTarget {
            hit_radius: 1.0,
            ..Default::default()
        },
    );
    scene.add_component(
        tank,
        ComponentCombatStats {
            team,
            ..Default::default()
        },
    );
    tank
}

/// A patch of brush at `(x, z)`: a 4 x 4 m box volume resting on the ground,
/// and one clump of grass.
fn patch(scene: &mut Scene, x: f32, z: f32) -> GameObject {
    let patch = scene.create(
        Some(ComponentID {
            name: "Brush".into(),
            ..Default::default()
        }),
        None,
    );
    scene.set_world_transform(patch, Transform::from_xyz(x, 0.0, z).matrix());
    scene.add_component(patch, ComponentBrush::default());
    let volume = scene.create(None, Some(patch));
    scene.set_transform(volume, &Transform::from_xyz(0.0, 0.75, 0.0).matrix());
    scene.add_component(
        volume,
        ComponentCollider {
            shape: ColliderShape::Cuboid {
                half_extents: vec3(2.0, 0.75, 2.0),
            },
            groups: crate::collision::BRUSH,
            ..Default::default()
        },
    );
    let clump = scene.create(None, Some(patch));
    scene.set_transform(clump, &Transform::from_xyz(1.0, 0.0, 0.0).matrix());
    patch
}

fn sees(scene: &Scene, viewer: GameObject, target: GameObject) -> bool {
    let position = crate::vision::ground(scene.world_transform(target).position);
    TeamVision::in_scene(scene, viewer).sees_unit(scene.uuid(target), position)
}

#[test]
fn every_primitive_volume_holds_what_stands_in_it() {
    let mut scene = test_scene();
    let root = scene.create(None, None);
    scene.add_component(root, ComponentBrush::default());
    for (x, shape) in [
        (0.0, ColliderShape::Sphere { radius: 1.0 }),
        (
            10.0,
            ColliderShape::Capsule {
                orientation: Orientation::X,
                height: 1.0,
                radius: 0.6,
            },
        ),
        (
            20.0,
            ColliderShape::Cuboid {
                half_extents: vec3(1.0, 1.0, 1.0),
            },
        ),
        (
            30.0,
            ColliderShape::Cone {
                height: 1.0,
                radius: 1.0,
            },
        ),
    ] {
        let volume = scene.create(None, Some(root));
        scene.set_transform(volume, &Transform::from_xyz(x, 0.0, 0.0).matrix());
        scene.add_component(
            volume,
            ComponentCollider {
                shape,
                groups: crate::collision::BRUSH,
                ..Default::default()
            },
        );
    }
    let patches = BrushPatches::read(&scene);
    let inside = |x: f32, z: f32| patches.patch_at(vec3(x, -BODY_HEIGHT, z)).is_some();
    // The sphere, the capsule's ends along X, the cube's corner, and the
    // cone's base.
    assert!(inside(0.0, 0.0) && inside(0.6, 0.6) && !inside(0.8, 0.8));
    assert!(inside(11.5, 0.0) && !inside(10.0, 0.7));
    assert!(inside(20.9, 0.9) && !inside(21.1, 0.0));
    assert!(inside(30.0, 0.0) && !inside(30.0, 0.6) && inside(30.0, -0.3));
    assert!(!inside(5.0, 0.0), "between the volumes");

    // A disabled patch holds nothing.
    scene.set_component_enabled::<ComponentBrush>(root, false);
    assert!(BrushPatches::read(&scene).is_empty());
}

#[test]
fn a_tank_in_brush_is_hidden_from_tanks_outside_it() {
    let mut scene = test_scene();
    let player = tank(&mut scene, "Player", 0.0, 0.0, 1);
    let enemy = tank(&mut scene, "Enemy", 8.0, 1.0, 2);
    assert!(sees(&scene, player, enemy), "in range and in the open");

    // The same tank, the same distance away, in a patch: hidden.
    patch(&mut scene, 8.0, 0.0);
    assert!(!sees(&scene, player, enemy), "hidden in brush");
    assert!(
        TeamVision::in_scene(&scene, player).visible(vec2(8.0, 1.0)),
        "the ground under it stays in sight"
    );
    assert!(sees(&scene, enemy, player), "tanks in brush see out of it");

    // Fog hides its model once it has faded out.
    let mut fog = FogState::default();
    apply_fog(&mut scene, player, &mut fog);
    fog.advance(FADE_TIME * 1.1);
    apply_fog(&mut scene, player, &mut fog);
    assert!(!scene.is_visible(enemy), "fog hides the tank in brush");
    assert_eq!(fog.presence(scene.uuid(enemy)), Some(0.0));
    assert!(scene.is_visible(player));
}

#[test]
fn tanks_in_the_same_patch_see_each_other() {
    let mut scene = test_scene();
    let player = tank(&mut scene, "Player", 0.0, 0.0, 1);
    patch(&mut scene, 8.0, 0.0);
    let other = patch(&mut scene, 0.0, 8.0);
    let enemy = tank(&mut scene, "Enemy", 8.0, 1.0, 2);
    assert!(!sees(&scene, player, enemy));

    // Another patch does not count.
    scene.set_world_transform(player, Transform::from_xyz(0.0, 0.0, 8.0).matrix());
    assert!(BrushPatches::read(&scene).patch_of(&scene, player) == Some(scene.uuid(other)));
    assert!(!sees(&scene, player, enemy), "from another patch");

    scene.set_world_transform(player, Transform::from_xyz(7.0, 0.0, -1.0).matrix());
    assert!(sees(&scene, player, enemy), "from the same patch");
    assert!(sees(&scene, enemy, player), "and the other way round");

    // An ally in the patch lends the side its sight of it.
    scene.set_world_transform(player, Transform::from_xyz(0.0, 0.0, 0.0).matrix());
    let ally = tank(&mut scene, "Ally", 9.0, -1.0, 1);
    assert!(sees(&scene, player, enemy), "an ally shares the patch");
    let _ = scene.write_component::<ComponentRespawnState, _>(ally, |state| state.alive = false);
    assert!(!sees(&scene, player, enemy), "a dead ally sees nothing");
}

#[test]
fn firing_from_brush_reveals_the_shooter_until_the_reveal_expires() {
    let mut scene = test_scene();
    let player = tank(&mut scene, "Player", 0.0, 0.0, 1);
    let brush = patch(&mut scene, 8.0, 0.0);
    let _ = scene.write_component::<ComponentBrush, _>(brush, |brush| brush.reveal_time = 2.0);
    let enemy = tank(&mut scene, "Enemy", 8.0, 0.0, 2);
    assert!(!sees(&scene, player, enemy));

    let shooter = GameObjectRef::new(scene.uuid(enemy));
    crate::tank::spawn_round(
        &mut scene,
        vec3(9.0, 0.5, 0.0),
        Default::default(),
        ComponentProjectile {
            shooter,
            ..Default::default()
        },
    );
    assert!(sees(&scene, player, enemy), "the shot gives it away");
    let reveal = |scene: &Scene| {
        scene
            .read_component::<ComponentBrushReveal, _, _>(enemy, |reveal| reveal.remaining)
            .unwrap()
    };
    assert_eq!(reveal(&scene), 2.0, "for the patch's reveal time");

    let _ = scene.write_component::<ComponentBrushReveal, _>(enemy, |reveal| reveal.advance(1.9));
    assert!(sees(&scene, player, enemy), "still revealed");
    let _ = scene.write_component::<ComponentBrushReveal, _>(enemy, |reveal| reveal.advance(0.2));
    assert!(
        !sees(&scene, player, enemy),
        "hidden once the reveal expires"
    );

    // Firing outside brush reveals nothing: the shooter is in the open.
    scene.set_world_transform(player, Transform::from_xyz(-10.0, 0.0, 0.0).matrix());
    let shooter = GameObjectRef::new(scene.uuid(player));
    crate::tank::spawn_round(
        &mut scene,
        vec3(-9.0, 0.5, 0.0),
        Default::default(),
        ComponentProjectile {
            shooter,
            ..Default::default()
        },
    );
    assert!(scene
        .read_component::<ComponentBrushReveal, _, _>(player, |_| ())
        .is_none());
}

#[test]
fn the_reveal_counts_down_in_play() {
    let mut scene = test_scene();
    let enemy = tank(&mut scene, "Enemy", 0.0, 0.0, 2);
    scene.add_component(enemy, ComponentBrushReveal { remaining: 0.5 });
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.set_step_seconds(0.1);
    runner.step_many(10);
    let remaining = runner
        .scene()
        .read_component::<ComponentBrushReveal, _, _>(enemy, |reveal| reveal.remaining)
        .unwrap();
    assert_eq!(remaining, 0.0);
}

#[test]
fn a_sentry_outside_the_patch_holds_fire_at_a_tank_in_it() {
    let mut scene = test_scene();
    let target = tank(&mut scene, "Target", 0.0, 6.0, 1);
    let sentry = scene.create(None, None);
    scene.set_world_transform(sentry, Transform::from_xyz(0.0, 0.5, 0.0).matrix());
    scene.add_component(
        sentry,
        ComponentSentry {
            target: GameObjectRef::new(scene.uuid(target)),
            range: 12.0,
            ..Default::default()
        },
    );
    let rounds = |scene: &Scene| {
        scene
            .objects()
            .filter(|&object| {
                scene
                    .read_component::<ComponentProjectile, _, _>(object, |_| ())
                    .is_some()
            })
            .count()
    };
    let brush = patch(&mut scene, 0.0, 6.0);
    let _ = brush;
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.set_step_seconds(0.1);
    runner.step_many(5);
    assert_eq!(rounds(runner.scene()), 0, "the sentry cannot see the tank");

    // Out of the brush, it fires.
    runner
        .scene_mut()
        .set_world_transform(target, Transform::from_xyz(0.0, 0.0, 9.0).matrix());
    runner.set_step_seconds(0.1);
    runner.step_many(2);
    assert!(
        rounds(runner.scene()) > 0,
        "the sentry fires once it sees it"
    );
}

#[test]
fn brush_volumes_neither_block_sight_nor_count_as_arena_cover() {
    let mut scene = test_scene();
    let player = tank(&mut scene, "Player", 0.0, 0.0, 1);
    patch(&mut scene, 5.0, 0.0);
    let vision = TeamVision::in_scene(&scene, player);
    assert!(vision.cover.is_empty());
    assert!(vision.visible(vec2(9.0, 0.0)), "sight crosses the patch");
}

#[test]
fn grass_bends_for_seen_tanks_and_parts_only_around_the_players_patch() {
    let mut scene = test_scene();
    let player = tank(&mut scene, "Player", 0.0, 0.0, 1);
    scene.add_component(player, ComponentTankHud::default());
    let field = scene.create(None, None);
    scene.add_component(field, ComponentGrassField::default());
    patch(&mut scene, 8.0, 0.0);
    let enemy = tank(&mut scene, "Enemy", 8.0, 1.0, 2);
    let ally = tank(&mut scene, "Ally", 8.5, -1.0, 1);
    let outside = tank(&mut scene, "Outside", -6.0, 0.0, 2);

    let reveal = |benders: &[GrassBender], at: (f32, f32)| {
        benders
            .iter()
            .find(|b| b.position.x == at.0 && b.position.z == at.1)
            .map(|b| b.reveal_radius)
    };
    // Out of the brush, every seen tank bends the grass and none parts it.
    let benders = grass_benders(&scene);
    assert_eq!(benders.len(), 4);
    assert!(benders
        .iter()
        .all(|b| b.bend_radius > 0.0 && b.reveal_radius == 0.0));

    // Fog hid the enemy in the brush: it must not move the grass at all.
    let _ = scene.write_component::<ComponentID, _>(enemy, |id| id.visible = false);
    let benders = grass_benders(&scene);
    assert_eq!(
        reveal(&benders, (8.0, 1.0)),
        None,
        "a hidden tank gives nothing away"
    );

    // In the patch, the player and the tanks it sees there part the grass.
    let _ = scene.write_component::<ComponentID, _>(enemy, |id| id.visible = true);
    scene.set_world_transform(player, Transform::from_xyz(7.0, 0.0, 0.0).matrix());
    let benders = grass_benders(&scene);
    assert_eq!(benders[0].position.x, 7.0, "the player's own comes first");
    assert_eq!(reveal(&benders, (7.0, 0.0)), Some(REVEAL_RADIUS));
    assert_eq!(reveal(&benders, (8.0, 1.0)), Some(REVEAL_RADIUS));
    assert_eq!(reveal(&benders, (8.5, -1.0)), Some(REVEAL_RADIUS));
    assert_eq!(
        reveal(&benders, (-6.0, 0.0)),
        Some(0.0),
        "outside the patch"
    );
    let _ = (ally, outside);

    // In play the first patch hands them to the field every frame.
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.step();
    let count = runner
        .scene()
        .read_component::<ComponentGrassField, _, _>(field, |field| field.benders.len())
        .unwrap();
    assert_eq!(count, 4);
}
