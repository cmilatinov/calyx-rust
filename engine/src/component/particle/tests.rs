use super::data::{ParticleRng, SimulationSpace};
use super::*;
use crate::component::ComponentID;
use crate::context::{GameContext, ReadOnlyAssetContext, UpdatedScene};
use crate::core::Ref;
use crate::input::Input;
use crate::math::{Curve, CurveInterpolation, CurveKey, Transform};
use crate::render::{
    Camera, ParticleSimulation, RenderContext, SceneRenderer, SceneRendererOptions,
};
use crate::test_support::{HeadlessSceneRunner, TestAssets};
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};
use std::collections::HashSet;
use std::ops::Range;
use uuid::Uuid;

/// Id of the object the golden scenarios were recorded with, which seeds
/// their random generator.
const GOLDEN_SEED: Uuid = Uuid::from_u128(0x1234_5678_9abc_def0_0fed_cba9_8765_4321);

/// Spawns an object with id `id` at `transform` holding `system`.
fn spawn_system(
    scene: &mut Scene,
    id: Uuid,
    transform: &Transform,
    system: ComponentParticleSystem,
) -> GameObject {
    let game_object = scene.create(
        Some(ComponentID {
            id,
            name: "System".into(),
            visible: true,
        }),
        None,
    );
    scene.add_component(game_object, system);
    scene.set_transform(game_object, &transform.matrix());
    game_object
}

/// A system that bursts `burst_count` still particles living ten seconds.
fn still_burst(burst_count: u32) -> ComponentParticleSystem {
    ComponentParticleSystem {
        emission: ParticleEmission {
            rate: 0.0,
            burst_count,
            max_particles: burst_count.max(1),
            ..Default::default()
        },
        lifetime: ParticleScalarRange {
            min: 10.0,
            max: 10.0,
        },
        velocity: ParticleVelocity {
            initial: Vec3::zeros(),
            randomness: Vec3::zeros(),
        },
        forces: ParticleForces {
            acceleration: Vec3::zeros(),
        },
        ..Default::default()
    }
}

/// A camera at `position` looking down the negative z axis.
fn camera_at(position: Vec3) -> (Camera, Transform) {
    let camera = Camera::new(1.0, 60f32.to_radians(), 0.1, 200.0);
    let transform = Transform::from_components(
        position,
        UnitQuaternion::face_towards(&vec3(0.0, 0.0, -1.0), &vec3(0.0, 1.0, 0.0)),
        vec3(1.0, 1.0, 1.0),
    );
    (camera, transform)
}

fn renderer(context: &ReadOnlyAssetContext) -> SceneRenderer {
    SceneRenderer::new(
        context,
        SceneRendererOptions {
            samples: 1,
            ..Default::default()
        },
        (32, 32),
    )
    .expect("scene renderer should load its assets")
}

#[test]
fn serialized_shape_groups_fields_by_module() {
    let system = ComponentParticleSystem::default();
    let value = serde_json::to_value(&system).unwrap();
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "active",
            "color",
            "emission",
            "forces",
            "lifetime",
            "local_space",
            "rendering",
            "shape",
            "size",
            "velocity",
        ]
    );
    assert_eq!(value["emission"]["rate"], 256.0);
    assert_eq!(value["rendering"]["blend_mode"], "Additive");
}

#[test]
fn a_partial_module_keeps_the_defaults_of_its_missing_fields() {
    let system: ComponentParticleSystem = serde_json::from_value(serde_json::json!({
        "emission": { "rate": 12.0 },
        "velocity": { "initial": [0.0, 1.0, 0.0] },
    }))
    .unwrap();
    let defaults = ComponentParticleSystem::default();
    assert_eq!(
        system.emission,
        ParticleEmission {
            rate: 12.0,
            ..defaults.emission
        }
    );
    assert_eq!(system.velocity.initial, vec3(0.0, 1.0, 0.0));
    assert_eq!(system.velocity.randomness, defaults.velocity.randomness);
    assert_eq!(system.lifetime, defaults.lifetime);
    assert_eq!(system.color, defaults.color);
}

