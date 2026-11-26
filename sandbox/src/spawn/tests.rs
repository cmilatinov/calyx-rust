use super::{
    find_spawn_transform, update_respawn_state, ComponentRespawnState, ComponentSpawnPoint,
    RespawnAction, SPAWN_TEAM_ANY,
};
use calyx_engine::math::Transform;
use nalgebra::UnitQuaternion;
use nalgebra_glm::vec3;

#[test]
fn death_request_starts_respawn_timer() {
    let mut state = ComponentRespawnState {
        respawn_delay: 4.0,
        invulnerability_remaining: 1.0,
        death_requested: true,
        ..Default::default()
    };

    assert_eq!(state.tick(0.5), None);
    assert!(!state.alive);
    assert_eq!(state.respawn_remaining, 4.0);
    assert_eq!(state.invulnerability_remaining, 0.0);
    assert!(!state.death_requested);
}

#[test]
fn dead_state_requests_spawn_when_timer_expires() {
    let mut state = ComponentRespawnState {
        alive: false,
        respawn_remaining: 0.25,
        ..Default::default()
    };

    assert_eq!(state.tick(0.5), Some(RespawnAction::Spawn));
    assert!(state.respawn_requested);
}

#[test]
fn live_respawn_request_does_not_bypass_next_death_delay() {
    let mut state = ComponentRespawnState {
        respawn_delay: 2.0,
        respawn_requested: true,
        ..Default::default()
    };

    assert_eq!(state.tick(0.0), None);
    assert!(state.alive);
    assert!(!state.respawn_requested);

    state.respawn_requested = true;
    state.death_requested = true;
    assert_eq!(state.tick(0.0), None);
    assert!(!state.alive);
    assert_eq!(state.respawn_remaining, 2.0);
    assert!(!state.respawn_requested);

    assert_eq!(state.tick(0.5), None);
    assert_eq!(state.respawn_remaining, 1.5);
}

#[test]
fn spawned_state_restores_alive_with_invulnerability() {
    let mut state = ComponentRespawnState {
        alive: false,
        respawn_requested: true,
        invulnerability_duration: 1.5,
        ..Default::default()
    };

    state.mark_spawned();

    assert!(state.alive);
    assert!(!state.respawn_requested);
    assert!(state.is_invulnerable());
    assert_eq!(state.invulnerability_remaining, 1.5);
}

#[test]
fn protected_respawn_state_locks_gameplay() {
    assert!(ComponentRespawnState {
        alive: false,
        ..Default::default()
    }
    .is_gameplay_locked());
    assert!(ComponentRespawnState {
        death_requested: true,
        ..Default::default()
    }
    .is_gameplay_locked());
    assert!(ComponentRespawnState {
        invulnerability_remaining: 1.0,
        ..Default::default()
    }
    .is_gameplay_locked());
    assert!(!ComponentRespawnState::default().is_gameplay_locked());
}

#[test]
fn spawn_lookup_prefers_matching_team_then_order() {
    let mut scene = calyx_engine::test_support::test_scene();
    let generic = scene.create(None, None);
    let team_late = scene.create(None, None);
    let team_first = scene.create(None, None);
    scene.set_world_transform(generic, Transform::from_xyz(1.0, 0.0, 0.0).matrix());
    scene.set_world_transform(team_late, Transform::from_xyz(2.0, 0.0, 0.0).matrix());
    scene.set_world_transform(team_first, Transform::from_xyz(3.0, 0.0, 0.0).matrix());
    scene.add_component(
        generic,
        ComponentSpawnPoint {
            team: SPAWN_TEAM_ANY,
            order: 0,
            ..Default::default()
        },
    );
    scene.add_component(
        team_late,
        ComponentSpawnPoint {
            team: 2,
            order: 10,
            ..Default::default()
        },
    );
    scene.add_component(
        team_first,
        ComponentSpawnPoint {
            team: 2,
            order: 2,
            ..Default::default()
        },
    );

    let transform = find_spawn_transform(&scene, 2).expect("team spawn should resolve");

    assert_eq!(transform.position, vec3(3.0, 0.0, 0.0));
}

