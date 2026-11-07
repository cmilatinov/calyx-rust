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
/// the effect in [`Self::sight`] every frame, as the side's viewers and the
/// blockers that cast their shadows. Each pixel is fogged by the ground
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

/// Everything a side sees: the union of its viewers' sight over the
/// ground, with the blockers that cast its shadows.
///
/// A viewer sees a ground point when nothing blocks the line from the
/// viewer to it, as much as its distance fade and weight leave. Sight
/// carries on through the run of touching blockers it meets first, so a
/// blocker the side sees is lit, along with the blocks it touches, while
/// the ground behind them is in shadow. Lines of sight are traced per
/// point, so shadow sides are exact lines from the viewer past each
/// blocker's corners, wherever the viewer stands.
#[derive(Debug, Default, PartialEq)]
pub struct FogOfWarSight {
    /// The side's viewers. With none, the side sees nothing.
    pub viewers: Vec<FogOfWarViewer>,
    /// What blocks sight.
    pub blockers: Vec<FogOfWarBlocker>,
}

impl Clone for FogOfWarSight {
    fn clone(&self) -> Self {
        Self {
            viewers: self.viewers.clone(),
            blockers: self.blockers.clone(),
        }
    }

    /// Reuses the viewer and blocker storage, so a sight game code hands
    /// over every frame does not allocate once warm.
    fn clone_from(&mut self, source: &Self) {
        self.viewers.clone_from(&source.viewers);
        self.blockers.clone_from(&source.blockers);
    }
}

impl FogOfWarSight {
    /// How far apart two blockers along a line of sight may be, in world
    /// units, and still count as touching, so sight carries on from one
    /// into the next.
    pub const TOUCHING: f32 = 0.05;

    /// Where the fog of war pass samples sight inside a pixel, in pixels
    /// from its centre: a rotated grid, so near-horizontal and
    /// near-vertical shadow sides each cross four distinct sample rows.
    pub const PIXEL_SAMPLES: [[f32; 2]; 4] = [
        [0.125, 0.375],
        [0.375, -0.125],
        [-0.125, -0.375],
        [-0.375, 0.125],
    ];

    /// How much of the ground point `point`, `(x, z)`, the side sees, from 0
    /// to 1: the most any viewer sees of it.
    pub fn visibility(&self, point: Vec2) -> f32 {
        let mut seen = 0.0f32;
        for viewer in &self.viewers {
            let sight = viewer.sight((point - viewer.position).norm());
            if sight > seen && self.clear_line(viewer.position, point) {
                seen = sight;
            }
        }
        seen
    }

    /// How much of a pixel the side sees, as the fog of war pass computes
    /// it: the mean [`Self::visibility`] at [`Self::PIXEL_SAMPLES`] around
    /// the ground point `point` the pixel's centre shows, a pixel to the
    /// right on screen lying `dx` further on the ground and a pixel down
    /// `dy`. Averaging at the pixel's own scale antialiases shadow sides
    /// without softening them.
    pub fn pixel_visibility(&self, point: Vec2, dx: Vec2, dy: Vec2) -> f32 {
        Self::PIXEL_SAMPLES
            .iter()
            .map(|&[x, y]| self.visibility(point + dx * x + dy * y))
            .sum::<f32>()
            / Self::PIXEL_SAMPLES.len() as f32
    }

    /// Whether sight reaches from `from` to `to` over the ground: the line
    /// between them leaves no blocker before `to` into open ground. Sight
    /// carries on through blockers that overlap or touch, within
    /// [`Self::TOUCHING`], so a point on or within the run of touching
    /// blocks the line meets first is seen, and only the ground past the
    /// run's far side is in shadow.
    pub fn clear_line(&self, from: Vec2, to: Vec2) -> bool {
        let offset = to - from;
        let distance = offset.norm();
        if distance < 1e-6 {
            return true;
        }
        let direction = offset / distance;
        let limit = distance + Self::TOUCHING;
        let spans = self
            .blockers
            .iter()
            .filter_map(|blocker| blocker.span(from, direction, limit))
            .collect::<Vec<_>>();
        !spans.iter().any(|&(_, exit)| {
            exit <= distance
                && !spans
                    .iter()
                    .any(|&(near, far)| near <= exit + Self::TOUCHING && far > exit)
        })
    }
}

/// A rectangle on the ground that blocks sight, such as a wall or a crate:
/// its centre and the two half-axes from the centre to the middle of two
/// adjacent sides, in `(x, z)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FogOfWarBlocker {
    pub center: Vec2,
    pub half_axes: [Vec2; 2],
}

impl FogOfWarBlocker {
    /// The rows of the matrix that takes a ground offset from the centre to
    /// the blocker's local space, where it spans -1 to 1 on both axes, or
    /// `None` when the blocker is flat and blocks nothing.
    pub fn to_local(&self) -> Option<[Vec2; 2]> {
        let [a, b] = self.half_axes;
        let det = a.x * b.y - a.y * b.x;
        if det.abs() < 1e-6 {
            return None;
        }
        Some([
            Vec2::new(b.y / det, -b.x / det),
            Vec2::new(-a.y / det, a.x / det),
        ])
    }

    /// Distance from the centre to the farthest corner.
    pub fn radius(&self) -> f32 {
        self.half_axes[0].norm() + self.half_axes[1].norm()
    }

    /// Where the line from `start` along the unit `direction` is inside the
    /// blocker within `limit` of `start`: the distances along it where it
    /// enters, no closer than `start`, and leaves, no further than `limit`.
    pub fn span(&self, start: Vec2, direction: Vec2, limit: f32) -> Option<(f32, f32)> {
        let [x, y] = self.to_local()?;
        let relative = start - self.center;
        let origin = [x.dot(&relative), y.dot(&relative)];
        let step = [x.dot(&direction), y.dot(&direction)];
        let mut near = f32::NEG_INFINITY;
        let mut far = f32::INFINITY;
        for axis in 0..2 {
            if step[axis].abs() < 1e-6 {
                if origin[axis].abs() > 1.0 {
                    return None;
                }
            } else {
                let a = (-1.0 - origin[axis]) / step[axis];
                let b = (1.0 - origin[axis]) / step[axis];
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
        }
        (near <= far && far >= 0.0 && near <= limit).then(|| (near.max(0.0), far.min(limit)))
    }
}

/// One viewer's sight over the ground: where it stands, how its sight
/// fades with distance, and how much of its sight it lends the side.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// How much of its sight the viewer lends the side, from 0 to 1, so a
    /// viewer joining or leaving the side fades its sight in or out.
    pub weight: f32,
}

impl Default for FogOfWarViewer {
    fn default() -> Self {
        Self {
            position: Vec2::zeros(),
            fade_start: 0.0,
            fade_end: 0.0,
            fade_curve: 0.0,
            weight: 1.0,
        }
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

    /// How much the viewer lends the side at `distance` from it, cover
    /// aside: its [`Self::range_fade`] scaled by its weight.
    pub fn sight(&self, distance: f32) -> f32 {
        self.range_fade(distance) * self.weight.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests;
