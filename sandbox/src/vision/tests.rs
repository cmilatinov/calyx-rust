use super::*;
use crate::arena::ComponentArena;
use crate::combat::ComponentCombatStats;
use crate::tank::spawn_round;
use calyx_engine::component::ComponentRigidBody;
use calyx_engine::scene::GameObjectRef;
use calyx_engine::test_support::HeadlessSceneRunner;
use nalgebra::UnitQuaternion;
use rapier3d::dynamics::RigidBodyType;

fn viewer(x: f32, z: f32, range: f32) -> Viewer {
    Viewer {
        id: Uuid::nil(),
        position: vec2(x, z),
        range,
    }
}

#[test]
fn visibility_needs_range_and_a_clear_line_from_any_viewer() {
    let wall = GroundRect::axis_aligned(vec2(4.0, 0.0), vec2(2.0, 4.0));
    let vision = TeamVision {
        viewers: vec![viewer(0.0, 0.0, 10.0)],
        cover: vec![wall],
    };
    assert!(vision.visible(vec2(2.0, 0.0)));
    assert!(!vision.visible(vec2(6.0, 0.0)), "behind cover");
    assert!(vision.visible(vec2(6.0, 6.0)), "around cover");
    assert!(!vision.visible(vec2(0.0, 11.0)), "beyond range");
    assert!((vision.sight_rays(4)[0].through[0] - 5.0).abs() < 1e-4);

    // A second tank on the side sees behind the wall from the far side.
    let vision = TeamVision {
        viewers: vec![viewer(0.0, 0.0, 10.0), viewer(12.0, 0.0, 10.0)],
        cover: vec![wall],
    };
    assert!(vision.visible(vec2(6.0, 0.0)));
    assert!(!vision.visible(vec2(6.0, 30.0)));
    assert!(
        !TeamVision::default().visible(Vec2::zeros()),
        "no tank, no sight"
    );
}

#[test]
fn units_on_touching_cover_are_seen_as_the_fog_lights_them() {
    // Two touching crates in a row, then a gap and a third.
    let vision = TeamVision {
        viewers: vec![viewer(0.0, 0.0, 10.0)],
        cover: vec![
            GroundRect::axis_aligned(vec2(4.0, 0.0), vec2(1.0, 1.0)),
            GroundRect::axis_aligned(vec2(5.0, 0.0), vec2(1.0, 1.0)),
            GroundRect::axis_aligned(vec2(7.5, 0.0), vec2(1.0, 1.0)),
        ],
    };
    assert!(vision.visible(vec2(3.8, 0.0)), "on the first crate");
    assert!(vision.visible(vec2(5.2, 0.0)), "on the crate touching it");
    assert!(!vision.visible(vec2(6.2, 0.0)), "in the gap behind them");
    assert!(!vision.visible(vec2(7.5, 0.0)), "on the crate past the gap");
}

#[test]
fn sight_rays_carry_through_touching_blocks_of_cover() {
    let vision = TeamVision {
        viewers: vec![viewer(0.0, 0.0, 10.0)],
        cover: vec![
            GroundRect::axis_aligned(vec2(4.0, 0.0), vec2(2.0, 2.0)),
            GroundRect::axis_aligned(vec2(6.0, 0.0), vec2(2.0, 2.0)),
        ],
    };
    let rays = vision.sight_rays(8);
    assert!((rays[0].through[0] - 7.0).abs() < 1e-4);
    // Rays that meet no cover reach the full range.
    assert_eq!(rays[0].through[2], 10.0);
}

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
            vision_range: 10.0,
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

#[test]
fn team_vision_comes_from_every_live_friendly_tank_only() {
    let mut scene = calyx_engine::test_support::test_scene();
    let player = tank(&mut scene, "Player", 0.0, 0.0, 1);
    let ally = tank(&mut scene, "Ally", 30.0, 0.0, 1);
    let enemy = tank(&mut scene, "Enemy", 0.0, 30.0, 2);

    let vision = TeamVision::of_player(&scene, player, Vec::new());
    assert_eq!(vision.viewers.len(), 2);
    assert!(vision.visible(vec2(35.0, 0.0)), "the ally lends its sight");
    assert!(!vision.visible(vec2(0.0, 35.0)), "the enemy does not");
    assert!(is_friendly(&scene, player, ally));
    assert!(!is_friendly(&scene, player, enemy));

    let _ = scene.write_component::<ComponentRespawnState, _>(ally, |state| state.alive = false);
    let vision = TeamVision::of_player(&scene, player, Vec::new());
    assert!(!vision.visible(vec2(35.0, 0.0)), "a dead ally sees nothing");
}

