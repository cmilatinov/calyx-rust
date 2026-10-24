//! The particle system component, built from modules.
//!
//! [`ComponentParticleSystem`] holds one instance of each module and runs
//! them in a fixed order:
//!
//! 1. update: [`advance_age`](data::advance_age), then each update module
//!    ([`ParticleForces`]), then dead particles are removed;
//! 2. emission: [`ParticleEmission`] says how many particles to spawn;
//! 3. init, per new particle: [`ParticleSpawnShape`], [`ParticleVelocity`],
//!    [`ParticleForces`], lifetime ([`ParticleScalarRange`]),
//!    [`ParticleSizeOverLifetime`];
//! 4. render: [`ParticleSizeOverLifetime`] and [`ParticleColorOverLifetime`]
//!    evaluate each particle into a [`ParticleRenderInstance`], drawn as
//!    [`ParticleRendering`] says.
//!
//! Particles live in a flat array of [`Particle`]s, and the update step only
//! reads and writes that array, so it can move to a GPU compute pass.

mod data;
mod emission;
mod modules;
#[cfg(test)]
mod tests;

pub(crate) use data::ParticleRenderInstance;
pub use emission::ParticleEmission;
pub use modules::{
    ParticleBlendMode, ParticleColorOverLifetime, ParticleForces, ParticleRendering,
    ParticleScalarRange, ParticleSizeOverLifetime, ParticleSpawnShape, ParticleVelocity,
};

use super::{
    Component, ComponentEventContext, ComponentReset, ComponentUpdate, ReflectComponent,
    ReflectComponentReset, ReflectComponentUpdate,
};
use crate as engine;
use crate::input::Input;
use crate::math::Transform;
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::Gizmos;
use crate::resource::ResourceMap;
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use data::{Particle, ParticleRng, SimulationSpace, DEFAULT_RNG_SEED};
use emission::EmissionState;
use nalgebra_glm::{Vec3, Vec4};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use uuid::Uuid;

/// Configurable particle emitter component.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "5214cd04-62ac-48e0-8f0b-4030d2102931"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate, ComponentReset)]
#[reflect_attr(name = "Particle System")]
#[serde(default)]
#[repr(C)]
pub struct ComponentParticleSystem {
    /// Whether the emitter is currently spawning particles. Live particles
    /// keep simulating either way.
    pub active: bool,
    /// Whether particles simulate in emitter-local space.
    pub local_space: bool,
    /// When and how many particles spawn.
    pub emission: ParticleEmission,
    /// Spawn volume for new particles.
    pub shape: ParticleSpawnShape,
    /// Lifetime range sampled per particle.
    pub lifetime: ParticleScalarRange,
    /// Spawn velocity.
    pub velocity: ParticleVelocity,
    /// Forces applied over the particle's life.
    pub forces: ParticleForces,
    /// Size over normalized particle lifetime.
    pub size: ParticleSizeOverLifetime,
    /// Color over normalized particle lifetime.
    pub color: ParticleColorOverLifetime,
    /// Blend mode and texture of the billboards.
    pub rendering: ParticleRendering,
    #[serde(skip)]
    #[reflect_skip]
    runtime: ParticleRuntime,
}

/// Simulation state of one system, rebuilt on reset and never serialized.
#[derive(Default)]
struct ParticleRuntime {
    particles: Vec<Particle>,
    emission: EmissionState,
    rng: ParticleRng,
}

impl Default for ComponentParticleSystem {
    fn default() -> Self {
        Self {
            active: true,
            local_space: false,
            emission: Default::default(),
            shape: Default::default(),
            lifetime: ParticleScalarRange { min: 0.4, max: 1.1 },
            velocity: Default::default(),
            forces: Default::default(),
            size: Default::default(),
            color: Default::default(),
            rendering: Default::default(),
            runtime: Default::default(),
        }
    }
}