fn golden_world_looping() -> ComponentParticleSystem {
    ComponentParticleSystem {
        emission: ParticleEmission {
            rate: 37.0,
            burst_count: 5,
            max_particles: 64,
            duration: 0.7,
            looping: true,
        },
        lifetime: ParticleScalarRange { min: 0.3, max: 0.9 },
        shape: ParticleSpawnShape::Sphere { radius: 0.8 },
        velocity: ParticleVelocity {
            initial: vec3(0.2, 3.0, -0.5),
            randomness: vec3(1.0, 0.5, 0.75),
        },
        forces: ParticleForces {
            acceleration: vec3(0.0, -4.0, 1.0),
        },
        size: ParticleSizeOverLifetime::linear(0.7, 0.1, 0.2),
        color: ParticleColorOverLifetime::linear(
            Color32::from_rgba_unmultiplied(255, 120, 10, 255),
            Color32::from_rgba_unmultiplied(90, 20, 200, 40),
        ),
        ..Default::default()
    }
}

fn golden_local_one_shot() -> ComponentParticleSystem {
    ComponentParticleSystem {
        local_space: true,
        emission: ParticleEmission {
            looping: false,
            rate: 50.0,
            burst_count: 3,
            max_particles: 40,
            duration: 0.5,
        },
        lifetime: ParticleScalarRange { min: 0.6, max: 0.6 },
        shape: ParticleSpawnShape::Box {
            extents: vec3(0.5, 0.25, 1.0),
        },
        velocity: ParticleVelocity {
            initial: vec3(1.0, 0.0, 0.0),
            randomness: vec3(0.3, 0.3, 0.0),
        },
        forces: ParticleForces {
            acceleration: vec3(0.0, 2.0, 0.0),
        },
        size: ParticleSizeOverLifetime::linear(0.2, 0.9, 0.05),
        ..Default::default()
    }
}

/// Builds one test scenario's system.
type SystemFactory = fn() -> ComponentParticleSystem;

/// Checks the GPU slots of a golden frame against the particles the
/// pre-module CPU simulation had alive then, in spawn order: ages and
/// lifetimes bit for bit, positions and velocities within 1e-4.
fn assert_golden_frame(
    label: &str,
    particles: &[Particle],
    slots: &[LedgerSlot],
    expected: &[[f32; 8]],
) {
    for (slot, (particle, ledger)) in particles.iter().zip(slots).enumerate() {
        assert_eq!(
            particle.is_alive(),
            ledger.is_alive(),
            "{label}: slot {slot} is alive on one side only"
        );
        if ledger.is_alive() {
            assert_eq!(
                particle.age.to_bits(),
                ledger.age.to_bits(),
                "{label}: slot {slot} ledger age"
            );
        }
    }
    let mut live: Vec<usize> = (0..slots.len())
        .filter(|&slot| slots[slot].is_alive())
        .collect();
    live.sort_by_key(|&slot| slots[slot].spawn_index);
    assert_eq!(live.len(), expected.len(), "{label}: live particles");
    for (index, (&slot, row)) in live.iter().zip(expected).enumerate() {
        let particle = &particles[slot];
        let label = format!("{label}: particle {index} (slot {slot})");
        assert_eq!(particle.age.to_bits(), row[0].to_bits(), "{label}: age");
        assert_eq!(
            particle.lifetime.to_bits(),
            row[1].to_bits(),
            "{label}: lifetime"
        );
        let actual = particle.position.into_iter().chain(particle.velocity);
        for (axis, (actual, expected)) in actual.zip(&row[2..]).enumerate() {
            assert!(
                (actual - expected).abs() <= 1e-4,
                "{label}: component {axis} of position and velocity is {actual}, expected {expected}"
            );
        }
    }
}

