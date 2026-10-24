//! The modules a particle system is built from, apart from emission.
//!
//! Each module contributes to one or more stages of a particle's life:
//!
//! - **init**: writes its part of a newly spawned [`Particle`], drawing random
//!   values from the system's [`ParticleRng`]. Runs on the CPU.
//! - **update**: a pure step over the particle slice and `delta_time`, with no
//!   random values, transforms or allocation, so it can move to a compute
//!   shader unchanged.
//! - **evaluate**: a pure function of one particle and its normalized age,
//!   used when building render instances.

use super::data::{Particle, ParticleRng, SimulationSpace, MIN_PARTICLE_LIFETIME};
use crate as engine;
use crate::assets::texture::Texture;
use crate::assets::AssetRef;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::TypeUuid;
use egui::Color32;
use nalgebra_glm::{vec3, Vec3};
use serde::{Deserialize, Serialize};

/// Spawn shape module: the volume new particles appear in, centered on the
/// emitter. Init: position.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "37c707d9-f6e8-4dc1-b2b1-159ceff99082"]
#[repr(C)]
pub enum ParticleSpawnShape {
    /// Uniform sphere centered on the emitter.
    Sphere {
        /// Sphere radius.
        radius: f32,
    },
    /// Axis-aligned box centered on the emitter.
    Box {
        /// Positive half extents on each axis.
        extents: Vec3,
    },
}

impl Default for ParticleSpawnShape {
    fn default() -> Self {
        Self::Sphere { radius: 1.0 }
    }
}

impl ParticleSpawnShape {
    pub(crate) fn init(
        &self,
        particle: &mut Particle,
        rng: &mut ParticleRng,
        space: SimulationSpace,
    ) {
        let local_position = match *self {
            Self::Sphere { radius } => rng.in_unit_sphere() * radius,
            Self::Box { extents } => rng.signed_vec3(extents),
        };
        particle.position = space.point(local_position).into();
    }
}

/// Inclusive scalar range sampled for per-particle values.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "6c5546bb-496e-4b68-8c1f-1d288b7665f5"]
#[serde(default)]
#[repr(C)]
pub struct ParticleScalarRange {
    /// Minimum sampled value.
    pub min: f32,
    /// Maximum sampled value.
    pub max: f32,
}

impl Default for ParticleScalarRange {
    fn default() -> Self {
        Self { min: 1.0, max: 1.0 }
    }
}

impl ParticleScalarRange {
    /// A uniform value in the range. An empty range draws no random value.
    pub(crate) fn sample(&self, rng: &mut ParticleRng) -> f32 {
        if (self.max - self.min).abs() <= f32::EPSILON {
            self.min
        } else {
            self.min + rng.scalar() * (self.max - self.min)
        }
    }

    /// Lifetime module. Init: lifetime, sampled from this range.
    pub(crate) fn init_lifetime(&self, particle: &mut Particle, rng: &mut ParticleRng) {
        particle.lifetime = self.sample(rng).max(MIN_PARTICLE_LIFETIME);
    }
}

/// Velocity module: emitter-local spawn velocity. Init: velocity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "a25e1804-ec6f-4904-8aeb-b87e421d200f"]
#[reflect(Default)]
#[serde(default)]
#[repr(C)]
pub struct ParticleVelocity {
    /// Base local-space spawn velocity.
    pub initial: Vec3,
    /// Random velocity variation per axis.
    pub randomness: Vec3,
}

impl Default for ParticleVelocity {
    fn default() -> Self {
        Self {
            initial: vec3(0.0, 5.0, 0.0),
            randomness: vec3(1.0, 1.5, 1.0),
        }
    }
}

impl ParticleVelocity {
    pub(crate) fn init(
        &self,
        particle: &mut Particle,
        rng: &mut ParticleRng,
        space: SimulationSpace,
    ) {
        let local_velocity = self.initial + rng.signed_vec3(self.randomness);
        particle.velocity = space.direction(local_velocity).into();
    }
}

/// Forces module: constant emitter-local acceleration. Init: acceleration.
/// Update: integrates velocity and position.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "6ef001ba-5f23-46e0-83b6-2da991b03ece"]
#[reflect(Default)]
#[serde(default)]
#[repr(C)]
pub struct ParticleForces {
    /// Constant acceleration applied to particles.
    pub acceleration: Vec3,
}