/// An authored arena with one wall-height crate across +X at x = 4, a player
/// tank at the origin with the tank HUD, and a sentry behind the crate, with
/// a light on its model.
fn sentry_behind_cover() -> (Scene, GameObject, GameObject, GameObject) {
    sentry_behind("Arena Cover Stack Crate")
}

/// [`sentry_behind_cover`], with the cover named `cover_name`.
fn sentry_behind(cover_name: &str) -> (Scene, GameObject, GameObject, GameObject) {
    let mut scene = calyx_engine::test_support::test_scene();
    let arena = scene.create(None, None);
    scene.add_component(
        arena,
        ComponentArena {
            generate_on_start: false,
            ..Default::default()
        },
    );
    let cover = scene.create(
        Some(ComponentID {
            name: cover_name.into(),
            ..Default::default()
        }),
        Some(arena),
    );
    scene.set_world_transform(cover, Transform::from_xyz(4.0, 1.0, 0.0).matrix());
    scene.add_component(
        cover,
        ComponentRigidBody {
            ty: RigidBodyType::Fixed,
            ..Default::default()
        },
    );
    scene.add_component(
        cover,
        ComponentCollider {
            shape: ColliderShape::Cuboid {
                half_extents: vec3(0.5, 1.0, 2.0),
            },
            ..Default::default()
        },
    );
    let player = tank(&mut scene, "Player", 0.0, 0.0, 1);
    scene.add_component(player, ComponentTankHud::default());

    let sentry = scene.create(
        Some(ComponentID {
            name: "Sentry".into(),
            ..Default::default()
        }),
        None,
    );
    scene.set_world_transform(sentry, Transform::from_xyz(7.0, 0.5, 0.0).matrix());
    let target = GameObjectRef::new(scene.uuid(player));
    scene.add_component(
        sentry,
        ComponentSentry {
            target,
            ..Default::default()
        },
    );
    scene.add_component(sentry, ComponentHealth::default());
    scene.add_component(sentry, ComponentProjectileTarget::default());
    scene.add_component(
        sentry,
        ComponentCollider {
            shape: ColliderShape::Sphere { radius: 0.5 },
            ..Default::default()
        },
    );
    let lamp = scene.create(None, Some(sentry));
    scene.add_component(lamp, ComponentPointLight::default());
    (scene, player, sentry, lamp)
}

#[test]
fn only_walls_boxes_and_hedgerows_block_sight() {
    for (cover, blocks) in [
        ("Arena Cover Stack Crate", true),
        ("Arena Cover Field Hedgerow", true),
        ("Arena Cover Left Supply", true),
        ("Arena Wall North 4", true),
        ("Arena Cover Centre Barrier", false),
        ("Arena Cover Right Sandbags", false),
        ("Arena Cover West Boulder", false),
        ("Arena Cover East Rock", false),
        ("Arena Cover Farm Hay Bale 0", false),
    ] {
        let (mut scene, player, sentry, _) = sentry_behind(cover);
        apply_fog(&mut scene, player, &mut FogState::default());
        assert_eq!(!scene.is_visible(sentry), blocks, "{cover}");
    }
}

#[test]
fn a_tank_behind_a_tank_blocker_stays_in_sight() {
    let (mut scene, player, sentry, _) = sentry_behind_cover();
    let enemy = tank(&mut scene, "Enemy", 7.0, 1.0, 2);
    let mut fog = FogState::default();
    apply_fog(&mut scene, player, &mut fog);
    assert!(!scene.is_visible(enemy), "the crate hides the tank");

    // Water stops tanks but not sight: as a tank-only blocker the same
    // collider hides neither the tank nor the sentry.
    let cover = scene
        .objects()
        .find(|object| scene.name(*object) == "Arena Cover Stack Crate")
        .unwrap();
    let _ = scene.write_component::<ComponentCollider, _>(cover, |collider| {
        collider.groups = crate::collision::TANK_BLOCKER;
    });
    apply_fog(&mut scene, player, &mut fog);
    assert!(scene.is_visible(enemy));
    assert!(scene.is_visible(sentry));
    assert!(!fog.hides(scene.uuid(enemy)));
    let geometry = arena_geometry(&scene).unwrap();
    assert!(geometry.sight_blockers.is_empty());
    assert_eq!(
        geometry.cover.len(),
        1,
        "it is still cover tanks drive round"
    );
}