/// The GPU simulation reproduces the particles the CPU simulation produced
/// before the module split, recorded in `pre_modules_values.json` from that
/// code: per frame the live count, and every 15th frame each live particle.
/// It runs once with one step per frame and once with each sampled frame's
/// steps queued into a single frame.
#[test]
fn modules_reproduce_the_pre_module_simulation() {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let render_context = context.render_context.clone();
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/particles/pre_modules_values.json"
    ))
    .unwrap();
    let transform = Transform::from_components(
        vec3(1.5, -2.0, 0.25),
        UnitQuaternion::from_euler_angles(0.3, 1.1, -0.4),
        vec3(1.0, 2.0, 0.5),
    );
    let mut steps = vec![0.0f32];
    for i in 0..90 {
        steps.push([1.0 / 60.0, 1.0 / 30.0, 0.05, 0.011][i % 4]);
    }
    let scenarios: [(&str, SystemFactory); 3] = [
        ("world_looping", golden_world_looping),
        ("local_one_shot", golden_local_one_shot),
        ("defaults", ComponentParticleSystem::default),
    ];
    for (scenario, system) in scenarios {
        let expected = &golden[scenario];
        let counts: Vec<u32> = serde_json::from_value(expected["counts"].clone()).unwrap();
        assert_eq!(counts.len(), steps.len());
        for queued in [false, true] {
            let frames: Vec<Range<usize>> = if queued {
                std::iter::once(0..1)
                    .chain((1..steps.len()).step_by(15).map(|start| start..start + 15))
                    .collect()
            } else {
                (0..steps.len()).map(|step| step..step + 1).collect()
            };
            let mut scene = context.scene();
            let game_object = spawn_system(&mut scene, GOLDEN_SEED, &transform, system());
            let mut simulation = ParticleSimulation::new();
            for frame in frames {
                simulation.simulate_steps(&scene, &render_context, &steps[frame.clone()]);
                let last = frame.end - 1;
                let label = format!("{scenario} (queued: {queued}), step {last}");
                assert_eq!(
                    simulation.live_count(game_object),
                    Some(counts[last]),
                    "{label}: live count"
                );
                let Some(rows) = expected["frames"].get(last.to_string()) else {
                    continue;
                };
                let rows: Vec<[f32; 8]> = serde_json::from_value(rows.clone()).unwrap();
                let particles = simulation.read_particles(&render_context, game_object);
                let slots = simulation
                    .system(game_object)
                    .expect("the system is simulated")
                    .emitter()
                    .slots();
                assert_golden_frame(&label, &particles, slots, &rows);
            }
        }
    }
}

/// CPU reference of one simulation step for one system: the update kernel
/// over the slots, then the ledger's step, then the scatter of its spawns.
fn reference_step(
    emitter: &mut EmitterState,
    particles: &mut [Particle],
    system: &ComponentParticleSystem,
    delta_time: f32,
    transform: &Transform,
) {
    if delta_time > 0.0 {
        for particle in particles.iter_mut().filter(|particle| particle.is_alive()) {
            particle.age += delta_time;
            for axis in 0..3 {
                particle.velocity[axis] += particle.acceleration[axis] * delta_time;
                particle.position[axis] += particle.velocity[axis] * delta_time;
            }
        }
    }
    emitter.step(system, delta_time, transform);
    for spawn in emitter.spawns() {
        particles[spawn.slot as usize] = spawn.particle;
    }
    emitter.finish_frame();
}

/// CPU reference of the evaluate, sort and gather kernels: the live slots'
/// instances, farthest from `camera` first, ties by slot.
fn reference_instances(
    particles: &[Particle],
    system: &ComponentParticleSystem,
    transform: &Transform,
    camera: Vec3,
) -> Vec<ParticleRenderInstance> {
    let space = SimulationSpace {
        emitter: transform,
        local: system.local_space,
    };
    let mut instances: Vec<(f32, usize, ParticleRenderInstance)> = particles
        .iter()
        .enumerate()
        .filter(|(_, particle)| particle.is_alive())
        .map(|(slot, particle)| {
            let t = particle.normalized_age();
            let position = space.to_world(Vec3::from(particle.position));
            let offset = position - camera;
            let instance = ParticleRenderInstance {
                position_size: [
                    position.x,
                    position.y,
                    position.z,
                    system.size.evaluate(particle, t),
                ],
                color: system.color.evaluate(t),
            };
            (offset.dot(&offset), slot, instance)
        })
        .collect();
    instances.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    instances
        .into_iter()
        .map(|(_, _, instance)| instance)
        .collect()
}

