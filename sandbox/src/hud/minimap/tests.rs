use super::*;
use crate::arena::ComponentArena;
use calyx_engine::component::{ColliderShape, ComponentCollider};
use calyx_engine::math::Transform;
use nalgebra_glm::{vec3, Vec3};

#[test]
fn playable_contours_remove_shared_tile_seams_and_keep_obstacle_edges() {
    let floor = GroundRect::axis_aligned(vec2(0.0, 0.0), vec2(20.0, 20.0));
    let cover = [
        GroundRect::axis_aligned(vec2(-1.0, 0.0), vec2(2.0, 2.0)),
        GroundRect::axis_aligned(vec2(1.0, 0.0), vec2(2.0, 2.0)),
    ];
    let edges = playable_boundary(Some(floor), &cover);
    assert!(!edges
        .iter()
        .any(|(a, b)| a.x.abs() < 1e-4 && b.x.abs() < 1e-4));
    let length: f32 = edges.iter().map(|(a, b)| (b - a).norm()).sum();
    assert!(
        (length - 92.0).abs() < 1e-3,
        "floor80 + obstacle12, got{length}"
    );
    let wall = [GroundRect::axis_aligned(vec2(0.0, -9.0), vec2(20.0, 2.0))];
    let edges = playable_boundary(Some(floor), &wall);
    assert!(!edges.iter().any(|(a, b)| a.y < -9.9 && b.y < -9.9));
    assert!(edges
        .iter()
        .any(|(a, b)| (a.y + 8.0).abs() < 1e-4 && (b.y + 8.0).abs() < 1e-4));
}

#[test]
fn authored_cover_uses_current_collider_poses_instead_of_grid_cells() {
    let mut scene = calyx_engine::test_support::test_scene();
    let arena = scene.create(None, None);
    scene.set_transform(arena, &Transform::from_xyz(0.0, 0.0, 11.0).matrix());
    scene.add_component(
        arena,
        ComponentArena {
            generate_on_start: false,
            ..Default::default()
        },
    );
    let cover = scene.create(
        Some(calyx_engine::component::ComponentID {
            name: "Arena Cover Test".into(),
            ..Default::default()
        }),
        Some(arena),
    );
    scene.add_component(
        cover,
        ComponentCollider {
            shape: ColliderShape::Cuboid {
                half_extents: vec3(1.2, 0.6, 0.4),
            },
            ..Default::default()
        },
    );
    scene.set_transform(
        cover,
        &Transform::from_components(
            vec3(4.0, 0.6, 1.0),
            nalgebra::UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0),
            vec3(1.0, 1.0, 1.0),
        )
        .matrix(),
    );
    let player = scene.create(None, None);
    let state = minimap_state(&scene, player);
    assert_eq!(state.cover.len(), 1);
    assert_eq!(state.cover[0].center, vec2(4.0, 12.0));
    assert!((state.cover[0].half_axes[0].norm() - 1.2).abs() < 1e-4);
    assert!((state.cover[0].half_axes[1].norm() - 0.4).abs() < 1e-4);
    assert!(state.cover[0].half_axes[0].x.abs() < 1e-4);
    scene.set_transform(cover, &Transform::from_xyz(8.0, 0.6, 1.0).matrix());
    assert_eq!(
        minimap_state(&scene, player).cover[0].center,
        vec2(8.0, 12.0)
    );
}

#[test]
fn interior_walls_are_outlined_and_block_sight_while_the_perimeter_bounds_the_floor() {
    let mut scene = calyx_engine::test_support::test_scene();
    let arena = scene.create(None, None);
    scene.add_component(
        arena,
        ComponentArena {
            generate_on_start: false,
            ..Default::default()
        },
    );
    let named = |scene: &mut Scene, name: &str, parent| {
        scene.create(
            Some(calyx_engine::component::ComponentID {
                name: name.into(),
                ..Default::default()
            }),
            Some(parent),
        )
    };
    let solid = |scene: &mut Scene, object, position: Vec3, half_extents| {
        scene.set_transform(
            object,
            &Transform::from_xyz(position.x, 1.0, position.z).matrix(),
        );
        scene.add_component(
            object,
            ComponentCollider {
                shape: ColliderShape::Cuboid { half_extents },
                ..Default::default()
            },
        );
    };
    // A perimeter strip, as the arena's own child, and an interior wall
    // run under its wall.
    let perimeter = named(&mut scene, "Arena Wall North collision", arena);
    solid(
        &mut scene,
        perimeter,
        vec3(0.0, 0.0, 9.0),
        vec3(10.0, 1.0, 0.6),
    );
    let wall = named(&mut scene, "Arena Wall Middle", arena);
    let run = named(&mut scene, "Arena Wall Middle collision 0", wall);
    solid(&mut scene, run, vec3(2.0, 0.0, 0.0), vec3(0.6, 1.0, 3.0));
    let player = scene.create(None, None);

    let geometry = arena_geometry(&scene).unwrap();
    assert_eq!(geometry.sight_blockers.len(), 2, "both walls block sight");
    let state = minimap_state(&scene, player);
    assert_eq!(state.cover.len(), 1, "only the interior wall is cover");
    assert_eq!(state.cover[0].center, vec2(2.0, 0.0));
    // The map outlines the interior wall's four sides.
    let edges = playable_boundary(state.floor, &state.cover);
    assert!(edges
        .iter()
        .any(|(a, b)| (a.x - 1.4).abs() < 1e-4 && (b.x - 1.4).abs() < 1e-4));
    assert!(edges
        .iter()
        .any(|(a, b)| (a.x - 2.6).abs() < 1e-4 && (b.x - 2.6).abs() < 1e-4));
}

