//! The distance fog effect component, read by the fog pass.

use super::{PostProcessEffect, ReflectPostProcessEffect};
use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use serde::{Deserialize, Serialize};

/// Distance fog: every surface fades towards the fog colour with its
/// distance from the camera, after lighting. The background, at the far
/// plane, takes the fog of that distance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "659a0e8b-963d-439b-9a73-dc8bb2c6ead1"]
#[reflect(Default, TypeUuidDynamic, Component, PostProcessEffect)]
#[reflect_attr(name = "Fog", category = "Post Processing")]
#[serde(default)]
pub struct ComponentFog {
    /// How the fog thickens with distance.
    pub mode: FogMode,
    /// The colour distant surfaces fade to, sRGB.
    pub color: Color32,
    /// Brightness of the fog colour, as linear HDR radiance: the colour is
    /// scaled by it before tone mapping, like a light's intensity.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub intensity: f32,
    /// Distance from the camera, in world units, where linear fog starts.
    #[reflect_attr(min = 0.0, speed = 0.5)]
    pub start: f32,
    /// Distance from the camera, in world units, where linear fog reaches
    /// its full opacity.
    #[reflect_attr(min = 0.0, speed = 0.5)]
    pub end: f32,
    /// Exponential fog's density, per world unit: a surface keeps
    /// `exp(-density * distance)` of its own colour.
    #[reflect_attr(min = 0.0, speed = 0.0005)]
    pub density: f32,
    /// The largest share of the fog colour any pixel takes, from 0 (no fog)
    /// to 1.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub opacity: f32,
}

impl_post_process_effect!(ComponentFog);

impl Component for ComponentFog {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for ComponentFog {
    fn default() -> Self {
        Self {
            mode: FogMode::Linear,
            color: Color32::from_rgb(180, 196, 212),
            intensity: 1.0,
            start: 0.0,
            end: 300.0,
            density: 0.01,
            opacity: 1.0,
        }
    }
}

/// How fog thickens with distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "c4e88c0d-130e-45e3-886c-23e8432b0d24"]
pub enum FogMode {
    /// None before `start`, rising linearly to full at `end`.
    #[default]
    Linear,
    /// `1 - exp(-density * distance)`: thin close to the camera, never
    /// quite full.
    Exponential,
}

impl FogMode {
    /// The value the fog shader switches on.
    pub(crate) fn shader_index(self) -> u32 {
        match self {
            Self::Linear => 0,
            Self::Exponential => 1,
        }
    }
}