#[test]
fn respawn_update_moves_object_to_spawn_point() {
    let mut scene = calyx_engine::test_support::test_scene();
    let spawn = scene.create(None, None);
    let player = scene.create(None, None);
    scene.set_world_transform(spawn, Transform::from_xyz(8.0, 0.0, 4.0).matrix());
    scene.set_world_transform(player, Transform::from_xyz(-4.0, 0.0, 0.0).matrix());
    scene.add_component(
        spawn,
        ComponentSpawnPoint {
            team: 3,
            ..Default::default()
        },
    );
    scene.add_component(
        player,
        ComponentRespawnState {
            team: 3,
            alive: false,
            respawn_remaining: 0.0,
            invulnerability_duration: 2.5,
            ..Default::default()
        },
    );

    assert!(update_respawn_state(&mut scene, player, 0.0));

    let player_transform = scene.world_transform(player);
    assert_eq!(player_transform.position, vec3(8.0, 0.0, 4.0));
    let state = scene
        .read_component::<ComponentRespawnState, _, _>(player, |state| *state)
        .expect("player should keep respawn state");
    assert!(state.alive);
    assert_eq!(state.invulnerability_remaining, 2.5);
}

#[test]
fn respawn_update_preserves_object_scale() {
    let mut scene = calyx_engine::test_support::test_scene();
    let spawn = scene.create(None, None);
    let player = scene.create(None, None);
    let mut spawn_transform = Transform::from_xyz(8.0, 0.0, 4.0);
    spawn_transform.scale = vec3(5.0, 5.0, 5.0);
    let mut player_transform = Transform::from_xyz(-4.0, 0.0, 0.0);
    player_transform.scale = vec3(0.5, 1.5, 2.0);
    scene.set_world_transform(spawn, spawn_transform.matrix());
    scene.set_world_transform(player, player_transform.matrix());
    scene.add_component(
        spawn,
        ComponentSpawnPoint {
            team: 3,
            ..Default::default()
        },
    );
    scene.add_component(
        player,
        ComponentRespawnState {
            team: 3,
            alive: false,
            respawn_remaining: 0.0,
            ..Default::default()
        },
    );

    assert!(update_respawn_state(&mut scene, player, 0.0));

    let updated = scene.world_transform(player);
    assert_eq!(updated.position, vec3(8.0, 0.0, 4.0));
    assert!((updated.scale - vec3(0.5, 1.5, 2.0)).magnitude() < 1e-6);
}

#[test]
fn respawn_update_places_the_controlled_object_and_moves_its_complete_rig() {
    let mut scene = calyx_engine::test_support::test_scene();
    let spawn = scene.create(None, None);
    let player = scene.create(None, None);
    let tank = scene.create(None, None);
    let camera = scene.create(None, None);
    let crosshair = scene.create(None, None);
    scene.set_parent(tank, Some(player)).unwrap();
    scene.set_parent(camera, Some(player)).unwrap();
    scene.set_parent(crosshair, Some(player)).unwrap();
    scene.set_transform(tank, &Transform::from_xyz(2.0, 0.0, 0.0).matrix());
    scene.set_transform(camera, &Transform::from_xyz(0.0, 16.0, -10.0).matrix());
    scene.set_transform(crosshair, &Transform::from_xyz(0.0, 0.0, 6.0).matrix());
    scene.set_world_transform(spawn, Transform::from_xyz(8.0, 0.0, 4.0).matrix());
    scene.set_world_transform(player, Transform::from_xyz(-4.0, 0.0, 0.0).matrix());
    scene.add_component(spawn, ComponentSpawnPoint::default());
    scene.add_component(
        tank,
        ComponentRespawnState {
            alive: false,
            respawn_remaining: 0.0,
            ..Default::default()
        },
    );

    assert!(update_respawn_state(&mut scene, tank, 0.0));

    assert_eq!(scene.world_transform(player).position, vec3(6.0, 0.0, 4.0));
    assert_eq!(scene.world_transform(tank).position, vec3(8.0, 0.0, 4.0));
    assert_eq!(
        scene.world_transform(camera).position,
        vec3(6.0, 16.0, -6.0)
    );
    assert_eq!(
        scene.world_transform(crosshair).position,
        vec3(6.0, 0.0, 10.0)
    );
}

