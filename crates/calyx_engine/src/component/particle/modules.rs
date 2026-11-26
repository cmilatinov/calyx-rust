//! The modules a particle system is built from, apart from emission.
//!
//! Each module contributes to one or more stages of a particle's life:
//!
//! - **init**: writes its part of a newly spawned [`Particle`], drawing random
//!   values from the system's [`ParticleRng`]. Runs on the CPU.
//! - **update**: a pure step of one particle over `delta_time`, run by the
//!   `update_main` kernel of `particles_simulate.wgsl`.
//! - **evaluate**: a pure function of one particle and its normalized age,
//!   run by the `evaluate_main` kernel when building render instances. The
//!   `evaluate` functions here are the CPU reference the tests check that
//!   kernel against.

use super::data::{Particle, ParticleRng, SimulationSpace, MIN_PARTICLE_LIFETIME};
use crate as calyx_engine;
use crate::assets::texture::Texture;
use crate::assets::AssetRef;
use crate::math::{Curve, Gradient};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::TypeUuid;
use egui::Color32;
use nalgebra_glm::{vec3, Vec3};
use serde::{Deserialize, Serialize};

/// Spawn shape module: the volume new particles appear in, centered on the
/// emitter. Init: position.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "37c707d9-f6e8-4dc1-b2b1-159ceff99082"]
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
/// Update, on the GPU: semi-implicit Euler, velocity first, then position
/// with the new velocity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "6ef001ba-5f23-46e0-83b6-2da991b03ece"]
#[reflect(Default)]
#[serde(default)]
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
}

/// Size over lifetime module. Init: a random size offset. Evaluate: the size
/// at a normalized age.
///
/// The size follows `curve`, in world units, over the particle's normalized
/// age. Each particle's random offset shifts every key of the curve, and
/// each shifted key is clamped at zero before the curve interpolates between
/// them, so a straight line from `start` to `end` sizes particles exactly as
/// the former start/end module did.
///
/// A module serialized with `start` and `end` instead of a curve, as before
/// curves, loads as the straight line between them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "5af8ac8a-9652-477b-9487-68a077b3a6b3"]
#[reflect(Default)]
#[serde(from = "ParticleSizeData")]
pub struct ParticleSizeOverLifetime {
    /// Size over normalized particle lifetime.
    pub curve: Curve,
    /// Random offset applied to every key of the curve.
    pub randomness: f32,
}

impl Default for ParticleSizeOverLifetime {
    fn default() -> Self {
        Self::linear(0.6, 0.0, 0.3)
    }
}

/// A [`ParticleSizeOverLifetime`] as it is serialized, curve or start/end.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ParticleSizeData {
    curve: Option<Curve>,
    start: Option<f32>,
    end: Option<f32>,
    randomness: Option<f32>,
}

impl From<ParticleSizeData> for ParticleSizeOverLifetime {
    fn from(data: ParticleSizeData) -> Self {
        let default = Self::default();
        let curve = match (data.curve, data.start, data.end) {
            (Some(curve), _, _) => curve,
            (None, None, None) => default.curve,
            (None, start, end) => Curve::linear(
                start.unwrap_or_else(|| default.curve.evaluate(0.0)),
                end.unwrap_or_else(|| default.curve.evaluate(1.0)),
            ),
        };
        Self {
            curve,
            randomness: data.randomness.unwrap_or(default.randomness),
        }
    }
}

impl ParticleSizeOverLifetime {
    /// A straight line from `start` to `end` over the lifetime, with
    /// `randomness`.
    pub fn linear(start: f32, end: f32, randomness: f32) -> Self {
        Self {
            curve: Curve::linear(start, end),
            randomness,
        }
    }

    pub(crate) fn init(&self, particle: &mut Particle, rng: &mut ParticleRng) {
        particle.size_offset = rng.signed() * self.randomness.max(0.0);
    }

    /// The particle's size at normalized age `t`. Never negative.
    #[cfg(test)]
    pub(crate) fn evaluate(&self, particle: &Particle, t: f32) -> f32 {
        self.curve
            .segment(t)
            .map(|size| (size + particle.size_offset).max(0.0))
            .value()
    }
}

