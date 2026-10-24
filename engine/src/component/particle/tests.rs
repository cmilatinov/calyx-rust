use super::*;
use crate::scene::Scene;
use crate::test_support::test_registries;
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::vec3;

/// A system that spawns still, constant-size particles living one second.
fn still_system() -> ComponentParticleSystem {
    ComponentParticleSystem {
        lifetime: ParticleScalarRange { min: 1.0, max: 1.0 },
        size: ParticleSizeOverLifetime {
            start: 1.0,
            end: 1.0,
            randomness: 0.0,
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

fn add_and_step(scene: &mut Scene, system: ComponentParticleSystem, delta_time: f32) -> GameObject {
    let go = scene.create(None, None);
    scene.add_component(go, system);
    let emitter_transform = scene.world_transform(go);
    let seed = scene.uuid(go);
    scene.write_component::<ComponentParticleSystem, _>(go, |system| {
        system.reset_runtime(seed);
        system.step(delta_time, &emitter_transform);
    });
    go
}

#[test]
fn particle_system_emits_burst_and_rate_particles() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let mut system = still_system();
    system.emission = ParticleEmission {
        rate: 4.0,
        burst_count: 2,
        max_particles: 16,
        ..Default::default()
    };
    let go = add_and_step(&mut scene, system, 0.5);

    let particle_count = scene
        .read_component::<ComponentParticleSystem, _, _>(go, |system| {
            system.runtime.particles.len()
        })
        .unwrap_or_default();
    assert_eq!(particle_count, 4);
}

#[test]
fn spawning_stops_at_max_particles() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let mut system = still_system();
    system.emission = ParticleEmission {
        rate: 100.0,
        burst_count: 3,
        max_particles: 5,
        ..Default::default()
    };
    let go = add_and_step(&mut scene, system, 0.5);

    let particle_count = scene
        .read_component::<ComponentParticleSystem, _, _>(go, |system| {
            system.runtime.particles.len()
        })
        .unwrap_or_default();
    assert_eq!(particle_count, 5);
}

#[test]
fn an_inactive_system_keeps_simulating_but_stops_spawning() {
    let mut system = still_system();
    system.emission.rate = 10.0;
    system.reset_runtime(Uuid::nil());
    let transform = Transform::default();
    system.step(0.5, &transform);
    assert_eq!(system.runtime.particles.len(), 5);

    system.active = false;
    system.step(0.6, &transform);
    assert_eq!(system.runtime.particles.len(), 5);
    system.step(0.6, &transform);
    assert!(system.runtime.particles.is_empty());
}

#[test]
fn local_space_particles_follow_emitter_transform() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let mut system = still_system();
    system.local_space = true;
    system.emission = ParticleEmission {
        rate: 0.0,
        burst_count: 1,
        ..Default::default()
    };
    system.shape = ParticleSpawnShape::Sphere { radius: 0.0 };
    let go = add_and_step(&mut scene, system, 0.0);
    scene.set_world_transform(go, Transform::from_xyz(5.0, 0.0, 0.0).matrix());

    let expected_x = scene.world_transform(go).position.x;
    let moved_position = scene
        .read_component::<ComponentParticleSystem, _, _>(go, |system| {
            let mut render_buffer = Vec::new();
            let transform = scene.world_transform(go);
            system.fill_render_buffer(&transform, &Vec3::zeros(), &mut render_buffer);
            render_buffer[0].position_size
        })
        .unwrap();

    assert!((moved_position[0] - expected_x).abs() < 1e-5);
}

#[test]
fn particle_size_changes_over_time() {
    let mut system = still_system();
    system.local_space = true;
    system.emission = ParticleEmission {
        rate: 0.0,
        burst_count: 1,
        ..Default::default()
    };
    system.size = ParticleSizeOverLifetime {
        start: 1.0,
        end: 0.25,
        randomness: 0.0,
    };
    let transform = Transform::default();
    system.reset_runtime(Uuid::nil());
    system.step(0.0, &transform);

    let mut render_buffer = Vec::new();
    system.fill_render_buffer(&transform, &Vec3::zeros(), &mut render_buffer);
    let initial_size = render_buffer[0].position_size[3];
    system.step(0.5, &transform);
    system.fill_render_buffer(&transform, &Vec3::zeros(), &mut render_buffer);
    let later_size = render_buffer[0].position_size[3];

    assert!(later_size < initial_size);
    assert!((later_size - 0.625).abs() < 1e-5);
}

