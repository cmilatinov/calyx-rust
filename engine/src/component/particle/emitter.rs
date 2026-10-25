//! The CPU side of one simulated particle system.
//!
//! The particles themselves live on the GPU. The CPU keeps what it needs to
//! decide what spawns, and where, without reading them back: the emission
//! state, the random generator the init modules draw from, and a life ledger
//! holding each slot's age and lifetime. The ledger advances ages with the
//! same `f32` arithmetic as the update kernel, so it knows which slots are
//! alive on the GPU, the live count, and the lowest free slot.

use super::data::{Particle, ParticleRng, SimulationSpace, DEFAULT_RNG_SEED};
use super::ComponentParticleSystem;
use crate::math::Transform;
use uuid::Uuid;

use super::emission::EmissionState;

/// One particle to write into a slot of the GPU particle buffer, laid out as
/// the `Spawn` struct of `particles_simulate.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ParticleSpawn {
    /// The particle as the init modules built it.
    pub particle: Particle,
    /// Slot the particle goes into.
    pub slot: u32,
    /// Index of the first of the frame's steps the particle lives through:
    /// the steps after the one it spawned in.
    pub first_step: u32,
    _padding: [u32; 2],
}

/// What the ledger knows about one slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LedgerSlot {
    /// Seconds since the slot's particle spawned.
    pub age: f32,
    /// Seconds the particle lives.
    pub lifetime: f32,
    /// How many particles the system spawned before this one, so live slots
    /// order by spawn.
    pub spawn_index: u64,
}

impl LedgerSlot {
    const FREE: Self = Self {
        age: Particle::DEAD.age,
        lifetime: Particle::DEAD.lifetime,
        spawn_index: 0,
    };

    /// Whether the slot's particle is alive, by the same test as the shader.
    pub fn is_alive(&self) -> bool {
        Particle {
            age: self.age,
            lifetime: self.lifetime,
            ..Default::default()
        }
        .is_alive()
    }
}

/// Emission, random generator and life ledger of one system, with the work
/// the current frame queued for the GPU.
pub(crate) struct EmitterState {
    emission: EmissionState,
    rng: ParticleRng,
    slots: Vec<LedgerSlot>,
    live: u32,
    /// No slot below this one is free.
    lowest_free: usize,
    spawned: u64,
    /// The positive step lengths queued this frame, in order.
    steps: Vec<f32>,
    /// The particles spawned this frame, at most one per slot.
    spawns: Vec<ParticleSpawn>,
    /// Index in `spawns` of each slot's spawn this frame.
    pending: Vec<Option<u32>>,
}

impl EmitterState {
    /// A fresh system with `capacity` free slots, its random generator seeded
    /// from its object's id.
    pub fn new(seed: Uuid, capacity: u32) -> Self {
        let mut state = Self {
            emission: EmissionState::default(),
            rng: ParticleRng::seeded(seed.as_u128() as u64 ^ DEFAULT_RNG_SEED),
            slots: Vec::new(),
            live: 0,
            lowest_free: 0,
            spawned: 0,
            steps: Vec::new(),
            spawns: Vec::new(),
            pending: Vec::new(),
        };
        state.set_capacity(capacity);
        state
    }

    /// Number of slots.
    pub fn capacity(&self) -> u32 {
        self.slots.len() as u32
    }

    /// Resizes the ledger to `capacity` slots. Particles in slots past the
    /// new capacity are gone.
    pub fn set_capacity(&mut self, capacity: u32) {
        let capacity = capacity as usize;
        if capacity < self.slots.len() {
            let dropped = self.slots[capacity..]
                .iter()
                .filter(|slot| slot.is_alive())
                .count() as u32;
            self.live -= dropped;
        }
        self.slots.resize(capacity, LedgerSlot::FREE);
        self.pending.resize(capacity, None);
        self.lowest_free = self.lowest_free.min(capacity);
    }

    /// Number of live particles.
    pub fn live_count(&self) -> u32 {
        self.live
    }

    /// The ledger, one entry per slot.
    #[cfg(test)]
    pub fn slots(&self) -> &[LedgerSlot] {
        &self.slots
    }

    /// The step lengths queued since [`Self::finish_frame`].
    pub fn steps(&self) -> &[f32] {
        &self.steps
    }

    /// The particles spawned since [`Self::finish_frame`].
    pub fn spawns(&self) -> &[ParticleSpawn] {
        &self.spawns
    }

    /// Forgets the frame's queued steps and spawns once the GPU work for
    /// them is recorded.
    pub fn finish_frame(&mut self) {
        self.steps.clear();
        for spawn in self.spawns.drain(..) {
            self.pending[spawn.slot as usize] = None;
        }
    }

    /// Advances the system by `delta_time`: ages the live slots, then spawns
    /// what emission asks for into the lowest free slots, under the
    /// `max_particles` cap. A zero `delta_time` only emits a pending burst.
    pub fn step(
        &mut self,
        system: &ComponentParticleSystem,
        delta_time: f32,
        emitter_transform: &Transform,
    ) {
        if delta_time > 0.0 {
            self.advance(delta_time);
        }
        if !system.active {
            return;
        }
        let requested = system.emission.emit(&mut self.emission, delta_time);
        let cap = system.emission.max_particles.min(self.capacity());
        let count = requested.min(cap.saturating_sub(self.live));
        let space = SimulationSpace {
            emitter: emitter_transform,
            local: system.local_space,
        };
        for _ in 0..count {
            let mut particle = Particle::default();
            system.shape.init(&mut particle, &mut self.rng, space);
            system.velocity.init(&mut particle, &mut self.rng, space);
            system.forces.init(&mut particle, space);
            system.lifetime.init_lifetime(&mut particle, &mut self.rng);
            system.size.init(&mut particle, &mut self.rng);
            self.spawn(particle);
        }
    }

    /// The ledger half of the update kernel: `advance_age` for every live
    /// slot, freeing the ones that die.
    fn advance(&mut self, delta_time: f32) {
        self.steps.push(delta_time);
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if !slot.is_alive() {
                continue;
            }
            slot.age += delta_time;
            if !slot.is_alive() {
                self.live -= 1;
                self.lowest_free = self.lowest_free.min(index);
            }
        }
    }

    fn spawn(&mut self, particle: Particle) {
        let slot = (self.lowest_free..self.slots.len())
            .find(|&index| !self.slots[index].is_alive())
            .expect("a system under its cap has a free slot");
        self.lowest_free = slot + 1;
        self.live += 1;
        self.slots[slot] = LedgerSlot {
            age: particle.age,
            lifetime: particle.lifetime,
            spawn_index: self.spawned,
        };
        self.spawned += 1;
        let spawn = ParticleSpawn {
            particle,
            slot: slot as u32,
            first_step: self.steps.len() as u32,
            _padding: [0; 2],
        };
        // A slot freed and filled again within the frame keeps only its
        // latest particle.
        match self.pending[slot] {
            Some(index) => self.spawns[index as usize] = spawn,
            None => {
                self.pending[slot] = Some(self.spawns.len() as u32);
                self.spawns.push(spawn);
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
}