#[test]
fn transformed_cover_and_floor_share_world_geometry() {
    let mut transform = Transform::from_xyz(10.0, 0.0, 3.0);
    transform.rotation =
        nalgebra::UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0);
    let cover = GroundRect::placed(&transform, vec3(0.0, 0.0, 0.0), vec2(4.0, 2.0));
    assert!((cover_entry(vec2(5.0, 3.0), vec2(15.0, 3.0), cover).unwrap() - 0.4).abs() < 1e-4);
    let view = MinimapView::fitting(cover, vec2(1.0, 0.0));
    let rect = UiRect::from_min_size(UiPoint::ZERO, UiSize::new(100.0, 100.0));
    assert_eq!(view.map_point(cover.center, rect), UiPoint::new(50.0, 50.0));
}

#[test]
fn contour_clipping_handles_crossings_outside_and_tangents() {
    assert_eq!(
        clip_line_circle((-20.0, 0.0), (20.0, 0.0), (0.0, 0.0), 10.0),
        Some(((-10.0, 0.0), (10.0, 0.0)))
    );
    assert!(clip_line_circle((-20.0, 11.0), (20.0, 11.0), (0.0, 0.0), 10.0).is_none());
    assert!(clip_line_circle((-20.0, 10.0), (20.0, 10.0), (0.0, 0.0), 10.0).is_none());
    assert_eq!(
        clip_line_circle((0.0, 0.0), (5.0, 0.0), (0.0, 0.0), 10.0),
        Some(((0.0, 0.0), (5.0, 0.0)))
    );
}

#[test]
fn contour_cache_reuses_geometry_and_invalidates_moved_cover() {
    let floor = Some(GroundRect::axis_aligned(vec2(0.0, 0.0), vec2(20.0, 20.0)));
    let mut cover = [GroundRect::axis_aligned(vec2(2.0, 0.0), vec2(2.0, 2.0))];
    let mut cache = BoundaryCache::default();
    let original = cache.edges(floor, &cover).to_vec();
    let allocation = cache.edges.as_ptr();
    assert_eq!(cache.edges(floor, &cover), original);
    assert_eq!(cache.edges.as_ptr(), allocation);
    cover[0].center.x = 4.0;
    assert_ne!(cache.edges(floor, &cover), original);
    assert_eq!(cache.edges(None, &[]).len(), 0);
}

#[test]
fn contour_cache_keeps_the_outline_while_cover_creeps_within_the_tolerance() {
    let floor = Some(GroundRect::axis_aligned(vec2(0.0, 0.0), vec2(20.0, 20.0)));
    let mut cover = [GroundRect::axis_aligned(vec2(2.0, 0.0), vec2(2.0, 2.0))];
    let mut cache = BoundaryCache::default();
    let original = cache.edges(floor, &cover).to_vec();
    let allocation = cache.edges.as_ptr();
    // A prop scaled a hair every frame keeps the traced outline...
    for _ in 0..10 {
        cover[0].half_axes[0].x -= OUTLINE_TOLERANCE / 20.0;
        assert_eq!(cache.edges(floor, &cover), original);
        assert_eq!(cache.edges.as_ptr(), allocation);
    }
    // ...until it has moved past the tolerance since it was traced.
    cover[0].half_axes[0].x -= OUTLINE_TOLERANCE;
    assert_eq!(cache.edges(floor, &cover), playable_boundary(floor, &cover));
    assert_ne!(cache.edges(floor, &cover), original);
}