#[test]
fn render_instances_are_sorted_back_to_front() {
    let mut system = still_system();
    system.emission = ParticleEmission {
        rate: 0.0,
        burst_count: 8,
        ..Default::default()
    };
    system.shape = ParticleSpawnShape::Box {
        extents: Vec3::from_element(4.0),
    };
    let transform = Transform::default();
    system.reset_runtime(Uuid::nil());
    system.step(0.0, &transform);

    let mut render_buffer = Vec::new();
    system.fill_render_buffer(&transform, &vec3(0.0, 0.0, 10.0), &mut render_buffer);
    assert_eq!(render_buffer.len(), 8);
    assert!(render_buffer
        .windows(2)
        .all(|pair| pair[0].distance_sq >= pair[1].distance_sq));
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

/// Hashes every frame of a scenario the way the pre-module recording did:
/// each particle's position, velocity, acceleration, age, lifetime and size
/// offset, then each render instance, as f32 bits.
fn record(
    mut system: ComponentParticleSystem,
    transform: &Transform,
    steps: &[f32],
) -> serde_json::Value {
    system.reset_runtime(Uuid::from_u128(0x1234_5678_9abc_def0_0fed_cba9_8765_4321));
    let camera = vec3(3.0, 2.0, 10.0);
    let mut frames = Vec::new();
    let mut buffer = Vec::new();
    for delta_time in steps {
        system.step(*delta_time, transform);
        system.fill_render_buffer(transform, &camera, &mut buffer);
        let particle_words = system.runtime.particles.iter().flat_map(|particle| {
            particle
                .position
                .into_iter()
                .chain(particle.velocity)
                .chain(particle.acceleration)
                .chain([particle.age, particle.lifetime, particle.size_offset])
        });
        let instance_words = buffer.iter().flat_map(|instance| {
            instance
                .position_size
                .into_iter()
                .chain(instance.color)
                .chain([instance.distance_sq])
        });
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for word in particle_words.chain(instance_words) {
            for byte in word.to_bits().to_le_bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x100_0000_01b3);
            }
        }
        frames.push(serde_json::json!({
            "particles": system.runtime.particles.len(),
            "instances": buffer.len(),
            "hash": format!("{hash:016x}"),
        }));
    }
    serde_json::Value::Array(frames)
}

/// The module split reproduces, bit for bit, the particles and render
/// instances the single-struct system produced before it, recorded in
/// `pre_modules_golden.json` from that code.
#[test]
fn modules_reproduce_the_pre_module_simulation() {
    let transform = Transform::from_components(
        vec3(1.5, -2.0, 0.25),
        UnitQuaternion::from_euler_angles(0.3, 1.1, -0.4),
        vec3(1.0, 2.0, 0.5),
    );
    let mut steps = vec![0.0f32];
    for i in 0..90 {
        steps.push([1.0 / 60.0, 1.0 / 30.0, 0.05, 0.011][i % 4]);
    }
    let world = ComponentParticleSystem {
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
        size: ParticleSizeOverLifetime {
            start: 0.7,
            end: 0.1,
            randomness: 0.2,
        },
        color: ParticleColorOverLifetime {
            start: Color32::from_rgba_unmultiplied(255, 120, 10, 255),
            end: Color32::from_rgba_unmultiplied(90, 20, 200, 40),
        },
        ..Default::default()
    };
    let local = ComponentParticleSystem {
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
        size: ParticleSizeOverLifetime {
            start: 0.2,
            end: 0.9,
            randomness: 0.05,
        },
        ..Default::default()
    };
    let recorded = serde_json::json!({
        "world_looping": record(world, &transform, &steps),
        "local_one_shot": record(local, &transform, &steps),
        "defaults": record(ComponentParticleSystem::default(), &transform, &steps),
    });

    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/particles/pre_modules_golden.json"
    ))
    .unwrap();
    for scenario in ["world_looping", "local_one_shot", "defaults"] {
        let expected = golden[scenario].as_array().unwrap();
        let actual = recorded[scenario].as_array().unwrap();
        assert_eq!(actual.len(), expected.len(), "{scenario}: frame count");
        for (frame, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(actual, expected, "{scenario}: frame {frame} differs");
        }
    }
}