/// Matches each GPU instance to a reference instance near the same sorted
/// position, so particles at nearly equal distances may swap, and checks
/// that the GPU order is back to front.
fn assert_instances_match(
    label: &str,
    actual: &[ParticleRenderInstance],
    expected: &[ParticleRenderInstance],
    camera: Vec3,
) {
    assert_eq!(actual.len(), expected.len(), "{label}: instance count");
    let close = |a: &ParticleRenderInstance, b: &ParticleRenderInstance| {
        (0..4).all(|i| (a.position_size[i] - b.position_size[i]).abs() <= 1e-3)
            && (0..4).all(|i| (a.color[i] - b.color[i]).abs() <= 1e-4)
    };
    let mut matched = vec![false; expected.len()];
    for (index, instance) in actual.iter().enumerate() {
        let window = index.saturating_sub(8)..(index + 9).min(expected.len());
        let found = window
            .clone()
            .find(|&candidate| !matched[candidate] && close(instance, &expected[candidate]));
        let Some(candidate) = found else {
            panic!(
                "{label}: GPU instance {index} {instance:?} matches no reference instance near it: {:?}",
                &expected[window]
            );
        };
        matched[candidate] = true;
    }
    let distance = |instance: &ParticleRenderInstance| {
        let position = Vec3::from_row_slice(&instance.position_size[..3]);
        (position - camera).norm_squared()
    };
    for (index, pair) in actual.windows(2).enumerate() {
        assert!(
            distance(&pair[0]) + 1e-3 >= distance(&pair[1]),
            "{label}: instances {index} and {} are not back to front",
            index + 1
        );
    }
}

