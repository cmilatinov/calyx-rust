use super::*;
use crate::component::{
    ParticleEmission, ParticleForces, ParticleScalarRange, ParticleSizeOverLifetime,
    ParticleVelocity,
};
use nalgebra_glm::Vec3;

/// A system that spawns still particles living `lifetime` seconds.
fn system(emission: ParticleEmission, lifetime: f32) -> ComponentParticleSystem {
    ComponentParticleSystem {
        emission,
        lifetime: ParticleScalarRange {
            min: lifetime,
            max: lifetime,
        },
        size: ParticleSizeOverLifetime::linear(1.0, 1.0, 0.0),
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

fn emitter(system: &ComponentParticleSystem) -> EmitterState {
    EmitterState::new(Uuid::nil(), system.emission.max_particles)
}

#[test]
fn a_step_spawns_the_burst_and_the_rate() {
    let system = system(
        ParticleEmission {
            rate: 4.0,
            burst_count: 2,
            max_particles: 16,
            ..Default::default()
        },
        1.0,
    );
    let mut state = emitter(&system);
    state.step(&system, 0.5, &Transform::default());
    assert_eq!(state.live_count(), 4);
    assert_eq!(state.spawns().len(), 4);
    assert_eq!(state.steps(), [0.5]);
    assert!(state.spawns().iter().all(|spawn| spawn.first_step == 1));
}

#[test]
fn spawning_stops_at_max_particles() {
    let system = system(
        ParticleEmission {
            rate: 100.0,
            burst_count: 3,
            max_particles: 5,
            ..Default::default()
        },
        1.0,
    );
    let mut state = emitter(&system);
    state.step(&system, 0.5, &Transform::default());
    assert_eq!(state.live_count(), 5);
}

#[test]
fn an_inactive_system_keeps_aging_but_stops_spawning() {
    let mut system = system(
        ParticleEmission {
            rate: 10.0,
            ..Default::default()
        },
        1.0,
    );
    let mut state = emitter(&system);
    let transform = Transform::default();
    state.step(&system, 0.5, &transform);
    assert_eq!(state.live_count(), 5);

    system.active = false;
    state.step(&system, 0.6, &transform);
    assert_eq!(state.live_count(), 5);
    state.step(&system, 0.6, &transform);
    assert_eq!(state.live_count(), 0);
}

#[test]
fn a_spawn_takes_the_lowest_free_slot() {
    let system = system(
        ParticleEmission {
            rate: 0.0,
            burst_count: 3,
            max_particles: 4,
            duration: 1.0,
            looping: true,
        },
        0.5,
    );
    let mut state = emitter(&system);
    let transform = Transform::default();
    state.step(&system, 0.0, &transform);
    let slots: Vec<u32> = state.spawns().iter().map(|spawn| spawn.slot).collect();
    assert_eq!(slots, [0, 1, 2]);
    state.finish_frame();

    // All three die at 0.5 s; the loop restarts at 1 s and the burst
    // fills slots 0 to 2 again, in the same frame's second step.
    state.step(&system, 0.6, &transform);
    assert_eq!(state.live_count(), 0);
    state.step(&system, 0.5, &transform);
    let spawns: Vec<(u32, u32)> = state
        .spawns()
        .iter()
        .map(|spawn| (spawn.slot, spawn.first_step))
        .collect();
    assert_eq!(spawns, [(0, 2), (1, 2), (2, 2)]);
    assert_eq!(state.steps(), [0.6, 0.5]);
}

#[test]
fn a_slot_refilled_within_a_frame_keeps_only_its_latest_particle() {
    let system = system(
        ParticleEmission {
            rate: 0.0,
            burst_count: 1,
            max_particles: 1,
            duration: 0.2,
            looping: true,
        },
        0.1,
    );
    let mut state = emitter(&system);
    let transform = Transform::default();
    state.step(&system, 0.0, &transform);
    state.step(&system, 0.25, &transform);
    assert_eq!(state.spawns().len(), 1);
    assert_eq!(state.spawns()[0].first_step, 1);
    assert_eq!(state.slots()[0].spawn_index, 1);
}

#[test]
fn shrinking_the_capacity_drops_the_particles_past_it() {
    let system = system(
        ParticleEmission {
            rate: 0.0,
            burst_count: 4,
            max_particles: 4,
            ..Default::default()
        },
        1.0,
    );
    let mut state = emitter(&system);
    state.step(&system, 0.0, &Transform::default());
    state.finish_frame();
    state.set_capacity(2);
    assert_eq!(state.live_count(), 2);
    state.set_capacity(8);
    assert_eq!(state.capacity(), 8);
    assert_eq!(state.live_count(), 2);
}