impl Component for ComponentParticleSystem {
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        let transform = scene.world_transform(game_object);
        gizmos.set_color(&Vec4::new(1.0, 0.6, 0.2, 1.0));
        match self.shape {
            ParticleSpawnShape::Sphere { radius } => {
                gizmos.wire_sphere(&transform.position, radius.max(0.0));
            }
            ParticleSpawnShape::Box { extents } => {
                gizmos.wire_cube(&transform.position, &(extents * 2.0));
            }
        }
    }
}

impl ComponentReset for ComponentParticleSystem {
    fn reset(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
    ) {
        let seed = scene.uuid(game_object);
        scene.write_component::<ComponentParticleSystem, _>(game_object, |system| {
            system.reset_runtime(seed);
        });
    }
}

impl ComponentUpdate for ComponentParticleSystem {
    fn update(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let emitter_transform = scene.world_transform(game_object);
        let delta_time = resources.time().delta_time();
        scene.write_component::<ComponentParticleSystem, _>(game_object, |system| {
            system.step(delta_time, &emitter_transform);
        });
    }
}

impl ComponentParticleSystem {
    /// Number of live particles.
    pub fn particle_count(&self) -> usize {
        self.runtime.particles.len()
    }

    /// Advances the system by `delta_time`: updates live particles, then
    /// spawns what emission asks for. A zero `delta_time` only emits a
    /// pending burst.
    pub(crate) fn step(&mut self, delta_time: f32, emitter_transform: &Transform) {
        if delta_time > 0.0 {
            self.update_particles(delta_time);
        }
        if !self.active {
            return;
        }
        let requested = self.emission.emit(&mut self.runtime.emission, delta_time);
        self.spawn_particles(requested, emitter_transform);
    }

    /// The update stage. Reads and writes only the particle array.
    fn update_particles(&mut self, delta_time: f32) {
        let particles = &mut self.runtime.particles;
        data::advance_age(particles, delta_time);
        ParticleForces::update(particles, delta_time);
        data::remove_dead(particles);
    }

    /// The init stage, for up to `count` new particles under the
    /// `max_particles` cap.
    fn spawn_particles(&mut self, count: u32, emitter_transform: &Transform) {
        let live = self.runtime.particles.len() as u32;
        let available = self.emission.max_particles.saturating_sub(live);
        let space = self.space(emitter_transform);
        let rng = &mut self.runtime.rng;
        for _ in 0..count.min(available) {
            let mut particle = Particle::default();
            self.shape.init(&mut particle, rng, space);
            self.velocity.init(&mut particle, rng, space);
            self.forces.init(&mut particle, space);
            self.lifetime.init_lifetime(&mut particle, rng);
            self.size.init(&mut particle, rng);
            self.runtime.particles.push(particle);
        }
    }

    /// The render stage: one instance per visible particle, in world space,
    /// sorted back to front from `camera_position`.
    pub(crate) fn fill_render_buffer(
        &self,
        emitter_transform: &Transform,
        camera_position: &Vec3,
        out: &mut Vec<ParticleRenderInstance>,
    ) {
        out.clear();
        let space = self.space(emitter_transform);
        for particle in &self.runtime.particles {
            let t = particle.normalized_age();
            let size = self.size.evaluate(particle, t);
            if size <= 0.0 {
                continue;
            }
            let world_position = space.to_world(Vec3::from(particle.position));
            let offset = world_position - *camera_position;
            out.push(ParticleRenderInstance {
                position_size: [world_position.x, world_position.y, world_position.z, size],
                color: self.color.evaluate(t),
                distance_sq: offset.dot(&offset),
            });
        }

        out.sort_by(|left, right| {
            right
                .distance_sq
                .partial_cmp(&left.distance_sq)
                .unwrap_or(Ordering::Equal)
        });
    }

    fn space<'a>(&self, emitter_transform: &'a Transform) -> SimulationSpace<'a> {
        SimulationSpace {
            emitter: emitter_transform,
            local: self.local_space,
        }
    }

    pub(crate) fn reset_runtime(&mut self, seed: Uuid) {
        self.runtime.particles.clear();
        self.runtime.emission = EmissionState::default();
        self.runtime.rng = ParticleRng::seeded(seed.as_u128() as u64 ^ DEFAULT_RNG_SEED);
    }
}