/// Over 120 seeded steps of varied length, some queued several to a frame,
/// with bursts, acceleration, a world-space and a local-space system whose
/// emitters move, and slots dying and refilling, each view's instance buffer
/// matches a CPU reference simulation and evaluation.
#[test]
fn gpu_instances_match_a_cpu_reference_over_120_seeded_steps() {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let render_context = context.render_context.clone();
    let world = || ComponentParticleSystem {
        emission: ParticleEmission {
            rate: 40.0,
            burst_count: 6,
            max_particles: 48,
            duration: 0.35,
            looping: true,
        },
        lifetime: ParticleScalarRange {
            min: 0.15,
            max: 0.45,
        },
        shape: ParticleSpawnShape::Sphere { radius: 0.6 },
        velocity: ParticleVelocity {
            initial: vec3(0.5, 4.0, -0.25),
            randomness: vec3(1.0, 0.75, 1.0),
        },
        forces: ParticleForces {
            acceleration: vec3(0.0, -6.0, 1.5),
        },
        size: ParticleSizeOverLifetime::linear(0.5, 0.05, 0.1),
        // Non-linear color: green eases down, blue steps up, alpha holds then
        // fades.
        color: ParticleColorOverLifetime {
            green: Curve::new(vec![
                CurveKey::new(0.0, 0.8).with_interpolation(CurveInterpolation::Smooth),
                CurveKey::new(0.6, 0.1),
                CurveKey::new(1.0, 0.0),
            ]),
            blue: Curve::new(vec![
                CurveKey::new(0.0, 0.1).with_interpolation(CurveInterpolation::Constant),
                CurveKey::new(0.5, 0.9),
            ]),
            alpha: Curve::new(vec![
                CurveKey::new(0.0, 1.0),
                CurveKey::new(0.3, 1.0).with_interpolation(CurveInterpolation::Smooth),
                CurveKey::new(1.0, 0.1),
            ]),
            ..ParticleColorOverLifetime::linear(
                Color32::from_rgba_unmultiplied(255, 200, 40, 255),
                Color32::from_rgba_unmultiplied(40, 10, 220, 30),
            )
        },
        ..Default::default()
    };
    let local = || ComponentParticleSystem {
        local_space: true,
        emission: ParticleEmission {
            rate: 25.0,
            burst_count: 4,
            max_particles: 24,
            duration: 0.5,
            looping: true,
        },
        lifetime: ParticleScalarRange { min: 0.3, max: 0.6 },
        shape: ParticleSpawnShape::Box {
            extents: vec3(0.4, 0.2, 0.8),
        },
        velocity: ParticleVelocity {
            initial: vec3(1.0, 0.5, 0.0),
            randomness: vec3(0.3, 0.3, 0.3),
        },
        forces: ParticleForces {
            acceleration: vec3(1.0, 2.0, 0.0),
        },
        // Non-linear size: swells smoothly, then shrinks to nothing, with
        // offsets that push the ends below zero.
        size: ParticleSizeOverLifetime {
            curve: Curve::new(vec![
                CurveKey::new(0.0, 0.05).with_interpolation(CurveInterpolation::Smooth),
                CurveKey::new(0.35, 0.8).with_interpolation(CurveInterpolation::Smooth),
                CurveKey::new(1.0, 0.0),
            ]),
            randomness: 0.1,
        },
        ..Default::default()
    };
    let emitter_transform = |frame: usize, offset: f32| {
        let angle = frame as f32 * 0.07 + offset;
        Transform::from_components(
            vec3(angle.cos() * 2.0, 0.5 + offset, angle.sin() * 2.0 - 1.0),
            UnitQuaternion::from_euler_angles(0.2 * angle, angle, -0.3),
            vec3(1.0, 1.5, 0.75),
        )
    };
    let mut scene = context.scene();
    let ids = [Uuid::from_u128(0x5eed_0001), Uuid::from_u128(0x5eed_0002)];
    let systems: [SystemFactory; 2] = [world, local];
    let game_objects: Vec<GameObject> = (0..2)
        .map(|index| {
            spawn_system(
                &mut scene,
                ids[index],
                &emitter_transform(0, index as f32),
                systems[index](),
            )
        })
        .collect();
    let mut references: Vec<(EmitterState, Vec<Particle>)> = (0..2)
        .map(|index| {
            let capacity = systems[index]().emission.max_particles;
            (
                EmitterState::new(ids[index], capacity),
                vec![Particle::DEAD; capacity as usize],
            )
        })
        .collect();
    let camera_position = vec3(0.5, 1.0, 9.0);
    let (camera, camera_transform) = camera_at(camera_position);
    let mut renderer = renderer(&context);
    let mut simulation = ParticleSimulation::new();
    let mut rng = ParticleRng::seeded(0x00c0_ffee);
    let mut step = 0;
    let mut frame = 0;
    let mut queued_frames = 0;
    while step < 120 {
        let count = (1 + frame % 3).min(120 - step);
        queued_frames += usize::from(count > 1);
        let steps: Vec<f32> = (0..count).map(|_| 0.004 + rng.scalar() * 0.05).collect();
        for (index, game_object) in game_objects.iter().enumerate() {
            scene.set_transform(
                *game_object,
                &emitter_transform(frame, index as f32).matrix(),
            );
        }
        simulation.simulate_steps(&scene, &render_context, &steps);
        renderer.render_scene_base(&camera, &camera_transform, &scene, Some(&simulation), None);

        for (index, game_object) in game_objects.iter().enumerate() {
            let label = format!("system {index}, frame {frame}");
            let system = systems[index]();
            let transform = scene.world_transform(*game_object);
            let (emitter, particles) = &mut references[index];
            for delta_time in &steps {
                reference_step(emitter, particles, &system, *delta_time, &transform);
            }
            let live = simulation.live_count(*game_object).unwrap_or_default();
            assert_eq!(live, emitter.live_count(), "{label}: live count");
            if live == 0 {
                continue;
            }
            let instances = renderer
                .particle_renderer()
                .read_instances(&render_context, *game_object);
            let expected = reference_instances(particles, &system, &transform, camera_position);
            assert_instances_match(
                &label,
                &instances[..live as usize],
                &expected,
                camera_position,
            );
        }
        step += count;
        frame += 1;
    }
    assert!(queued_frames > 0, "some frames should queue several steps");
    for (index, (emitter, _)) in references.iter().enumerate() {
        let spawned = emitter
            .slots()
            .iter()
            .map(|slot| slot.spawn_index)
            .max()
            .unwrap_or_default();
        assert!(
            spawned >= u64::from(emitter.capacity()),
            "system {index} should refill freed slots, spawned {spawned}"
        );
    }
}

/// A curve edited while the simulation does not step, as when it is paused,
/// reaches the particles the next time a view draws them.
#[test]
fn a_curve_edit_shows_on_the_next_draw_without_a_simulation_step() {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let render_context = context.render_context.clone();
    let mut scene = context.scene();
    let game_object = spawn_system(
        &mut scene,
        GOLDEN_SEED,
        &Transform::default(),
        still_burst(4),
    );
    let mut simulation = ParticleSimulation::new();
    simulation.simulate(&scene, &render_context, 0.016);
    let (camera, camera_transform) = camera_at(vec3(0.0, 0.0, 10.0));
    let mut renderer = renderer(&context);
    let red = |renderer: &mut SceneRenderer, scene: &Scene| {
        renderer.render_scene_base(&camera, &camera_transform, scene, Some(&simulation), None);
        renderer
            .particle_renderer()
            .read_instances(&render_context, game_object)
            .iter()
            .map(|instance| instance.color[0])
            .collect::<Vec<f32>>()
    };
    assert!(red(&mut renderer, &scene).iter().all(|&red| red > 0.99));

    scene.write_component::<ComponentParticleSystem, _>(game_object, |system| {
        system.color.red = Curve::constant(0.25);
    });
    assert_eq!(red(&mut renderer, &scene), vec![0.25; 4]);
}

