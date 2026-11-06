//! Hands what the player's side sees to the renderer's fog of war.
//!
//! The renderer darkens every pixel by the ground position of the surface it
//! shows ([`ComponentFogOfWar`]), so cover a tank sees is lit up to its top
//! while the ground in its shadow stays dark wherever it lands on screen.
//! This module builds that sight from the side's sight rays: each tank sees
//! along its rays as far as their reach through the cover they meet, with
//! hard shadow sides, and its sight fades with distance along
//! [`range_fade`]. When what the side sees changes, each ray's reach eases
//! towards its new value over [`FADE_TIME`] instead of jumping, so fog
//! sweeps open and closed rather than popping; a tank that joins the side
//! opens its sight from nothing, and one that leaves closes it.
//!
//! The minimap lights the same sight, so the map and the main view agree.

use super::{SightRays, TeamVision, FADE_TIME};
use calyx_engine::component::{ComponentFogOfWar, FogOfWarSight, FogOfWarViewer};
use calyx_engine::scene::Scene;
use calyx_engine::utils::Uuid;
use std::collections::HashMap;

/// Sight rays per tank; a cover shadow's side lies between two rays.
pub(crate) const RAYS: usize = 720;
/// How far inside a tank's vision range the fade towards the range starts,
/// in metres.
pub(crate) const RANGE_FADE_INSIDE: f32 = 8.0;
/// How far past a tank's vision range the fade reaches full darkness, in
/// metres.
pub(crate) const RANGE_FADE_OUTSIDE: f32 = 12.0;
/// Steepness of the range fade: it darkens slowly near the tank and faster
/// further out, as `(e^(k t) - 1) / (e^k - 1)` over the fade.
pub(crate) const RANGE_FADE_CURVE: f32 = 3.0;
/// How much two neighbouring rays' reaches may differ, in metres, before
/// the edge between them is a shadow's side, kept hard and radial.
pub(crate) const SHADOW_JUMP: f32 = 0.3;
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

/// The sight rays fog reads: `vision`'s, reaching past each tank's range
/// by the whole range fade, so only cover cuts them short and the range
/// edge comes from [`range_fade`] alone.
fn fog_rays(vision: &TeamVision) -> Vec<SightRays> {
    let mut extended = vision.clone();
    for viewer in &mut extended.viewers {
        viewer.range += RANGE_FADE_OUTSIDE;
    }
    extended.sight_rays(RAYS)
}

/// The renderer's viewer for a tank of vision `range` at `rays`' origin,
/// seeing as far as `reach` along its rays.
fn fog_viewer(rays: &SightRays, range: f32, reach: Vec<f32>) -> FogOfWarViewer {
    FogOfWarViewer {
        position: rays.viewer.position,
        fade_start: range - RANGE_FADE_INSIDE,
        fade_end: range + RANGE_FADE_OUTSIDE,
        fade_curve: RANGE_FADE_CURVE,
        reach,
        shadow_jump: SHADOW_JUMP,
    }
}

/// What the side `vision` describes sees right now, with no easing.
pub(crate) fn side_sight(vision: &TeamVision) -> FogOfWarSight {
    FogOfWarSight {
        viewers: fog_rays(vision)
            .into_iter()
            .map(|rays| {
                let range = rays.viewer.range - RANGE_FADE_OUTSIDE;
                let reach = rays.through.clone();
                fog_viewer(&rays, range, reach)
            })
            .collect(),
    }
}

/// The side's sight as fog shows it, eased over time.
#[derive(Clone, Debug, Default)]
pub(crate) struct FogSight {
    /// The tank behind each viewer of `sight`: the side's tanks in the
    /// order the side lists them, then the ones that left the side and are
    /// still closing their sight.
    ids: Vec<Uuid>,
    sight: FogOfWarSight,
    /// Whether fog has shown anything yet; the first update shows the
    /// sight as it is.
    started: bool,
}

impl FogSight {
    /// Eases the shown sight `dt` seconds towards what `vision` sees: each
    /// ray's reach moves towards its new value by at most its tank's whole
    /// sight, to the end of its range fade, per [`FADE_TIME`].
    pub fn update(&mut self, vision: &TeamVision, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, MAX_STEP)
        } else {
            0.0
        };
        let step = dt / FADE_TIME;
        let mut previous = std::mem::take(&mut self.ids)
            .into_iter()
            .zip(std::mem::take(&mut self.sight.viewers))
            .collect::<HashMap<_, _>>();
        let ease = |shown: &mut Vec<f32>, target: &[f32], most: f32| {
            shown.resize(target.len(), 0.0);
            for (shown, &target) in shown.iter_mut().zip(target) {
                *shown += (target - *shown).clamp(-most, most);
            }
        };
        for (viewer, rays) in vision.viewers.iter().zip(fog_rays(vision)) {
            let most = (viewer.range + RANGE_FADE_OUTSIDE) * step;
            let target = &rays.through;
            let reach = match previous.remove(&viewer.id) {
                Some(mut shown) if shown.reach.len() == target.len() => {
                    ease(&mut shown.reach, target, most);
                    shown.reach
                }
                _ if !self.started => target.clone(),
                _ => {
                    let mut reach = Vec::new();
                    ease(&mut reach, target, most);
                    reach
                }
            };
            self.ids.push(viewer.id);
            self.sight
                .viewers
                .push(fog_viewer(&rays, viewer.range, reach));
        }
        // Tanks that left the side close their sight where they last stood.
        let mut leaving = previous.into_iter().collect::<Vec<_>>();
        leaving.sort_by_key(|(id, _)| *id);
        for (id, mut shown) in leaving {
            let most = shown.fade_end * step;
            let closed = vec![0.0; shown.reach.len()];
            ease(&mut shown.reach, &closed, most);
            if shown.reach.iter().any(|&reach| reach > 0.0) {
                self.ids.push(id);
                self.sight.viewers.push(shown);
            }
        }
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
        .objects()
        .filter(|&object| {
            scene
                .read_component::<ComponentFogOfWar, _, _>(object, |_| ())
                .is_some()
        })
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