#[test]
fn live_cover_moves_and_visibility_changes_follow_generated_children() {
    let mut scene = calyx_engine::test_support::test_scene();
    let player = scene.create(None, None);
    scene.add_component(player, ComponentTankController::default());
    let arena = scene.create(None, None);
    scene.add_component(arena, ComponentArena::default());
    let cover = scene.create(None, Some(arena));
    scene.add_component(cover, ComponentCollider::default());
    scene.set_transform(
        cover,
        &Transform::from_components(vec3(3.0, 1.0, 0.0), Default::default(), vec3(2.0, 2.0, 2.0))
            .matrix(),
    );
    assert!(!minimap_state(&scene, player).vision.visible(vec2(6.0, 0.0)));
    scene.set_transform(
        cover,
        &Transform::from_components(vec3(3.0, 1.0, 5.0), Default::default(), vec3(2.0, 2.0, 2.0))
            .matrix(),
    );
    assert!(minimap_state(&scene, player).vision.visible(vec2(6.0, 0.0)));
}

#[test]
fn a_tank_blocker_leaves_the_minimap_seeing_across_it() {
    let mut scene = calyx_engine::test_support::test_scene();
    let player = scene.create(None, None);
    scene.add_component(player, ComponentTankController::default());
    let arena = scene.create(None, None);
    scene.add_component(arena, ComponentArena::default());
    let cover = scene.create(None, Some(arena));
    scene.add_component(cover, ComponentCollider::default());
    scene.set_transform(
        cover,
        &Transform::from_components(vec3(3.0, 1.0, 0.0), Default::default(), vec3(2.0, 2.0, 2.0))
            .matrix(),
    );
    assert!(!minimap_state(&scene, player).vision.visible(vec2(6.0, 0.0)));

    let _ = scene.write_component::<ComponentCollider, _>(cover, |collider| {
        collider.groups = crate::collision::TANK_BLOCKER;
    });
    let state = minimap_state(&scene, player);
    assert!(state.vision.visible(vec2(6.0, 0.0)));
    assert_eq!(state.cover.len(), 1, "the map still outlines the blocker");
}

#[test]
fn the_map_marks_only_units_and_paints_the_player_last() {
    let mut scene = calyx_engine::test_support::test_scene();
    let player = scene.create(None, None);
    scene.add_component(player, ComponentTankController::default());
    // Props that are not units get no blip, however many there are.
    for _ in 0..20 {
        let prop = scene.create(None, None);
        scene.add_component(prop, ComponentCollider::default());
    }
    let enemy = scene.create(None, None);
    scene.add_component(enemy, ComponentTankController::default());
    let target = scene.create(None, None);
    scene.add_component(target, ComponentProjectileTarget::default());

    let blips = minimap_state(&scene, player).blips;
    let mut ids = blips.iter().map(|blip| blip.id).collect::<Vec<_>>();
    assert_eq!(ids.len(), 3);
    assert_eq!(blips.last().map(|blip| blip.kind), Some(BlipKind::Player));
    ids.sort();
    let mut expected = vec![scene.uuid(player), scene.uuid(enemy), scene.uuid(target)];
    expected.sort();
    assert_eq!(ids, expected);
}

#[test]
fn sight_rays_are_cast_again_only_when_the_sight_changes() {
    use calyx_engine::component::{FogOfWarBlocker, FogOfWarViewer};
    let viewer = FogOfWarViewer {
        position: vec2(0.0, 0.0),
        fade_start: 10.0,
        fade_end: 12.0,
        fade_curve: 0.0,
        weight: 1.0,
    };
    let mut sight = FogOfWarSight {
        viewers: vec![viewer],
        blockers: vec![FogOfWarBlocker {
            center: vec2(4.0, 0.0),
            half_axes: [vec2(1.0, 0.0), vec2(0.0, 1.0)],
        }],
        border: Default::default(),
    };
    let mut cache = BoundaryCache::default();
    let first = cache.sight_rays(&sight, 64).to_vec();
    assert_eq!(first, sight_rays(&sight, 64));
    let kept = cache.sight_rays(&sight, 64).as_ptr();
    assert_eq!(
        cache.sight_rays(&sight, 64).as_ptr(),
        kept,
        "an unchanged sight is not cast again"
    );

    sight.viewers[0].position = vec2(0.0, 3.0);
    let moved = cache.sight_rays(&sight, 64).to_vec();
    assert_eq!(moved, sight_rays(&sight, 64));
    assert_ne!(moved, first);
    assert_eq!(cache.sight_rays(&sight, 32).len(), 1);
    assert_eq!(cache.sight_rays(&sight, 32)[0].through.len(), 32);
}
