//! Per-particle data and the steps every particle goes through regardless of
//! which modules a system uses.
//!
//! A [`Particle`] is plain `#[repr(C)]` data laid out in 16-byte rows, so the
//! particle array can be uploaded as a storage buffer and the update step
//! ([`advance_age`] followed by the update modules) can run in a compute
//! shader over the same layout.

use crate::math::Transform;
use nalgebra_glm::{vec3, Vec3};

/// Shortest lifetime a particle can have, so normalized age stays finite.
pub(crate) const MIN_PARTICLE_LIFETIME: f32 = 0.01;
/// Seed of a system that has not been reset yet.
pub(crate) const DEFAULT_RNG_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

/// One live particle, in the simulation space of its system: world space, or
/// emitter-local space for a local-space system.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Particle {
    pub position: [f32; 3],
    /// Seconds since the particle spawned.
    pub age: f32,
    pub velocity: [f32; 3],
    /// Seconds the particle lives, at least [`MIN_PARTICLE_LIFETIME`].
    pub lifetime: f32,
    /// Constant acceleration, already in simulation space.
    pub acceleration: [f32; 3],
    /// Random offset the size module added to this particle's size.
    pub size_offset: f32,
}

impl Particle {
    /// Age as a fraction of the lifetime, in `0..=1`.
    pub fn normalized_age(&self) -> f32 {
        let lifetime = self.lifetime.max(MIN_PARTICLE_LIFETIME);
        (self.age / lifetime).clamp(0.0, 1.0)
    }

    pub fn is_alive(&self) -> bool {
        self.age < self.lifetime.max(MIN_PARTICLE_LIFETIME)
    }
}

/// Ages every particle by `delta_time`. The first step of every update.
pub(crate) fn advance_age(particles: &mut [Particle], delta_time: f32) {
    for particle in particles {
        particle.age += delta_time;
    }
}

/// Drops the particles that outlived their lifetime, keeping the order of the
/// others. The last step of every update.
pub(crate) fn remove_dead(particles: &mut Vec<Particle>) {
    particles.retain(Particle::is_alive);
}

/// Instance data the particle renderer draws for one particle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ParticleRenderInstance {
    pub position_size: [f32; 4],
    pub color: [f32; 4],
    pub distance_sq: f32,
}

/// Where a system's particles live and how emitter-local spawn values map
/// into it.
#[derive(Clone, Copy)]
pub(crate) struct SimulationSpace<'a> {
    pub emitter: &'a Transform,
    pub local: bool,
}

impl SimulationSpace<'_> {
    /// Maps an emitter-local point into simulation space.
    pub fn point(self, local_point: Vec3) -> Vec3 {
        if self.local {
            local_point
        } else {
            self.emitter.transform_position(&local_point)
        }
    }

    /// Maps an emitter-local direction into simulation space.
    pub fn direction(self, local_direction: Vec3) -> Vec3 {
        if self.local {
            local_direction
        } else {
            self.emitter.transform_direction(&local_direction)
        }
    }

    /// Maps a simulation-space point into world space.
    pub fn to_world(self, point: Vec3) -> Vec3 {
        if self.local {
            self.emitter.transform_position(&point)
        } else {
            point
        }
    }
}

/// Xorshift generator the spawn modules draw their random values from, seeded
/// per system so a system replays the same particles after a reset.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParticleRng(u64);

impl Default for ParticleRng {
    fn default() -> Self {
        Self(DEFAULT_RNG_SEED)
    }
}

impl ParticleRng {
    pub fn seeded(seed: u64) -> Self {
        Self(seed)
    }

    /// Uniform value in `0..=1`.
    pub fn scalar(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 32) as u32) as f32 / (u32::MAX as f32)
    }

    /// Uniform value in `-1..=1`.
    pub fn signed(&mut self) -> f32 {
        self.scalar() * 2.0 - 1.0
    }

    /// Each axis uniform in `-extents..=extents`.
    pub fn signed_vec3(&mut self, extents: Vec3) -> Vec3 {
        vec3(
            self.signed() * extents.x,
            self.signed() * extents.y,
            self.signed() * extents.z,
        )
    }

    /// A point in the unit sphere, or the origin when 16 draws all miss it.
    pub fn in_unit_sphere(&mut self) -> Vec3 {
        for _ in 0..16 {
            let point = vec3(self.signed(), self.signed(), self.signed());
            if point.dot(&point) <= 1.0 {
                return point;
            }
        }
        Vec3::zeros()
    }
}