#[test]
fn respawn_does_not_move_siblings_under_a_shared_group() {
    let mut scene = calyx_engine::test_support::test_scene();
    let spawn = scene.create(None, None);
    let players = scene.create(None, None);
    let player = scene.create(None, Some(players));
    let tank = scene.create(None, Some(player));
    let other_player = scene.create(None, Some(players));
    scene.set_world_transform(spawn, Transform::from_xyz(8.0, 0.0, 4.0).matrix());
    scene.set_world_transform(player, Transform::from_xyz(-4.0, 0.0, 0.0).matrix());
    scene.set_world_transform(other_player, Transform::from_xyz(12.0, 0.0, 0.0).matrix());
    scene.add_component(spawn, ComponentSpawnPoint::default());
    scene.add_component(
        tank,
        ComponentRespawnState {
            alive: false,
            respawn_remaining: 0.0,
            ..Default::default()
        },
    );

    assert!(update_respawn_state(&mut scene, tank, 0.0));

    assert_eq!(scene.world_transform(tank).position, vec3(8.0, 0.0, 4.0));
    assert_eq!(scene.world_transform(player).position, vec3(8.0, 0.0, 4.0));
    assert_eq!(
        scene.world_transform(other_player).position,
        vec3(12.0, 0.0, 0.0)
    );
}

#[test]
fn respawn_preserves_rig_rotation_when_the_controlled_object_has_turned() {
    let mut scene = calyx_engine::test_support::test_scene();
    let spawn = scene.create(None, None);
    let player = scene.create(None, None);
    let tank = scene.create(None, None);
    let camera = scene.create(None, None);
    scene.set_parent(tank, Some(player)).unwrap();
    scene.set_parent(camera, Some(player)).unwrap();
    scene.set_transform(
        tank,
        &Transform::from_components(
            vec3(2.0, 0.0, 0.0),
            UnitQuaternion::from_euler_angles(0.0, 1.0, 0.0),
            vec3(1.0, 1.0, 1.0),
        )
        .matrix(),
    );
    scene.set_transform(
        camera,
        &Transform::from_components(
            vec3(0.0, 16.0, -10.0),
            UnitQuaternion::from_euler_angles(0.8, 0.2, 0.0),
            vec3(1.0, 1.0, 1.0),
        )
        .matrix(),
    );
    let spawn_transform = Transform::from_components(
        vec3(8.0, 0.0, 4.0),
        UnitQuaternion::from_euler_angles(0.0, -0.5, 0.0),
        vec3(1.0, 1.0, 1.0),
    );
    scene.set_world_transform(spawn, spawn_transform.matrix());
    scene.set_world_transform(player, Transform::from_xyz(-4.0, 0.0, 0.0).matrix());
    scene.add_component(spawn, ComponentSpawnPoint::default());
    scene.add_component(
        tank,
        ComponentRespawnState {
            alive: false,
            respawn_remaining: 0.0,
            ..Default::default()
        },
    );
    let camera_rotation = scene.world_transform(camera).rotation;

    assert!(update_respawn_state(&mut scene, tank, 0.0));

    assert_eq!(scene.world_transform(tank).position, vec3(8.0, 0.0, 4.0));
    assert!(
        scene
            .world_transform(tank)
            .rotation
            .angle_to(&spawn_transform.rotation)
            < 1e-5
    );
    assert_eq!(scene.world_transform(camera).rotation, camera_rotation);
}
