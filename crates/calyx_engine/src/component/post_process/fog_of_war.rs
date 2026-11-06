//! The fog of war effect component, read by the fog of war pass.

use super::{PostProcessEffect, ReflectPostProcessEffect};
use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use nalgebra_glm::Vec2;
use serde::{Deserialize, Serialize};

/// Fog of war: darkens every surface a side cannot see, after tone mapping.
///
/// What the side sees is game state, not a setting: game code hands it to
/// the effect in [`Self::sight`] every frame, as the sight of each of the
/// side's viewers over the ground. Each pixel is fogged by the ground
/// position, `(x, z)`, of the surface it shows, rebuilt from the scene
/// depth, so a wall a viewer sees is lit up to its top while the ground
/// behind it, wherever it lands on screen, stays dark. While no game code
/// supplies a sight, as in edit mode, the effect draws nothing.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect, PostProcessEffect,
)]
#[uuid = "0592aa8e-ecd0-4aa4-9509-b4c030e7794c"]
#[reflect(Default, TypeUuidDynamic, Component, PostProcessEffect)]
#[reflect_attr(name = "Fog Of War", category = "Post Processing")]
#[serde(default)]
pub struct ComponentFogOfWar {
    /// The colour unseen surfaces are darkened towards, sRGB.
    pub color: Color32,
    /// The share of the fog colour a surface nobody sees takes, from 0 (no
    /// fog) to 1. It mixes in sRGB-encoded colour, as a translucent overlay
    /// drawn over the image would.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub opacity: f32,
    /// What the side sees this frame. Game code sets it every frame; while
    /// it is `None` the effect draws nothing. It is runtime state: never
    /// saved, and a profile's copy of the effect has none.
    #[serde(skip)]
    #[reflect_skip]
    pub sight: Option<FogOfWarSight>,
}

impl Component for ComponentFogOfWar {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for ComponentFogOfWar {
    fn default() -> Self {
        Self {
            color: Color32::BLACK,
            opacity: 0.64,
            sight: None,
        }
    }
}

/// Everything a side sees: the union of its viewers' sight, each ground
/// point as seen as the viewer that sees it best.
#[derive(Debug, Default, PartialEq)]
pub struct FogOfWarSight {
    /// The side's viewers. With none, the side sees nothing.
    pub viewers: Vec<FogOfWarViewer>,
}

impl Clone for FogOfWarSight {
    fn clone(&self) -> Self {
        Self {
            viewers: self.viewers.clone(),
        }
    }

    /// Reuses the viewers' storage, as [`FogOfWarViewer::clone_from`] does.
    fn clone_from(&mut self, source: &Self) {
        self.viewers.clone_from(&source.viewers);
    }
}

impl FogOfWarSight {
    /// How much of the ground point `point`, `(x, z)`, the side sees, from 0
    /// to 1: the most any viewer sees of it. The fog of war pass computes
    /// the same per pixel.
    pub fn visibility(&self, point: Vec2) -> f32 {
        self.viewers
            .iter()
            .map(|viewer| viewer.visibility(point))
            .fold(0.0, f32::max)
    }
}

/// One viewer's sight over the ground: how far it sees along each of a
/// set of evenly spaced rays, and how its sight fades with distance.
///
/// A ground point is seen when it lies closer than the reach of the rays
/// either side of it, and then as much as the distance fade leaves. Where
/// the two rays' reaches differ by more than [`Self::shadow_jump`], the
/// edge between them is the side of a shadow: the point takes the reach of
/// the nearer ray, so the side is a hard radial line halfway between them.
/// Otherwise the reach is interpolated between the two, so an edge of sight
/// that runs along cover is smooth.
#[derive(Debug, Default, PartialEq)]
pub struct FogOfWarViewer {
    /// The viewer's ground position, `(x, z)`.
    pub position: Vec2,
    /// Distance from the viewer, in world units, where its sight starts to
    /// fade.
    pub fade_start: f32,
    /// Distance where its sight has faded out. At or below `fade_start`,
    /// sight ends sharply at `fade_start`.
    pub fade_end: f32,
    /// Steepness of the fade: over it, sight is
    /// `1 - (e^(k t) - 1) / (e^k - 1)` at `t` from 0 at `fade_start` to 1 at
    /// `fade_end`, so a positive `k` keeps sight bright for longer and drops
    /// it faster near the end. 0 fades linearly.
    pub fade_curve: f32,
    /// How far each ray reaches, in world units. Ray `i` of `n` points at
    /// the angle `i * 2π / n` from +X towards +Z, so ray `i` heads along
    /// `(cos, sin)` of that angle in `(x, z)`. With no rays, nothing cuts
    /// the viewer's sight short.
    pub reach: Vec<f32>,
    /// The least difference between neighbouring rays' reaches, in world
    /// units, that makes the edge between them a hard shadow side.
    pub shadow_jump: f32,
}

impl Clone for FogOfWarViewer {
    fn clone(&self) -> Self {
        Self {
            position: self.position,
            fade_start: self.fade_start,
            fade_end: self.fade_end,
            fade_curve: self.fade_curve,
            reach: self.reach.clone(),
            shadow_jump: self.shadow_jump,
        }
    }

    /// Reuses the reach storage, so settings resolved every frame do not
    /// allocate once warm.
    fn clone_from(&mut self, source: &Self) {
        self.position = source.position;
        self.fade_start = source.fade_start;
        self.fade_end = source.fade_end;
        self.fade_curve = source.fade_curve;
        self.reach.clone_from(&source.reach);
        self.shadow_jump = source.shadow_jump;
    }
}

impl FogOfWarViewer {
    /// How much the viewer sees at `distance` from it, cover aside: 1 up
    /// to `fade_start`, falling along the fade curve to 0 at `fade_end`.
    pub fn range_fade(&self, distance: f32) -> f32 {
        let t = if self.fade_end <= self.fade_start {
            if distance >= self.fade_start {
                1.0
            } else {
                0.0
            }
        } else {
            ((distance - self.fade_start) / (self.fade_end - self.fade_start)).clamp(0.0, 1.0)
        };
        let k = self.fade_curve;
        if k.abs() < 1e-4 {
            return 1.0 - t;
        }
        1.0 - (k * t).exp_m1() / k.exp_m1()
    }

    /// How far the viewer sees in the ground direction `offset`, `(x, z)`,
    /// from the rays either side of it.
    pub fn reach_at(&self, offset: Vec2) -> f32 {
        let count = self.reach.len();
        if count == 0 {
            return f32::INFINITY;
        }
        let tau = std::f32::consts::TAU;
        let turns = (offset.y.atan2(offset.x) / tau * count as f32).rem_euclid(count as f32);
        let index = (turns.floor() as usize).min(count - 1);
        let next = (index + 1) % count;
        let blend = turns - turns.floor();
        let (a, b) = (self.reach[index], self.reach[next]);
        if (a - b).abs() > self.shadow_jump {
            if blend >= 0.5 {
                b
            } else {
                a
            }
        } else {
            a + (b - a) * blend
        }
    }

    /// How much of the ground point `point`, `(x, z)`, the viewer sees: its
    /// [`Self::range_fade`] inside the reach of its rays, 0 at or past it.
    pub fn visibility(&self, point: Vec2) -> f32 {
        let offset = point - self.position;
        let distance = offset.norm();
        let fade = self.range_fade(distance);
        if fade <= 0.0 || distance >= self.reach_at(offset) {
            return 0.0;
        }
        fade
    }
}

#[cfg(test)]
mod tests;
