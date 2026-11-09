//! Hands what the player's side sees to the renderer's fog of war.
//!
//! The renderer darkens every pixel by the ground position of the surface it
//! shows ([`ComponentFogOfWar`]), so cover a tank sees is lit up to its top
//! while the ground in its shadow stays dark wherever it lands on screen.
//! This module hands it the side's tanks and the arena's sight blockers; the
//! renderer traces each pixel's lines of sight past the blockers itself, so
//! shadow sides are exact and stay crisp while tanks move, and each tank's
//! sight fades with distance along [`range_fade`]. A tank that joins the
//! side fades its sight in over [`FADE_TIME`], and one that leaves fades it
//! out where it last stood, rather than popping.
//!
//! The minimap lights the same sight, from rays traced past the same
//! blockers.

use super::{GroundRect, SightRays, TeamVision, Viewer, FADE_TIME};
use calyx_engine::component::{ComponentFogOfWar, FogOfWarBlocker, FogOfWarSight, FogOfWarViewer};
use calyx_engine::scene::Scene;
use calyx_engine::utils::Uuid;
use std::collections::HashMap;

/// How far inside a tank's vision range the fade towards the range starts,
/// in metres.
pub(crate) const RANGE_FADE_INSIDE: f32 = 8.0;
/// How far past a tank's vision range the fade reaches full darkness, in
/// metres.
pub(crate) const RANGE_FADE_OUTSIDE: f32 = 12.0;
/// Steepness of the range fade: it darkens slowly near the tank and faster
/// further out, as `(e^(k t) - 1) / (e^k - 1)` over the fade.
pub(crate) const RANGE_FADE_CURVE: f32 = 3.0;
/// Longest step of fog time one update lets pass, in seconds, so a long
/// frame does not count as fading.
const MAX_STEP: f32 = 0.1;

/// How much a tank sees at `distance` from it given its vision `range`:
/// 1 up to [`RANGE_FADE_INSIDE`] short of the range, falling along an
/// exponential to 0 at [`RANGE_FADE_OUTSIDE`] past it. The renderer's
/// viewers fade so, from the bounds [`fog_viewer`] gives them; the tests
/// hold them to it.
#[cfg(test)]
pub(crate) fn range_fade(distance: f32, range: f32) -> f32 {
    let start = range - RANGE_FADE_INSIDE;
    let t = ((distance - start) / (RANGE_FADE_INSIDE + RANGE_FADE_OUTSIDE)).clamp(0.0, 1.0);
    1.0 - (RANGE_FADE_CURVE * t).exp_m1() / RANGE_FADE_CURVE.exp_m1()
}

/// `count` evenly spaced sight rays from each of `sight`'s viewers, as far
/// as its sight reaches past `sight`'s blockers before its range fade ends,
/// for the minimap to light. A ray reaches where the pass's lines of sight
/// along it stop.
pub(crate) fn sight_rays(sight: &FogOfWarSight, count: usize) -> Vec<SightRays> {
    TeamVision {
        viewers: sight
            .viewers
            .iter()
            .map(|viewer| Viewer {
                id: Uuid::nil(),
                position: viewer.position,
                range: viewer.fade_end.max(0.0),
            })
            .collect(),
        cover: sight
            .blockers
            .iter()
            .map(|blocker| GroundRect {
                center: blocker.center,
                half_axes: blocker.half_axes,
            })
            .collect(),
    }
    .sight_rays(count)
}

/// The renderer's viewer for `viewer`, lending `weight` of its sight.
fn fog_viewer(viewer: &Viewer, weight: f32) -> FogOfWarViewer {
    FogOfWarViewer {
        position: viewer.position,
        fade_start: viewer.range - RANGE_FADE_INSIDE,
        fade_end: viewer.range + RANGE_FADE_OUTSIDE,
        fade_curve: RANGE_FADE_CURVE,
        weight,
    }
}

/// Sets `blockers` to the blocks of `cover` that can shade any ground
/// `viewers` might see: those within reach of a viewer's faded-out range.
fn near_blockers(
    cover: &[GroundRect],
    viewers: &[FogOfWarViewer],
    blockers: &mut Vec<FogOfWarBlocker>,
) {
    blockers.clear();
    blockers.extend(
        cover
            .iter()
            .map(|cover| FogOfWarBlocker {
                center: cover.center,
                half_axes: cover.half_axes,
            })
            .filter(|blocker| {
                viewers.iter().any(|viewer| {
                    (blocker.center - viewer.position).norm()
                        <= viewer.fade_end + blocker.radius() + FogOfWarSight::TOUCHING
                })
            }),
    );
}

/// What the side `vision` describes sees right now, with no easing.
pub(crate) fn side_sight(vision: &TeamVision) -> FogOfWarSight {
    let viewers = vision
        .viewers
        .iter()
        .map(|viewer| fog_viewer(viewer, 1.0))
        .collect::<Vec<_>>();
    let mut blockers = Vec::new();
    near_blockers(&vision.cover, &viewers, &mut blockers);
    FogOfWarSight { viewers, blockers }
}

/// The side's sight as fog shows it, with tanks joining and leaving the
/// side eased in and out over time.
#[derive(Clone, Debug, Default)]
pub(crate) struct FogSight {
    /// The tank behind each viewer of `sight`: the side's tanks in the
    /// order the side lists them, then the ones that left the side and are
    /// still fading their sight out.
    ids: Vec<Uuid>,
    sight: FogOfWarSight,
    /// Whether fog has shown anything yet; the first update shows the
    /// side's sight in full.
    started: bool,
}

impl FogSight {
    /// Moves the shown sight `dt` seconds towards what `vision` sees: each
    /// tank's sight follows it at once, while the weight of a tank joining
    /// the side rises from 0 to 1, and that of one leaving it falls to 0,
    /// over [`FADE_TIME`].
    pub fn update(&mut self, vision: &TeamVision, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, MAX_STEP)
        } else {
            0.0
        };
        let step = dt / FADE_TIME;
        let mut previous = std::mem::take(&mut self.ids)
            .into_iter()
            .zip(self.sight.viewers.drain(..))
            .collect::<HashMap<_, _>>();
        for viewer in &vision.viewers {
            let weight = match previous.remove(&viewer.id) {
                Some(shown) => (shown.weight + step).min(1.0),
                None if !self.started => 1.0,
                None => step.min(1.0),
            };
            self.ids.push(viewer.id);
            self.sight.viewers.push(fog_viewer(viewer, weight));
        }
        // Tanks that left the side fade their sight out where they last
        // stood.
        let mut leaving = previous.into_iter().collect::<Vec<_>>();
        leaving.sort_by_key(|(id, _)| *id);
        for (id, mut shown) in leaving {
            shown.weight -= step;
            if shown.weight > 0.0 {
                self.ids.push(id);
                self.sight.viewers.push(shown);
            }
        }
        near_blockers(&vision.cover, &self.sight.viewers, &mut self.sight.blockers);
        self.started = true;
    }

    /// The sight fog shows.
    pub fn sight(&self) -> &FogOfWarSight {
        &self.sight
    }
}

/// Hands `sight` to every fog of war effect in `scene`.
pub(crate) fn show_sight(scene: &mut Scene, sight: &FogOfWarSight) {
    let effects = scene
        .objects_with::<ComponentFogOfWar>()
        .collect::<Vec<_>>();
    for object in effects {
        let _ = scene.write_component::<ComponentFogOfWar, _>(object, |fog| match &mut fog.sight {
            Some(shown) => shown.clone_from(sight),
            None => fog.sight = Some(sight.clone()),
        });
    }
}

#[cfg(test)]
mod tests;
