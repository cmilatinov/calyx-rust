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
/// blocker's corners, wherever the viewer stands. A [`FogOfWarBorder`]
/// bounds what the side can see at all.
#[derive(Debug, Default, PartialEq)]
pub struct FogOfWarSight {
    /// The side's viewers. With none, the side sees nothing.
    pub viewers: Vec<FogOfWarViewer>,
    /// What blocks sight.
    pub blockers: Vec<FogOfWarBlocker>,
    /// The wall round the playable area, if it has one: nothing past it is
    /// ever seen.
    pub border: FogOfWarBorder,
}

impl Clone for FogOfWarSight {
    fn clone(&self) -> Self {
        Self {
            viewers: self.viewers.clone(),
            blockers: self.blockers.clone(),
            border: self.border.clone(),
        }
    }

    /// Reuses the viewer and blocker storage, so a sight game code hands
    /// over every frame does not allocate once warm.
    fn clone_from(&mut self, source: &Self) {
        self.viewers.clone_from(&source.viewers);
        self.blockers.clone_from(&source.blockers);
        self.border.clone_from(&source.border);
    }
}

impl FogOfWarSight {
    /// How far apart two blockers along a line of sight may be, in world
    /// units, and still count as touching, so sight carries on from one
    /// into the next.
    pub const TOUCHING: f32 = 0.05;

    /// How far past the side of a blocker a line of sight leaves by, in
    /// world units, a point may lie and still count as on the blocker. The
    /// pass rebuilds every pixel of a wall's face to a point on the edge of
    /// the wall's footprint, give or take the depth's rounding, so without
    /// it whether the face of a wall in sight turned from the viewer is lit
    /// or in shadow falls either way from pixel to pixel and frame to frame.
    pub const EDGE: f32 = 0.005;

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
    /// to 1: the most any viewer sees of it. A viewer outside the border
    /// sees nothing. A viewer outside the border
    /// sees nothing.
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
    /// between them leaves no blocker into open ground more than
    /// [`Self::EDGE`] before `to`. Sight carries on through blockers that
    /// overlap or touch, within [`Self::TOUCHING`], so a point on or within
    /// the run of touching blocks the line meets first is seen, and only the
    /// ground past the run's far side is in shadow. The border stops sight
    /// as [`FogOfWarBorder::blocks`] says.
    pub fn clear_line(&self, from: Vec2, to: Vec2) -> bool {
        if self.border.blocks(from, to) {
            return false;
        }
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
            exit < distance - Self::EDGE
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

/// The playable area's border, as fog of war sees it: the wall round it,
/// between two closed rings on the ground, the wall's inner face and its
/// outer face, vertices `(x, z)` in order either way round.
///
/// The wall blocks sight as a blocker does: a line of sight that leaves
/// the area into the wall sees the wall, its top and faces, but nothing
/// past it, whether it comes back into the area through the wall's other
/// side, as past a cove's point, or carries on out past the outer face.
/// Ground outside the outer face is never seen, and a viewer outside the
/// inner face sees nothing. Being one wall rather than a run of blocks,
/// it has no joints at its bends for sight to slip through, and it closes
/// any gap in the wall it is drawn across, as where water leaves the area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FogOfWarBorder {
    /// The wall's inner face, round the playable area.
    pub inner: Vec<Vec2>,
    /// The wall's outer face, round the inner one.
    pub outer: Vec<Vec2>,
}

impl FogOfWarBorder {
    /// How far apart along a line of sight two crossings of the inner face
    /// must be, in world units, for the line to count as having left the
    /// area and come back into it. Two edges meeting at a vertex the line
    /// passes through cross it at one point.
    pub const REENTRY: f32 = 1e-3;

    /// Whether there is no border: a face with fewer than three vertices.
    pub fn is_empty(&self) -> bool {
        self.inner.len() < 3 || self.outer.len() < 3
    }

    /// Whether `point` lies within the inner face; always, without a border.
    pub fn contains(&self, point: Vec2) -> bool {
        self.is_empty() || ring_contains(&self.inner, point)
    }