/// Color over lifetime module. Evaluate: the unpremultiplied color at a
/// normalized age.
///
/// The color follows `gradient` over the particle's normalized age, blended
/// in unmultiplied sRGB from `0.0` to `1.0`, as the renderer reads it.
///
/// A module serialized with `start` and `end` colors instead of a gradient,
/// as before gradients, loads as a two-key gradient between them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "dda37e30-d5de-4c77-a4b2-b273797bc2d5"]
#[reflect(Default)]
#[serde(from = "ParticleColorData")]
pub struct ParticleColorOverLifetime {
    /// Color over normalized particle lifetime.
    pub gradient: Gradient,
}

/// Start and end colors of the default gradient.
fn default_colors() -> (Color32, Color32) {
    (
        Color32::from_rgba_unmultiplied(255, 4, 0, 255),
        Color32::from_rgba_unmultiplied(255, 169, 0, 84),
    )
}

impl Default for ParticleColorOverLifetime {
    fn default() -> Self {
        let (start, end) = default_colors();
        Self::linear(start, end)
    }
}

/// A [`ParticleColorOverLifetime`] as it is serialized, gradient or
/// start/end.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ParticleColorData {
    gradient: Option<Gradient>,
    start: Option<Color32>,
    end: Option<Color32>,
}

impl From<ParticleColorData> for ParticleColorOverLifetime {
    fn from(data: ParticleColorData) -> Self {
        if let Some(gradient) = data.gradient {
            return Self { gradient };
        }
        let (start, end) = default_colors();
        Self::linear(data.start.unwrap_or(start), data.end.unwrap_or(end))
    }
}

impl ParticleColorOverLifetime {
    /// A two-key gradient from `start` to `end` over the lifetime, keyed by
    /// their unmultiplied sRGB bytes, as the start/end module rendered them.
    pub fn linear(start: Color32, end: Color32) -> Self {
        let unmultiplied = |color: Color32| {
            color
                .to_srgba_unmultiplied()
                .map(|byte| byte as f32 / 255.0)
        };
        Self {
            gradient: Gradient::linear(unmultiplied(start), unmultiplied(end)),
        }
    }

    /// Unmultiplied sRGB RGBA in `0..=1`.
    #[cfg(test)]
    pub(crate) fn evaluate(&self, t: f32) -> [f32; 4] {
        self.gradient.evaluate(t).into()
    }
}

/// Supported particle blending modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "444a8eaa-3d18-486b-96bb-7d038b1979bc"]
pub enum ParticleBlendMode {
    /// Standard alpha blending.
    Alpha,
    /// Additive blending for emissive effects.
    #[default]
    Additive,
}

/// Rendering module: how the particle renderer draws the system's billboards.
#[derive(Clone, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "dc67d8fd-a232-4830-ba3d-c02830d5e964"]
#[reflect(Default)]
#[serde(default)]
pub struct ParticleRendering {
    /// Blend mode used by the renderer.
    pub blend_mode: ParticleBlendMode,
    /// Optional texture asset for billboard rendering. White when unset.
    pub texture: AssetRef<Texture>,
    /// Multiplies RGB radiance, including values above one for bloom. Alpha is unchanged.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub intensity: f32,
    /// Smallest billboard width in physical pixels. Zero disables the screen-size floor.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub min_screen_size: f32,
    /// Orientation and length of the camera-facing quad. The colour and depth passes
    /// use the same shape, including the texture and elliptical silhouette.
    pub billboard: ParticleBillboard,
}

impl Default for ParticleRendering {
    fn default() -> Self {
        Self {
            blend_mode: Default::default(),
            texture: Default::default(),
            intensity: 1.0,
            min_screen_size: 6.0,
            billboard: Default::default(),
        }
    }
}

/// How a particle's camera-facing billboard is oriented in the image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "7f998df4-bf0f-4c5e-9b07-3652377cf280"]
pub enum ParticleBillboard {
    /// The existing circular billboard, with the camera's horizontal and vertical axes.
    #[default]
    CameraFacing,
    /// The long axis follows the particle's projected world velocity. A stationary
    /// particle uses the emitter's forward (+Z) axis instead.
    VelocityAligned {
        /// Length divided by width, at least one. Size curves control the width.
        length_scale: f32,
    },
    /// The long axis follows an emitter-local direction, including for still particles.
    Directional {
        /// Direction in emitter-local space. A zero vector falls back to emitter +Z.
        direction: Vec3,
        /// Length divided by width, at least one. Size curves control the width.
        length_scale: f32,
    },
}

#[cfg(test)]
mod tests;
