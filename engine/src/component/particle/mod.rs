//! The particle system component, built from modules.
//!
//! [`ComponentParticleSystem`] holds only authoring data: one instance of each
//! module. [`ParticleSimulation`](crate::render::ParticleSimulation) runs
//! them each frame, in a fixed order:
//!
//! 1. update, on the GPU: `advance_age`, then each update module
//!    ([`ParticleForces`]); a slot whose particle outlives its lifetime is
//!    free;
//! 2. emission, on the CPU: [`ParticleEmission`] says how many particles to
//!    spawn;
//! 3. init, on the CPU, per new particle: [`ParticleSpawnShape`],
//!    [`ParticleVelocity`], [`ParticleForces`], lifetime
//!    ([`ParticleScalarRange`]), [`ParticleSizeOverLifetime`]; the GPU then
//!    scatters the new particles into their slots;
//! 4. render, on the GPU, per view: [`ParticleSizeOverLifetime`] and
//!    [`ParticleColorOverLifetime`] evaluate each particle into a
//!    [`ParticleRenderInstance`], drawn as [`ParticleRendering`] says.
//!
//! The kernels live in `assets/shaders/particles_simulate.wgsl` and work on
//! flat buffers of [`Particle`]s. The CPU side of each system, its
//! [`EmitterState`], never reads them back.

mod data;
mod emission;
mod emitter;
mod modules;
#[cfg(test)]
mod tests;

pub(crate) use data::{Particle, ParticleRenderInstance};
pub use emission::ParticleEmission;
#[cfg(test)]
pub(crate) use emitter::LedgerSlot;
pub(crate) use emitter::{EmitterState, ParticleSpawn};
pub use modules::{
    ParticleBlendMode, ParticleColorOverLifetime, ParticleForces, ParticleRendering,
    ParticleScalarRange, ParticleSizeOverLifetime, ParticleSpawnShape, ParticleVelocity,
};

use super::{
    Component, ComponentEventContext, ComponentReset, ReflectComponent, ReflectComponentReset,
};
use crate as engine;
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::Gizmos;
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::Vec4;
use serde::{Deserialize, Serialize};

/// Configurable particle emitter component.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "5214cd04-62ac-48e0-8f0b-4030d2102931"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentReset)]
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
    /// Counts the resets asked for. The simulation re-seeds the system when
    /// it changes. Never serialized.
    #[serde(skip)]
    #[reflect_skip]
    reset_count: u32,
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
            reset_count: 0,
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
        scene.write_component::<ComponentParticleSystem, _>(game_object, |system| {
            system.restart();
        });
    }
}

impl ComponentParticleSystem {
    /// Asks the simulation to drop this system's particles and start it over
    /// from its seed.
    pub fn restart(&mut self) {
        self.reset_count = self.reset_count.wrapping_add(1);
    }

    /// Changes each time [`Self::restart`] is called.
    pub(crate) fn reset_count(&self) -> u32 {
        self.reset_count
    }
}