    /// Whether the border stops sight from `from` reaching `to`: `from` is
    /// outside the inner face, or the line between them crosses the outer
    /// face, or crosses the inner face twice, before `to`. A crossing of an
    /// edge that `to` lies within [`FogOfWarSight::EDGE`] of does not
    /// count, so a point on either face is on the wall, give or take the
    /// depth's rounding, as a point on a blocker's edge is.
    pub fn blocks(&self, from: Vec2, to: Vec2) -> bool {
        if self.is_empty() {
            return false;
        }
        if !ring_contains(&self.inner, from) {
            return true;
        }
        let offset = to - from;
        let distance = offset.norm();
        if distance < 1e-6 {
            return false;
        }
        let direction = offset / distance;
        // Where the line crosses the edges of `ring` further than the edge
        // margin from `to`.
        fn crossings(
            ring: &[Vec2],
            from: Vec2,
            to: Vec2,
            direction: Vec2,
            distance: f32,
        ) -> impl Iterator<Item = f32> + '_ {
            ring_edges(ring).filter_map(move |(a, b)| {
                edge_crossing(from, direction, distance, a, b)
                    .filter(|_| segment_distance(to, a, b) > FogOfWarSight::EDGE)
            })
        }
        if crossings(&self.outer, from, to, direction, distance)
            .next()
            .is_some()
        {
            return true;
        }
        let (first, last) = crossings(&self.inner, from, to, direction, distance)
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), t| {
                (low.min(t), high.max(t))
            });
        last - first > Self::REENTRY
    }

    /// How far a line of sight from `from` along the unit `direction` sees
    /// past the border, up to `limit`: to where it first crosses the outer
    /// face or crosses the inner face the second time. 0 from outside the
    /// inner face.
    pub fn reach(&self, from: Vec2, direction: Vec2, limit: f32) -> f32 {
        if self.is_empty() {
            return limit;
        }
        if !ring_contains(&self.inner, from) {
            return 0.0;
        }
        let mut reach = ring_edges(&self.outer)
            .filter_map(|(a, b)| edge_crossing(from, direction, limit, a, b))
            .fold(limit, f32::min);
        let mut inner = ring_edges(&self.inner)
            .filter_map(|(a, b)| edge_crossing(from, direction, limit, a, b))
            .collect::<Vec<_>>();
        inner.sort_by(f32::total_cmp);
        if let Some(&first) = inner.first() {
            if let Some(&again) = inner.iter().find(|&&t| t - first > Self::REENTRY) {
                reach = reach.min(again);
            }
        }
        reach
    }
}

/// The edges of the closed ring `ring`, each as its two ends.
pub fn ring_edges(ring: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    (0..ring.len()).map(move |i| (ring[i], ring[(i + 1) % ring.len()]))
}

/// Whether `point` lies inside the closed ring `ring`, by the even-odd rule.
pub fn ring_contains(ring: &[Vec2], point: Vec2) -> bool {
    let mut inside = false;
    for (a, b) in ring_edges(ring) {
        if (a.y > point.y) != (b.y > point.y)
            && point.x < a.x + (point.y - a.y) / (b.y - a.y) * (b.x - a.x)
        {
            inside = !inside;
        }
    }
    inside
}

/// Where the line from `start` along the unit `direction` crosses the edge
/// from `a` to `b` within `limit` of `start`, as the distance along it; a
/// line along the edge crosses it nowhere.
pub fn edge_crossing(start: Vec2, direction: Vec2, limit: f32, a: Vec2, b: Vec2) -> Option<f32> {
    let edge = b - a;
    let denom = direction.x * edge.y - direction.y * edge.x;
    if denom.abs() < 1e-9 {
        return None;
    }
    let w = a - start;
    let t = (w.x * edge.y - w.y * edge.x) / denom;
    let u = (w.x * direction.y - w.y * direction.x) / denom;
    (t >= 0.0 && t <= limit && (0.0..=1.0).contains(&u)).then_some(t)
}

/// How far `point` lies from the segment from `a` to `b`.
pub fn segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let edge = b - a;
    let length = edge.norm_squared();
    let f = if length > 0.0 {
        ((point - a).dot(&edge) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (point - (a + edge * f)).norm()
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