/// A system's curve keys reach the GPU when it is first simulated and again
/// only on a frame its curves changed.
#[test]
fn curve_keys_upload_only_when_the_curves_change() {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let render_context = context.render_context.clone();
    let mut scene = context.scene();
    let game_object = spawn_system(
        &mut scene,
        GOLDEN_SEED,
        &Transform::default(),
        still_burst(4),
    );
    let mut simulation = ParticleSimulation::new();
    let uploads = |simulation: &ParticleSimulation| {
        simulation
            .system(game_object)
            .expect("the system is simulated")
            .curve_uploads()
    };

    for _ in 0..3 {
        simulation.simulate(&scene, &render_context, 0.016);
    }
    assert_eq!(uploads(&simulation), 1);

    scene.write_component::<ComponentParticleSystem, _>(game_object, |system| {
        system.size.randomness = 0.5;
    });
    simulation.simulate(&scene, &render_context, 0.016);
    assert_eq!(uploads(&simulation), 1, "randomness is not a curve");

    scene.write_component::<ComponentParticleSystem, _>(game_object, |system| {
        system.color.alpha.insert(CurveKey::new(0.5, 0.25));
    });
    simulation.simulate(&scene, &render_context, 0.016);
    simulation.simulate(&scene, &render_context, 0.016);
    assert_eq!(uploads(&simulation), 2);
    let ranges = simulation.system(game_object).unwrap().curve_ranges();
    assert_eq!(ranges[4].count, 3, "the alpha curve's new key is uploaded");
}

#[test]
fn a_system_seeds_from_its_object_id_whatever_built_the_scene() {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let render_context = context.render_context.clone();
    let mut scene = context.scene();
    let mut system = still_burst(4);
    system.shape = ParticleSpawnShape::Sphere { radius: 2.0 };
    let game_object = spawn_system(&mut scene, GOLDEN_SEED, &Transform::default(), system);
    let copy = scene.restore_snapshot(scene.snapshot());
    assert_ne!(copy.id(), scene.id());

    let positions = |scene: &Scene| {
        let mut simulation = ParticleSimulation::new();
        simulation.simulate(scene, &render_context, 0.0);
        let game_object = scene.objects().next().expect("the scene has one object");
        simulation
            .read_particles(&render_context, game_object)
            .into_iter()
            .map(|particle| particle.position)
            .collect::<Vec<_>>()
    };
    assert_eq!(positions(&scene), positions(&copy));

    // A restart re-seeds the running system the same way.
    let mut simulation = ParticleSimulation::new();
    simulation.simulate(&scene, &render_context, 0.0);
    let first = simulation.read_particles(&render_context, game_object);
    simulation.simulate(&scene, &render_context, 0.5);
    scene.write_component::<ComponentParticleSystem, _>(game_object, |system| {
        system.restart();
    });
    simulation.simulate(&scene, &render_context, 0.0);
    assert_eq!(
        simulation.read_particles(&render_context, game_object),
        first
    );
}

#[test]
fn a_destroyed_system_is_released_and_an_undrawn_one_is_kept() {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let render_context = context.render_context.clone();
    let mut scene = context.scene();
    let kept = spawn_system(
        &mut scene,
        Uuid::new_v4(),
        &Transform::default(),
        still_burst(2),
    );
    let removed = spawn_system(
        &mut scene,
        Uuid::new_v4(),
        &Transform::default(),
        still_burst(2),
    );
    let mut short = still_burst(1);
    short.lifetime = ParticleScalarRange { min: 0.1, max: 0.1 };
    let expired = spawn_system(&mut scene, Uuid::new_v4(), &Transform::default(), short);
    let (camera, transform) = camera_at(vec3(0.0, 0.0, 5.0));
    let mut renderer = renderer(&context);
    let mut simulation = ParticleSimulation::new();

    simulation.simulate(&scene, &render_context, 0.0);
    renderer.render_scene_base(&camera, &transform, &scene, Some(&simulation), None);
    let all = HashSet::from([kept, removed, expired]);
    assert_eq!(simulation.systems().collect::<HashSet<_>>(), all);
    assert_eq!(renderer.particle_renderer().tracked_systems(), all);

    scene.delete(removed);
    scene.flush_deletes();
    simulation.simulate(&scene, &render_context, 0.2);
    assert_eq!(simulation.live_count(removed), None);
    assert_eq!(simulation.live_count(expired), Some(0));
    renderer.render_scene_base(&camera, &transform, &scene, Some(&simulation), None);
    // `expired` draws nothing but is still simulated, so its view state stays.
    assert_eq!(
        renderer.particle_renderer().tracked_systems(),
        HashSet::from([kept, expired])
    );
}

