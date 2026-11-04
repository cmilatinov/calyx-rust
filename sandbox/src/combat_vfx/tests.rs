use super::*;
use crate::tank::{spawn_round, ComponentProjectile};
use calyx_engine::test_support::{HeadlessSceneRunner, TestAssets};

fn effect_scene() -> (TestAssets, Scene) {
    // The GPU simulation needs its compute shaders; an empty test registry
    // silently has no particle kernels and would not exercise cleanup.
    let assets = TestAssets::engine();
    let scene = assets.asset_context().registries.lock_read().scene();
    (assets, scene)
}

#[test]
fn shell_mesh_and_signal_material_resolve_from_the_project_assets() {
    let copy = calyx_engine::test_support::TestAssets::game();
    let assets = copy.asset_context();
    let visual = shell_visual();
    assert!(visual
        .mesh
        .get_ref(&assets.registries.lock_read())
        .is_some());
    assert!(visual
        .material
        .get_ref(&assets.registries.lock_read())
        .is_some());
    assert!(!visual.cast_shadows);
}

#[test]
fn swept_hits_spawn_sparks_for_a_target_and_dust_for_static_cover() {
    use crate::tank::{ComponentHealth, ComponentProjectileTarget};
    use calyx_engine::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
    use rapier3d::prelude::RigidBodyType;

    for tank in [true, false] {
        let (_assets, mut scene) = effect_scene();
        let target = scene.create(None, None);
        scene.set_world_transform(target, Transform::from_xyz(0.0, 1.0, 3.0).matrix());
        if tank {
            scene.add_component(
                target,
                ComponentProjectileTarget {
                    hit_radius: 0.8,
                    ..Default::default()
                },
            );
            scene.add_component(target, ComponentHealth::default());
        } else {
            scene.add_component(
                target,
                ComponentRigidBody {
                    ty: RigidBodyType::Fixed,
                    ..Default::default()
                },
            );
            scene.add_component(
                target,
                ComponentCollider {
                    shape: ColliderShape::Cuboid {
                        half_extents: vec3(0.5, 0.5, 0.5),
                    },
                    ..Default::default()
                },
            );
        }
        shot(&mut scene);
        let mut runner = HeadlessSceneRunner::from_scene(scene).with_particles();
        runner.step_many(6);
        let name = if tank {
            "Tank Impact Sparks"
        } else {
            "Cover Impact Dust"
        };
        let impact = named(runner.scene(), name);
        assert!(runner.particles().unwrap().live_count(impact).unwrap() > 0);
        let position = runner.scene().world_transform(impact).position;
        assert!(
            position.z < 3.0,
            "impact belongs on the contact surface, not inside the target"
        );
        if tank {
            assert_eq!(
                runner
                    .scene()
                    .read_component::<ComponentHealth, _, _>(target, |h| h.current_health),
                Some(65.0)
            );
        }
        runner.step_many(60);
        assert_eq!(runner.scene().objects().count(), 1);
        assert_eq!(runner.particles().unwrap().systems().count(), 0);
    }
}

fn shot(scene: &mut Scene) {
    spawn_round(
        scene,
        vec3(0.0, 1.0, 0.0),
        UnitQuaternion::identity(),
        ComponentProjectile {
            lifetime_remaining: 0.3,
            ..Default::default()
        },
    );
}

fn named(scene: &Scene, name: &str) -> GameObject {
    scene
        .objects()
        .find(|&o| scene.name(o) == name)
        .expect(name)
}

#[test]
fn firing_emits_flash_and_world_space_smoke_then_drains_on_expiry() {
    let (_assets, mut scene) = effect_scene();
    shot(&mut scene);
    let flash = named(&scene, "Muzzle Flash");
    let trail = named(&scene, "Shell Smoke Trail");
    let mut runner = HeadlessSceneRunner::from_scene(scene).with_particles();
    runner.step();
    let particles = runner.particles().unwrap();
    assert_eq!(particles.live_count(flash), Some(7));
    assert!(particles.live_count(trail).unwrap() > 0);
    assert!(!runner
        .scene()
        .read_component::<ComponentParticleSystem, _, _>(trail, |s| s.local_space)
        .unwrap());
    runner.step_many(20);
    assert!(!runner
        .scene()
        .objects()
        .any(|o| runner.scene().name(o) == "Projectile"));
    assert!(!runner
        .scene()
        .read_component::<ComponentParticleSystem, _, _>(trail, |s| s.active)
        .unwrap());
    // Expiry stops emission but preserves smoke already in flight.
    assert!(runner.particles().unwrap().live_count(trail).unwrap() > 0);
    runner.step_many(40);
    assert_eq!(runner.scene().objects().count(), 0);
    assert_eq!(runner.particles().unwrap().systems().count(), 0);
}

#[test]
fn tank_and_cover_hits_have_distinct_effects_and_both_expire() {
    let (_assets, mut scene) = effect_scene();
    spawn_impact(&mut scene, vec3(2.0, 1.0, 0.0), true);
    spawn_impact(&mut scene, vec3(-2.0, 1.0, 0.0), false);
    let sparks = named(&scene, "Tank Impact Sparks");
    let dust = named(&scene, "Cover Impact Dust");
    let mut runner = HeadlessSceneRunner::from_scene(scene).with_particles();
    runner.step();
    assert_eq!(runner.particles().unwrap().live_count(sparks), Some(14));
    assert_eq!(runner.particles().unwrap().live_count(dust), Some(10));
    assert_eq!(
        runner
            .scene()
            .read_component::<ComponentParticleSystem, _, _>(sparks, |s| s.rendering.blend_mode),
        Some(ParticleBlendMode::Additive)
    );
    assert_eq!(
        runner
            .scene()
            .read_component::<ComponentParticleSystem, _, _>(dust, |s| s.rendering.blend_mode),
        Some(ParticleBlendMode::Alpha)
    );
    runner.step_many(45);
    assert_eq!(runner.scene().objects().count(), 0);
    assert_eq!(runner.particles().unwrap().systems().count(), 0);
}

#[test]
fn repeated_firing_and_impacts_keep_emitters_bounded_and_release_gpu_systems() {
    let (_assets, scene) = effect_scene();
    let mut runner = HeadlessSceneRunner::from_scene(scene).with_particles();
    runner.step();
    let started = std::time::Instant::now();
    for i in 0..120 {
        shot(runner.scene_mut());
        spawn_impact(runner.scene_mut(), vec3(0.0, 1.0, 4.0), i % 2 == 0);
        runner.step_many(6);
        assert!(
            runner.particles().unwrap().systems().count() > 0,
            "the GPU simulation must actually run"
        );
        assert!(
            runner.scene().objects().count() <= 30,
            "transient objects must stay bounded"
        );
        assert!(
            runner.particles().unwrap().systems().count() <= 25,
            "GPU emitter state must stay bounded"
        );
    }
    eprintln!("120 shots + impacts, 720 simulation frames: {:.3} ms/frame (GPU particle simulation, no rendering)",
        started.elapsed().as_secs_f64() * 1000.0 / 720.0);
    runner.step_many(90);
    assert_eq!(runner.scene().objects().count(), 0);
    assert_eq!(runner.particles().unwrap().systems().count(), 0);
}