impl Default for ParticleForces {
    fn default() -> Self {
        Self {
            acceleration: vec3(0.0, 5.0, 0.0),
        }
    }
}

impl ParticleForces {
    pub(crate) fn init(&self, particle: &mut Particle, space: SimulationSpace) {
        particle.acceleration = space.direction(self.acceleration).into();
    }

    /// Semi-implicit Euler: velocity first, then position with the new
    /// velocity.
    pub(crate) fn update(particles: &mut [Particle], delta_time: f32) {
        for particle in particles {
            for axis in 0..3 {
                particle.velocity[axis] += particle.acceleration[axis] * delta_time;
                particle.position[axis] += particle.velocity[axis] * delta_time;
            }
        }
    }
}

/// Size over lifetime module. Init: a random size offset. Evaluate: the size
/// at a normalized age.
///
/// `evaluate` is the module's only reader of `start` and `end`, so a curve can
/// replace them without touching the rest of the system.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "5af8ac8a-9652-477b-9487-68a077b3a6b3"]
#[reflect(Default)]
#[serde(default)]
#[repr(C)]
pub struct ParticleSizeOverLifetime {
    /// Size at the start of particle life.
    pub start: f32,
    /// Size at the end of particle life.
    pub end: f32,
    /// Random offset applied to both endpoints.
    pub randomness: f32,
}

impl Default for ParticleSizeOverLifetime {
    fn default() -> Self {
        Self {
            start: 0.6,
            end: 0.0,
            randomness: 0.3,
        }
    }
}

impl ParticleSizeOverLifetime {
    pub(crate) fn init(&self, particle: &mut Particle, rng: &mut ParticleRng) {
        particle.size_offset = rng.signed() * self.randomness.max(0.0);
    }

    /// The particle's size at normalized age `t`. Never negative.
    pub(crate) fn evaluate(&self, particle: &Particle, t: f32) -> f32 {
        let start = (self.start + particle.size_offset).max(0.0);
        let end = (self.end + particle.size_offset).max(0.0);
        start + (end - start) * t
    }
}

/// Color over lifetime module. Evaluate: the unpremultiplied color at a
/// normalized age.
///
/// `evaluate` is the module's only reader of `start` and `end`, so a gradient
/// can replace them without touching the rest of the system.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "dda37e30-d5de-4c77-a4b2-b273797bc2d5"]
#[reflect(Default)]
#[serde(default)]
#[repr(C)]
pub struct ParticleColorOverLifetime {
    /// Color at birth.
    pub start: Color32,
    /// Color at death.
    pub end: Color32,
}

impl Default for ParticleColorOverLifetime {
    fn default() -> Self {
        Self {
            start: Color32::from_rgba_unmultiplied(255, 4, 0, 255),
            end: Color32::from_rgba_unmultiplied(255, 169, 0, 84),
        }
    }
}

impl ParticleColorOverLifetime {
    /// Linear RGBA in `0..=1`, interpolated in unmultiplied sRGB bytes.
    pub(crate) fn evaluate(&self, t: f32) -> [f32; 4] {
        let start = self.start.to_srgba_unmultiplied();
        let end = self.end.to_srgba_unmultiplied();
        std::array::from_fn(|channel| {
            (start[channel] as f32 + (end[channel] as f32 - start[channel] as f32) * t) / 255.0
        })
    }
}

/// Supported particle blending modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "444a8eaa-3d18-486b-96bb-7d038b1979bc"]
#[repr(C)]
pub enum ParticleBlendMode {
    /// Standard alpha blending.
    Alpha,
    /// Additive blending for emissive effects.
    #[default]
    Additive,
}

/// Rendering module: how the particle renderer draws the system's billboards.
#[derive(Clone, Default, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "dc67d8fd-a232-4830-ba3d-c02830d5e964"]
#[reflect(Default)]
#[serde(default)]
#[repr(C)]
pub struct ParticleRendering {
    /// Blend mode used by the renderer.
    pub blend_mode: ParticleBlendMode,
    /// Optional texture asset for billboard rendering. White when unset.
    pub texture: AssetRef<Texture>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Transform;