/// A game context over the engine assets whose authoring scene holds one
/// particle system.
fn game_with_particles(assets: &TestAssets) -> GameContext {
    let mut game = GameContext::new(assets.asset_context());
    let scene = game.scenes.current_scene_mut();
    spawn_system(scene, Uuid::new_v4(), &Transform::default(), still_burst(3));
    game
}

fn update_simulation(game: &mut GameContext) {
    let context = egui::Context::default();
    let input = Input::from_ctx(&context, None, game.resources.input_mut(), false);
    game.update_scene(UpdatedScene::Simulation, &input);
}

#[test]
fn swapping_the_simulated_scene_releases_its_systems_and_rebinds() {
    let assets = TestAssets::engine();
    let mut game = game_with_particles(&assets);
    let context = game.assets.lock_read();
    let mut renderer = renderer(&context);
    let (camera, transform) = camera_at(vec3(0.0, 0.0, 5.0));
    let draw = |game: &GameContext, renderer: &mut SceneRenderer| {
        renderer.render_scene_base(
            &camera,
            &transform,
            game.scenes.simulation_scene(),
            Some(&game.particles),
            None,
        );
    };

    // Play start binds the play copy.
    game.scenes.start_simulation();
    update_simulation(&mut game);
    let first = game.scenes.simulation_scene().id();
    assert_eq!(game.particles.scene_id(), Some(first));
    assert_eq!(game.particles.frames(), 1);
    let first_systems: HashSet<GameObject> = game.particles.systems().collect();
    assert_eq!(first_systems.len(), 1);
    draw(&game, &mut renderer);
    assert_eq!(
        renderer.particle_renderer().tracked_systems(),
        first_systems
    );

    // A paused simulation does not step.
    game.scenes.pause_simulation();
    update_simulation(&mut game);
    assert_eq!(game.particles.frames(), 1);
    game.scenes.start_simulation();
    update_simulation(&mut game);
    assert_eq!(game.particles.frames(), 2);

    // Stop: the authoring scene draws no particles from the old copy's
    // simulation, which keeps its systems until it binds another scene.
    game.scenes.stop_simulation();
    update_simulation(&mut game);
    assert_eq!(game.particles.scene_id(), Some(first));
    draw(&game, &mut renderer);
    assert_eq!(
        renderer.particle_renderer().tracked_systems(),
        first_systems
    );

    // Play again: a new copy, so every system is released and rebuilt.
    game.scenes.start_simulation();
    update_simulation(&mut game);
    let second = game.scenes.simulation_scene().id();
    assert_ne!(second, first);
    assert_eq!(game.particles.scene_id(), Some(second));
    assert_eq!(game.particles.frames(), 1);
    let second_systems: HashSet<GameObject> = game.particles.systems().collect();
    assert_eq!(second_systems.len(), 1);
    assert!(first_systems.is_disjoint(&second_systems));
    draw(&game, &mut renderer);
    assert_eq!(
        renderer.particle_renderer().tracked_systems(),
        second_systems
    );

    // Loading a scene stops the simulation; the next play binds its copy.
    let loaded = Ref::new(game.scenes.current_scene().clone());
    game.scenes.load_scene(loaded.readonly());
    game.scenes.start_simulation();
    update_simulation(&mut game);
    let third = game.scenes.simulation_scene().id();
    assert!(third != first && third != second);
    assert_eq!(game.particles.scene_id(), Some(third));
    assert_eq!(game.particles.frames(), 1);
    let third_systems: HashSet<GameObject> = game.particles.systems().collect();
    assert_eq!(third_systems.len(), 1);
    draw(&game, &mut renderer);
    assert_eq!(
        renderer.particle_renderer().tracked_systems(),
        third_systems
    );
}