#[test]
fn fog_hides_a_sentry_behind_cover_and_shows_it_once_in_sight() {
    let (mut scene, player, sentry, lamp) = sentry_behind_cover();
    let mut fog = FogState::default();

    apply_fog(&mut scene, player, &mut fog);
    assert!(!scene.is_visible(sentry), "cover hides the sentry");
    assert!(!scene.is_component_enabled::<ComponentPointLight>(lamp));
    assert!(fog.hides(scene.uuid(sentry)));
    assert!(scene.is_visible(player), "friendly units are always seen");
    assert!(
        scene.is_component_enabled::<ComponentCollider>(sentry),
        "fog changes only what the player sees"
    );

    // Driving round the cover brings the sentry into sight at once.
    scene.set_world_transform(player, Transform::from_xyz(4.0, 0.0, 5.0).matrix());
    apply_fog(&mut scene, player, &mut fog);
    assert!(scene.is_visible(sentry));
    assert!(scene.is_component_enabled::<ComponentPointLight>(lamp));
    assert!(!fog.hides(scene.uuid(sentry)));

    // Beyond vision range it fades out, then hides.
    fog.advance(1.0);
    scene.set_world_transform(player, Transform::from_xyz(-6.0, 0.0, 9.0).matrix());
    apply_fog(&mut scene, player, &mut fog);
    fog.advance(FADE_TIME * 0.9);
    apply_fog(&mut scene, player, &mut fog);
    assert!(scene.is_visible(sentry));
    fog.advance(FADE_TIME * 0.2);
    apply_fog(&mut scene, player, &mut fog);
    assert!(!scene.is_visible(sentry));
    assert!(!scene.is_component_enabled::<ComponentPointLight>(lamp));
}

#[test]
fn units_fade_in_and_out_of_sight_over_the_fade_time() {
    let (mut scene, player, sentry, lamp) = sentry_behind_cover();
    let id = scene.uuid(sentry);
    let mut fog = FogState::default();
    apply_fog(&mut scene, player, &mut fog);
    assert_eq!(fog.presence(id), Some(0.0), "unseen units are gone at once");

    // Coming into sight shows the unit at once and fades it in.
    scene.set_world_transform(player, Transform::from_xyz(4.0, 0.0, 5.0).matrix());
    apply_fog(&mut scene, player, &mut fog);
    assert!(scene.is_visible(sentry));
    assert_eq!(fog.presence(id), Some(0.0));
    fog.advance(FADE_TIME * 0.5);
    apply_fog(&mut scene, player, &mut fog);
    assert!((fog.presence(id).unwrap() - 0.5).abs() < 1e-4);
    fog.advance(FADE_TIME);
    assert_eq!(fog.presence(id), Some(1.0));

    // Going behind cover fades it out, light and all, before fog hides it.
    scene.set_world_transform(player, Transform::from_xyz(0.0, 0.0, 0.0).matrix());
    apply_fog(&mut scene, player, &mut fog);
    assert!(scene.is_visible(sentry));
    assert!(scene.is_component_enabled::<ComponentPointLight>(lamp));
    fog.advance(FADE_TIME * 0.25);
    apply_fog(&mut scene, player, &mut fog);
    assert!((fog.presence(id).unwrap() - 0.75).abs() < 1e-4);
    assert!(scene.is_visible(sentry), "still fading out");
    fog.advance(FADE_TIME * 0.75);
    apply_fog(&mut scene, player, &mut fog);
    assert!(!scene.is_visible(sentry));
    assert!(!scene.is_component_enabled::<ComponentPointLight>(lamp));
    assert_eq!(fog.presence(id), Some(0.0));

    // Seen again partway through fading out, it fades back in from there.
    scene.set_world_transform(player, Transform::from_xyz(4.0, 0.0, 5.0).matrix());
    apply_fog(&mut scene, player, &mut fog);
    fog.advance(FADE_TIME);
    scene.set_world_transform(player, Transform::from_xyz(0.0, 0.0, 0.0).matrix());
    apply_fog(&mut scene, player, &mut fog);
    fog.advance(FADE_TIME * 0.5);
    scene.set_world_transform(player, Transform::from_xyz(4.0, 0.0, 5.0).matrix());
    apply_fog(&mut scene, player, &mut fog);
    assert!(scene.is_visible(sentry));
    fog.advance(FADE_TIME * 0.25);
    assert!((fog.presence(id).unwrap() - 0.75).abs() < 1e-4);
}