    fn particle() -> Particle {
        Particle {
            lifetime: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn forces_integrate_velocity_before_position() {
        let mut particles = [Particle {
            velocity: [1.0, 0.0, 0.0],
            acceleration: [0.0, 2.0, 0.0],
            ..particle()
        }];
        ParticleForces::update(&mut particles, 0.5);
        assert_eq!(particles[0].velocity, [1.0, 1.0, 0.0]);
        assert_eq!(particles[0].position, [0.5, 0.5, 0.0]);
    }

    #[test]
    fn world_space_init_applies_the_emitter_transform() {
        let emitter = Transform::from_xyz(2.0, 0.0, 0.0);
        let space = SimulationSpace {
            emitter: &emitter,
            local: false,
        };
        let mut rng = ParticleRng::default();
        let mut particle = particle();
        ParticleSpawnShape::Sphere { radius: 0.0 }.init(&mut particle, &mut rng, space);
        assert_eq!(particle.position, [2.0, 0.0, 0.0]);

        let local = SimulationSpace {
            local: true,
            ..space
        };
        ParticleSpawnShape::Sphere { radius: 0.0 }.init(&mut particle, &mut rng, local);
        assert_eq!(particle.position, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn box_shape_stays_inside_its_extents() {
        let emitter = Transform::default();
        let space = SimulationSpace {
            emitter: &emitter,
            local: true,
        };
        let extents = vec3(0.5, 1.0, 2.0);
        let mut rng = ParticleRng::seeded(7);
        let mut particle = particle();
        for _ in 0..64 {
            ParticleSpawnShape::Box { extents }.init(&mut particle, &mut rng, space);
            for axis in 0..3 {
                assert!(particle.position[axis].abs() <= extents[axis]);
            }
        }
    }

    #[test]
    fn velocity_randomness_varies_per_particle() {
        let emitter = Transform::default();
        let space = SimulationSpace {
            emitter: &emitter,
            local: true,
        };
        let velocity = ParticleVelocity {
            initial: Vec3::zeros(),
            randomness: Vec3::from_element(1.0),
        };
        let mut rng = ParticleRng::default();
        let mut first = particle();
        let mut second = particle();
        velocity.init(&mut first, &mut rng, space);
        velocity.init(&mut second, &mut rng, space);
        assert_ne!(first.velocity, second.velocity);
    }

    #[test]
    fn an_empty_lifetime_range_draws_no_random_value() {
        let mut rng = ParticleRng::default();
        let mut particle = particle();
        ParticleScalarRange { min: 0.5, max: 0.5 }.init_lifetime(&mut particle, &mut rng);
        assert_eq!(particle.lifetime, 0.5);
        let mut untouched = ParticleRng::default();
        assert_eq!(rng.scalar(), untouched.scalar());

        ParticleScalarRange { min: 0.0, max: 0.0 }.init_lifetime(&mut particle, &mut rng);
        assert_eq!(particle.lifetime, MIN_PARTICLE_LIFETIME);
    }

    #[test]
    fn size_interpolates_and_clamps_at_zero() {
        let size = ParticleSizeOverLifetime {
            start: 1.0,
            end: 0.25,
            randomness: 0.0,
        };
        let mut particle = particle();
        assert_eq!(size.evaluate(&particle, 0.5), 0.625);
        particle.size_offset = -0.5;
        assert_eq!(size.evaluate(&particle, 1.0), 0.0);
    }

    #[test]
    fn color_lerp_uses_unmultiplied_alpha() {
        let color = ParticleColorOverLifetime {
            start: Color32::from_rgba_unmultiplied(255, 169, 0, 84),
            end: Color32::from_rgba_unmultiplied(255, 169, 0, 84),
        }
        .evaluate(0.5);

        assert!(color[0] > 0.9, "expected red to stay unpremultiplied");
        assert!(color[1] > 0.55, "expected green to stay unpremultiplied");
        assert!(color[2] < 0.05, "expected blue to stay near zero");
        assert!((color[3] - (84.0 / 255.0)).abs() < 0.02);
    }
}