/// Reads `renderer`'s scene texture back from the GPU.
fn read_scene_texture(
    renderer: &SceneRenderer,
    render_context: &RenderContext,
) -> image::RgbaImage {
    let mut readback = renderer
        .submit_scene_texture_readback()
        .expect("scene texture has a size");
    loop {
        let _ = render_context
            .device()
            .poll(egui_wgpu::wgpu::Maintain::Wait);
        match readback.try_finish(render_context) {
            crate::render::ReadbackPoll::Pending => continue,
            crate::render::ReadbackPoll::Ready(image) => break image,
            crate::render::ReadbackPoll::Failed(error) => panic!("scene readback failed: {error}"),
        }
    }
}

#[test]
fn two_views_of_one_scene_share_one_simulation_step_per_frame() {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let render_context = context.render_context.clone();
    let mut scene = context.scene();
    let mut system = still_burst(1);
    system.shape = ParticleSpawnShape::Sphere { radius: 0.0 };
    system.size = ParticleSizeOverLifetime::linear(1.0, 1.0, 0.0);
    system.color = ParticleColorOverLifetime::linear(Color32::RED, Color32::RED);
    let game_object = spawn_system(&mut scene, Uuid::new_v4(), &Transform::default(), system);
    let mut runner = HeadlessSceneRunner::from_scene(scene).with_particles();
    let (camera, transform) = camera_at(vec3(0.0, 0.0, 5.0));
    let mut viewport = renderer(&context);
    let mut game_view = renderer(&context);

    const FRAMES: usize = 3;
    let mut expected_age = 0.0f32;
    for frame in 0..FRAMES {
        runner.step();
        // The burst spawns after the first frame's update, so it ages from
        // the second frame on.
        if frame > 0 {
            expected_age += runner.resources().time().delta_time();
        }
        let particles = runner.particles();
        for view in [&mut viewport, &mut game_view] {
            view.options_mut().clear_color = Color32::BLACK;
            view.render_scene_base(&camera, &transform, runner.scene(), particles, None);
            view.finalize_scene();
        }
    }

    let particles = runner.particles().expect("the runner simulates particles");
    assert_eq!(particles.frames(), FRAMES as u64);
    let slots = particles
        .system(game_object)
        .expect("the system is simulated")
        .emitter()
        .slots();
    assert_eq!(slots[0].age, expected_age);
    let gpu = particles.read_particles(&render_context, game_object);
    assert_eq!(gpu[0].age, expected_age);
    for view in [&viewport, &game_view] {
        let image = read_scene_texture(view, &render_context);
        let pixel = image.get_pixel(16, 16);
        assert!(
            pixel[0] > 200 && pixel[1] < 50,
            "each view should draw the particle, got {pixel:?}"
        );
    }
}

#[test]
fn a_view_draws_particles_only_for_the_simulated_scene() {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let render_context = context.render_context.clone();
    let mut scene = context.scene();
    let mut system = still_burst(1);
    system.shape = ParticleSpawnShape::Sphere { radius: 0.0 };
    system.color = ParticleColorOverLifetime::linear(Color32::RED, Color32::RED);
    spawn_system(&mut scene, Uuid::new_v4(), &Transform::default(), system);
    // Same objects and ids, but another scene, as a thumbnail or the edit
    // scene beside the play copy.
    let copy = scene.restore_snapshot(scene.snapshot());
    let mut simulation = ParticleSimulation::new();
    simulation.simulate(&scene, &render_context, 0.0);
    let (camera, transform) = camera_at(vec3(0.0, 0.0, 5.0));
    let mut view = renderer(&context);
    view.options_mut().clear_color = Color32::BLACK;

    let center = |view: &mut SceneRenderer, scene: &Scene, particles| {
        view.render_scene_base(&camera, &transform, scene, particles, None);
        view.finalize_scene();
        *read_scene_texture(view, &render_context).get_pixel(16, 16)
    };
    assert!(center(&mut view, &scene, Some(&simulation))[0] > 200);
    assert_eq!(center(&mut view, &copy, Some(&simulation))[0], 0);
    assert_eq!(center(&mut view, &scene, None)[0], 0);
}