#[test]
fn rounds_and_their_effects_hide_at_once_when_sight_of_them_is_lost() {
    let (mut scene, player, _, _) = sentry_behind_cover();
    let enemy = tank(&mut scene, "Enemy", 0.0, 40.0, 2);
    let shooter = GameObjectRef::new(scene.uuid(enemy));
    // Fired behind the cover, flying away from the player, who watches from
    // around the cover.
    scene.set_world_transform(player, Transform::from_xyz(4.0, 0.0, 5.0).matrix());
    spawn_round(
        &mut scene,
        vec3(7.0, 0.5, 0.0),
        UnitQuaternion::identity(),
        ComponentProjectile {
            direction: vec3(1.0, 0.0, 0.0),
            shooter,
            ..Default::default()
        },
    );
    let round = rounds(&scene)[0];
    let parts = scene
        .objects()
        .filter(|&object| {
            object == round
                || scene
                    .read_component::<ComponentCombatEffectLifetime, _, _>(object, |_| ())
                    .is_some()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        parts.len(),
        3,
        "the round, its muzzle flash and smoke trail"
    );

    // Seen long enough to be fully present.
    let mut fog = FogState::default();
    apply_fog(&mut scene, player, &mut fog);
    fog.advance(FADE_TIME);
    apply_fog(&mut scene, player, &mut fog);
    for &part in &parts {
        assert!(scene.is_visible(part), "seen: {}", scene.name(part));
    }

    // Back behind the cover: they hide the same frame.
    scene.set_world_transform(player, Transform::from_xyz(0.0, 0.0, 0.0).matrix());
    apply_fog(&mut scene, player, &mut fog);
    for part in parts {
        assert!(!scene.is_visible(part), "{}", scene.name(part));
    }
}

#[test]
fn fog_leaves_objects_hidden_by_others_alone() {
    let (mut scene, player, sentry, _) = sentry_behind_cover();
    let _ = scene.write_component::<ComponentID, _>(sentry, |id| id.visible = false);
    scene.set_world_transform(player, Transform::from_xyz(4.0, 0.0, 5.0).matrix());
    let mut fog = FogState::default();
    scene.set_world_transform(player, Transform::from_xyz(0.0, 0.0, 0.0).matrix());
    apply_fog(&mut scene, player, &mut fog);
    scene.set_world_transform(player, Transform::from_xyz(4.0, 0.0, 5.0).matrix());
    apply_fog(&mut scene, player, &mut fog);
    assert!(
        !scene.is_visible(sentry),
        "fog never shows what it did not hide"
    );
}

fn rounds(scene: &Scene) -> Vec<GameObject> {
    scene
        .objects()
        .filter(|&object| {
            scene
                .read_component::<ComponentProjectile, _, _>(object, |_| ())
                .is_some()
        })
        .collect()
}

fn health(scene: &Scene, object: GameObject) -> f32 {
    scene
        .read_component::<ComponentHealth, _, _>(object, |health| health.current_health)
        .unwrap()
}

#[test]
fn a_hidden_sentry_still_fires_and_its_round_shows_once_it_heads_for_the_player() {
    // Out of the player's 10 m sight, but inside the sentry's 16 m range.
    let mut scene = calyx_engine::test_support::test_scene();
    let player = tank(&mut scene, "Player", 0.0, 0.0, 1);
    scene.add_component(player, ComponentTankHud::default());
    let sentry = scene.create(None, None);
    scene.set_world_transform(sentry, Transform::from_xyz(0.0, 0.5, 14.0).matrix());
    let target = GameObjectRef::new(scene.uuid(player));
    scene.add_component(
        sentry,
        ComponentSentry {
            target,
            ..Default::default()
        },
    );
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.step();
    let scene = runner.scene();
    assert!(!scene.is_visible(sentry), "the sentry is out of sight");
    let fired = rounds(scene);
    assert_eq!(fired.len(), 1, "a hidden sentry still fires");
    assert!(
        scene.is_visible(fired[0]),
        "a round that will hit the player is always shown"
    );

    runner.step_many(90);
    assert!(
        health(runner.scene(), player) < 100.0,
        "the hidden sentry's rounds still hurt"
    );
}

#[test]
fn unseen_rounds_and_their_muzzle_effects_stay_hidden_and_hidden_units_take_damage() {
    let (mut scene, player, sentry, _) = sentry_behind_cover();
    // An enemy round far off, flying away from the player.
    let enemy = tank(&mut scene, "Enemy", 0.0, 40.0, 2);
    let shooter = GameObjectRef::new(scene.uuid(enemy));
    spawn_round(
        &mut scene,
        vec3(0.0, 0.5, 40.0),
        UnitQuaternion::identity(),
        ComponentProjectile {
            direction: vec3(1.0, 0.0, 0.0),
            shooter,
            ..Default::default()
        },
    );
    let round = rounds(&scene)[0];
    assert!(!scene.is_visible(round), "hidden the frame it is fired");
    let effects = scene
        .objects()
        .filter(|&object| {
            scene
                .read_component::<ComponentCombatEffectLifetime, _, _>(object, |_| ())
                .is_some()
        })
        .collect::<Vec<_>>();
    assert_eq!(effects.len(), 2, "muzzle flash and smoke trail");
    for effect in effects {
        assert!(!scene.is_visible(effect));
        assert!(!scene.is_component_enabled::<ComponentParticleSystem>(effect));
    }
    scene.delete(round);
    scene.flush_deletes();

    // The player's own round into the hidden sentry still damages it.
    let shooter = GameObjectRef::new(scene.uuid(player));
    spawn_round(
        &mut scene,
        vec3(5.5, 0.5, 0.0),
        UnitQuaternion::identity(),
        ComponentProjectile {
            direction: vec3(1.0, 0.0, 0.0),
            shooter,
            ..Default::default()
        },
    );
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.step_many(10);
    let scene = runner.scene();
    assert!(!scene.is_visible(sentry));
    assert!(
        health(scene, sentry) < 100.0,
        "a hidden sentry takes damage"
    );
}

#[test]
fn impacts_out_of_sight_stay_hidden_the_frame_they_spawn() {
    let (mut scene, _, _, _) = sentry_behind_cover();
    let enemy = tank(&mut scene, "Enemy", 0.0, 40.0, 2);
    let shooter = GameObjectRef::new(scene.uuid(enemy));
    // Start the scene so the cover's collider is in the physics world.
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.step();
    let scene = runner.scene_mut();
    // Behind the cover, out of the player's sight: one round into the
    // hidden sentry and one into the back of the cover.
    spawn_round(
        scene,
        vec3(6.2, 0.5, 0.0),
        UnitQuaternion::identity(),
        ComponentProjectile {
            direction: vec3(1.0, 0.0, 0.0),
            shooter,
            ..Default::default()
        },
    );
    spawn_round(
        scene,
        vec3(5.2, 0.5, 1.5),
        UnitQuaternion::identity(),
        ComponentProjectile {
            direction: vec3(-1.0, 0.0, 0.0),
            shooter,
            ..Default::default()
        },
    );
    // The sentry's own rounds, fired at the player into the cover, are
    // out of sight too.
    for round in rounds(scene) {
        crate::tank::update_projectile(scene, round, 0.1);
    }
    let named = |name: &str| {
        scene
            .objects()
            .filter(|&object| {
                scene
                    .read_component::<ComponentID, _, _>(object, |id| id.name == name)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>()
    };
    let sparks = named("Tank Impact Sparks");
    let dust = named("Cover Impact Dust");
    assert!(!sparks.is_empty(), "the round hits the sentry");
    assert!(!dust.is_empty(), "the round hits the cover");
    for impact in sparks.into_iter().chain(dust) {
        assert!(!scene.is_visible(impact));
        assert!(!scene.is_component_enabled::<ComponentParticleSystem>(impact));
    }
}
