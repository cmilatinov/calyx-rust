//! What a side can see, and fog of war over what it cannot.
//!
//! A side sees a ground point when the point lies within any of its live
//! tanks' vision range and no block of arena cover stands between that tank
//! and the point. [`TeamVision`] answers that question; the minimap, the HUD
//! and [`apply_fog`] all ask it, so the map and the main view always agree.
//!
//! Fog only changes what the local player sees. A hidden object keeps its
//! colliders, health, update hooks and rounds; fog clears its render flag and
//! disables its lights and particle systems, and restores exactly what it
//! changed once the object is seen again.

use crate::arena::ComponentArena;
use crate::combat::{combat_team, is_tank_combatant};
use crate::combat_vfx::ComponentCombatEffectLifetime;
use crate::hud::ComponentTankHud;
use crate::sentry::ComponentSentry;
use crate::spawn::ComponentRespawnState;
use crate::tank::{
    ComponentHealth, ComponentProjectile, ComponentProjectileTarget, ComponentTankController,
};
use calyx_engine::component::{
    ColliderShape, ComponentCollider, ComponentID, ComponentParticleSystem, ComponentPointLight,
};
use calyx_engine::math::Transform;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::Uuid;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};
use std::collections::HashMap;

/// A rectangle on the ground: its centre and the two half-axes from the
/// centre to the middle of two adjacent sides. Ground positions are `(x, z)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GroundRect {
    pub center: Vec2,
    pub half_axes: [Vec2; 2],
}

impl GroundRect {
    /// A rectangle of `size` along X and Z, centred on `center`.
    #[cfg(test)]
    pub fn axis_aligned(center: Vec2, size: Vec2) -> Self {
        Self {
            center,
            half_axes: [vec2(size.x * 0.5, 0.0), vec2(0.0, size.y * 0.5)],
        }
    }

    /// The ground under a rectangle of `size` along its local X and Z,
    /// centred on `center` in the local space of `transform`.
    pub fn placed(transform: &Transform, center: Vec3, size: Vec2) -> Self {
        let half_axis = |local: Vec3| ground(transform.transform_direction(&local));
        Self {
            center: ground(transform.transform_position(&center)),
            half_axes: [
                half_axis(vec3(size.x * 0.5, 0.0, 0.0)),
                half_axis(vec3(0.0, 0.0, size.y * 0.5)),
            ],
        }
    }

    /// Distance from the centre to the farthest corner.
    fn reach(&self) -> f32 {
        self.half_axes[0].norm() + self.half_axes[1].norm()
    }
}

/// The ground position of a world position.
pub(crate) fn ground(position: Vec3) -> Vec2 {
    vec2(position.x, position.z)
}

/// Where the segment `start`..`end` is inside a transformed ground footprint,
/// as fractions of the segment. Inverting the two half axes handles rotated,
/// scaled and sheared arena parents without axis-aligned leaks.
fn cover_span(start: Vec2, end: Vec2, cover: GroundRect) -> Option<(f32, f32)> {
    let [a, b] = cover.half_axes;
    let det = a.x * b.y - a.y * b.x;
    if det.abs() < 1e-6 {
        return None;
    }
    let local = |v: Vec2| vec2((v.x * b.y - v.y * b.x) / det, (a.x * v.y - a.y * v.x) / det);
    let origin = local(start - cover.center);
    let direction = local(end - start);
    let mut near = f32::NEG_INFINITY;
    let mut far = f32::INFINITY;
    for axis in 0..2 {
        if direction[axis].abs() < 1e-6 {
            if origin[axis].abs() > 1.0 {
                return None;
            }
        } else {
            let a = (-1.0 - origin[axis]) / direction[axis];
            let b = (1.0 - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    (far >= 0.0 && near <= 1.0).then_some((near.max(0.0), far))
}

/// The fraction of `start`..`end` where it enters `cover`, if it does.
pub(crate) fn cover_entry(start: Vec2, end: Vec2, cover: GroundRect) -> Option<f32> {
    cover_span(start, end, cover).map(|(near, _)| near)
}

/// The arena's floor, walls included, and each wall-height block of cover.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ArenaGeometry {
    pub floor: GroundRect,
    pub cover: Vec<GroundRect>,
}

/// Reads the first arena's ground geometry from its live cover objects.
pub(crate) fn arena_geometry(scene: &Scene) -> Option<ArenaGeometry> {
    let (object, transform, ((width, depth), cell_size, cover, generated)) =
        scene.objects().find_map(|object| {
            scene
                .read_component::<ComponentArena, _, _>(object, |arena| {
                    Some((
                        arena.extent()?,
                        arena.cell_size,
                        arena.cover_positions(),
                        arena.generate_on_start,
                    ))
                })
                .flatten()
                .map(|arena| (object, scene.world_transform(object), arena))
        })?;
    let floor = GroundRect::placed(&transform, Vec3::zeros(), vec2(width, depth));
    let live = |child: &GameObject| {
        scene.is_visible_in_hierarchy(*child)
            && scene.is_component_enabled::<ComponentCollider>(*child)
    };
    let children = scene.children(object).collect::<Vec<_>>();
    let cover = if generated && children.is_empty() {
        cover
            .iter()
            .map(|position| GroundRect::placed(&transform, *position, vec2(cell_size, cell_size)))
            .collect()
    } else if generated {
        // Generated cover is one unit cube per cell; follow its live pose and
        // visibility.
        children
            .into_iter()
            .filter(|child| live(child) && scene.transform(*child).position.y > 0.0)
            .map(|child| {
                GroundRect::placed(&scene.world_transform(child), Vec3::zeros(), vec2(1.0, 1.0))
            })
            .collect()
    } else {
        // Authored kit instances have independent collider children; read
        // their actual pose, including edits.
        scene
            .descendants(object)
            .filter(|child| scene.name(*child).starts_with("Arena Cover") && live(child))
            .filter_map(|child| {
                let ColliderShape::Cuboid { half_extents } = scene
                    .read_component::<ComponentCollider, _, _>(child, |collider| collider.shape)?
                else {
                    return None;
                };
                Some(GroundRect::placed(
                    &scene.world_transform(child),
                    Vec3::zeros(),
                    vec2(half_extents.x * 2.0, half_extents.z * 2.0),
                ))
            })
            .collect()
    };
    Some(ArenaGeometry { floor, cover })
}

/// One tank lending its sight to its side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Viewer {
    pub position: Vec2,
    pub range: f32,
}

/// How far each of a viewer's evenly spaced sight rays reaches, for drawing
/// vision as an area rather than testing single points.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SightRays {
    pub viewer: Viewer,
    /// Where each ray first meets cover, or its full range.
    pub entry: Vec<f32>,
    /// Like `entry`, but a ray that meets cover carries on through the
    /// touching blocks it meets, so the cover itself is not in shadow.
    pub through: Vec<f32>,
}

impl SightRays {
    /// The ray angle of ray `index`, counterclockwise from +X on the ground.
    pub fn angle(&self, index: usize) -> f32 {
        index as f32 * std::f32::consts::TAU / self.entry.len() as f32
    }

    /// How far sight reaches along `angle`, through the cover it first
    /// meets, interpolating between the two nearest rays.
    fn reach_along(&self, angle: f32) -> f32 {
        let count = self.through.len();
        let turns = angle.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * count as f32;
        let index = turns.floor() as usize % count;
        let next = (index + 1) % count;
        let blend = turns.fract();
        self.through[index] * (1.0 - blend) + self.through[next] * blend
    }

    /// How much of `point` this viewer sees: 1 inside its sight, falling to 0
    /// over about `soft` metres past the edge of its range and across the
    /// edges of cover shadows.
    pub fn coverage(&self, point: Vec2, soft: f32) -> f32 {
        const SAMPLES: i32 = 3;
        let offset = point - self.viewer.position;
        let distance = offset.norm();
        if distance > self.viewer.range + soft * 2.0 + 2.0 {
            return 0.0;
        }
        let soft = soft.max(f32::EPSILON);
        let angle = offset.y.atan2(offset.x);
        // Sample a soft metre's arc either way, so shadow sides blur as much
        // as the end of sight does.
        let spread = (soft * 0.5 / distance.max(soft)) / SAMPLES as f32;
        let total = (-SAMPLES..=SAMPLES)
            .map(|step| {
                let reach = self.reach_along(angle + step as f32 * spread);
                let t = ((distance - reach) / soft).clamp(0.0, 1.0);
                1.0 - t * t * (3.0 - 2.0 * t)
            })
            .sum::<f32>();
        total / (2 * SAMPLES + 1) as f32
    }
}

/// Everything one side can currently see.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TeamVision {
    pub viewers: Vec<Viewer>,
    pub cover: Vec<GroundRect>,
}

impl TeamVision {
    /// The vision of `player`'s side: every live tank on `player`'s team,
    /// `player` included, sees over `cover`.
    pub fn of_player(scene: &Scene, player: GameObject, cover: Vec<GroundRect>) -> Self {
        let viewers = scene
            .objects()
            .filter(|&object| is_live_tank(scene, object) && is_friendly(scene, player, object))
            .filter_map(|object| {
                let range = scene
                    .read_component::<ComponentTankController, _, _>(object, |controller| {
                        controller.vision_range
                    })?;
                Some(Viewer {
                    position: ground(scene.world_transform(object).position),
                    range: range.max(0.0),
                })
            })
            .collect();
        Self { viewers, cover }
    }

    /// Reads the scene's arena cover and `player`'s side's vision over it.
    pub fn in_scene(scene: &Scene, player: GameObject) -> Self {
        let cover = arena_geometry(scene)
            .map(|arena| arena.cover)
            .unwrap_or_default();
        Self::of_player(scene, player, cover)
    }

    /// Whether no cover stands between `from` and `to`.
    pub fn clear_line(&self, from: Vec2, to: Vec2) -> bool {
        !self
            .cover
            .iter()
            .any(|cover| cover_entry(from, to, *cover).is_some_and(|t| t < 0.999))
    }

    /// Whether the side sees the ground point `point`.
    pub fn visible(&self, point: Vec2) -> bool {
        self.viewers.iter().any(|viewer| {
            (point - viewer.position).norm() <= viewer.range
                && self.clear_line(viewer.position, point)
        })
    }

    /// `count` evenly spaced sight rays of every viewer.
    pub fn sight_rays(&self, count: usize) -> Vec<SightRays> {
        let count = count.max(3);
        self.viewers
            .iter()
            .map(|&viewer| {
                let near = self
                    .cover
                    .iter()
                    .copied()
                    .filter(|cover| {
                        (cover.center - viewer.position).norm() <= viewer.range + cover.reach()
                    })
                    .collect::<Vec<_>>();
                let mut entry = Vec::with_capacity(count);
                let mut through = Vec::with_capacity(count);
                for index in 0..count {
                    let angle = index as f32 * std::f32::consts::TAU / count as f32;
                    let direction = vec2(angle.cos(), angle.sin());
                    let (hit, beyond) = ray_through_cover(viewer, direction, &near);
                    entry.push(hit);
                    through.push(beyond);
                }
                SightRays {
                    viewer,
                    entry,
                    through,
                }
            })
            .collect()
    }

    /// How much of `point` the side sees, from 0 to 1, with a soft edge of
    /// `soft` metres; `rays` are this vision's [`Self::sight_rays`].
    pub fn coverage(rays: &[SightRays], point: Vec2, soft: f32) -> f32 {
        rays.iter()
            .map(|rays| rays.coverage(point, soft))
            .fold(0.0, f32::max)
    }
}

/// How far a sight ray reaches before it meets cover, and how far it reaches
/// once it carries on through the run of touching blocks it met.
fn ray_through_cover(viewer: Viewer, direction: Vec2, cover: &[GroundRect]) -> (f32, f32) {
    // Reach a little past the range so a block straddling the edge of sight
    // is still carried through.
    let length = viewer.range + 4.0;
    let start = viewer.position;
    let end = start + direction * length;
    let spans = cover
        .iter()
        .filter_map(|cover| cover_span(start, end, *cover))
        .map(|(near, far)| (near * length, far.min(1.0) * length))
        .collect::<Vec<_>>();
    let Some(&(entry, mut exit)) = spans
        .iter()
        .filter(|(near, _)| *near < viewer.range)
        .min_by(|a, b| a.0.total_cmp(&b.0))
    else {
        return (viewer.range, viewer.range);
    };
    // Blocks of one prop touch or overlap; walk through all of them.
    loop {
        let further = spans
            .iter()
            .filter(|(near, far)| *near <= exit + 0.05 && *far > exit)
            .map(|(_, far)| *far)
            .fold(exit, f32::max);
        if further <= exit {
            break;
        }
        exit = further;
    }
    (entry, exit)
}

/// Whether `object` is a tank that is alive and in play.
fn is_live_tank(scene: &Scene, object: GameObject) -> bool {
    is_tank_combatant(scene, object)
        && scene
            .read_component::<ComponentRespawnState, _, _>(object, |state| state.alive)
            .unwrap_or(true)
        && !scene
            .read_component::<ComponentHealth, _, _>(object, |health| health.dead)
            .unwrap_or(false)
}

/// Whether `object` is on `viewer`'s side: `viewer` itself, or a tank of its
/// team. Sentries and practice targets belong to no side.
pub(crate) fn is_friendly(scene: &Scene, viewer: GameObject, object: GameObject) -> bool {
    object == viewer
        || combat_team(scene, viewer)
            .zip(combat_team(scene, object))
            .is_some_and(|(a, b)| a == b)
}

/// What fog decides for one object it manages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sighting {
    Seen,
    Hidden,
}

/// What fog of war changed in the scene, so it can undo exactly that.
#[derive(Clone, Debug, Default)]
pub(crate) struct FogState {
    /// Each object fog hid, with the lights and particle systems in its
    /// hierarchy that fog disabled.
    hidden: HashMap<Uuid, Vec<(Uuid, Effect)>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    Light,
    Particles,
}

impl FogState {
    /// Whether fog currently hides the object with id `id`.
    #[cfg(test)]
    pub fn hides(&self, id: Uuid) -> bool {
        self.hidden.contains_key(&id)
    }
}

/// Decides what fog does with `object` for `viewer`'s side, or `None` when
/// fog does not manage it.
fn sighting(
    scene: &Scene,
    viewer: GameObject,
    vision: &TeamVision,
    object: GameObject,
) -> Option<Sighting> {
    let seen = |visible: bool| {
        Some(if visible {
            Sighting::Seen
        } else {
            Sighting::Hidden
        })
    };
    let position = ground(scene.world_transform(object).position);
    if let Some(round) = scene.read_component::<ComponentProjectile, _, _>(object, |round| *round) {
        let friendly = round
            .shooter
            .game_object(scene)
            .is_some_and(|shooter| is_friendly(scene, viewer, shooter));
        return seen(
            friendly
                || vision.visible(position)
                || heads_for(scene, vision, object, &round, viewer),
        );
    }
    if let Some(effect) =
        scene.read_component::<ComponentCombatEffectLifetime, _, _>(object, |effect| *effect)
    {
        // A smoke trail shows exactly while the round it follows does.
        if let Some(source) = effect.follow.game_object(scene) {
            if source != object {
                return sighting(scene, viewer, vision, source).or(Some(Sighting::Seen));
            }
        }
        return seen(vision.visible(position));
    }
    let is_unit = scene
        .read_component::<ComponentTankController, _, _>(object, |_| ())
        .or_else(|| scene.read_component::<ComponentSentry, _, _>(object, |_| ()))
        .or_else(|| scene.read_component::<ComponentProjectileTarget, _, _>(object, |_| ()))
        .is_some();
    if !is_unit {
        return None;
    }
    seen(is_friendly(scene, viewer, object) || vision.visible(position))
}

/// Whether `round`, on `object`, is on course to hit `player`: its remaining
/// flight passes through `player`'s hit sphere with no cover in the way.
fn heads_for(
    scene: &Scene,
    vision: &TeamVision,
    object: GameObject,
    round: &ComponentProjectile,
    player: GameObject,
) -> bool {
    let start = ground(scene.world_transform(object).position);
    let target = ground(scene.world_transform(player).position);
    let reach = round.radius
        + scene
            .read_component::<ComponentProjectileTarget, _, _>(player, |target| target.hit_radius)
            .unwrap_or(1.0);
    let Some(direction) = ground(round.direction).try_normalize(f32::EPSILON) else {
        return false;
    };
    let flight = round.speed.max(0.0) * round.lifetime_remaining.max(0.0);
    let along = (target - start).dot(&direction).clamp(0.0, flight);
    let closest = start + direction * along;
    (closest - target).norm() <= reach && vision.clear_line(start, closest)
}

/// Hides or reveals `object` and the lights and particle systems in its
/// hierarchy, recording what it changed in `fog`.
fn conceal(scene: &mut Scene, fog: &mut FogState, object: GameObject, hide: bool) {
    let id = scene.uuid(object);
    if !hide {
        let Some(effects) = fog.hidden.remove(&id) else {
            return;
        };
        let _ = scene.write_component::<ComponentID, _>(object, |id| id.visible = true);
        for (part, effect) in effects {
            let Some(part) = scene.find(part) else {
                continue;
            };
            match effect {
                Effect::Light => scene.set_component_enabled::<ComponentPointLight>(part, true),
                Effect::Particles => {
                    scene.set_component_enabled::<ComponentParticleSystem>(part, true)
                }
            };
        }
        return;
    }
    if let std::collections::hash_map::Entry::Vacant(entry) = fog.hidden.entry(id) {
        if !scene.is_visible(object) {
            // Hidden by someone else; fog leaves it alone.
            return;
        }
        let _ = scene.write_component::<ComponentID, _>(object, |id| id.visible = false);
        entry.insert(Vec::new());
    }
    // Catch lights and emitters added since the object was hidden.
    let parts = scene.descendants(object).collect::<Vec<_>>();
    for part in parts {
        for effect in [Effect::Light, Effect::Particles] {
            let enabled = match effect {
                Effect::Light => {
                    scene
                        .read_component::<ComponentPointLight, _, _>(part, |_| ())
                        .is_some()
                        && scene.is_component_enabled::<ComponentPointLight>(part)
                }
                Effect::Particles => {
                    scene
                        .read_component::<ComponentParticleSystem, _, _>(part, |_| ())
                        .is_some()
                        && scene.is_component_enabled::<ComponentParticleSystem>(part)
                }
            };
            if !enabled {
                continue;
            }
            let disabled = match effect {
                Effect::Light => scene.set_component_enabled::<ComponentPointLight>(part, false),
                Effect::Particles => {
                    scene.set_component_enabled::<ComponentParticleSystem>(part, false)
                }
            };
            if disabled {
                let part = scene.uuid(part);
                if let Some(effects) = fog.hidden.get_mut(&id) {
                    effects.push((part, effect));
                }
            }
        }
    }
}

/// Applies `viewer`'s side's fog of war to `objects`: enemy units, rounds and
/// combat effects it cannot see are hidden, and the ones it now sees reappear.
fn apply_fog_to(
    scene: &mut Scene,
    viewer: GameObject,
    fog: &mut FogState,
    vision: &TeamVision,
    objects: &[GameObject],
) {
    let decisions = objects
        .iter()
        .filter_map(|&object| Some((object, sighting(scene, viewer, vision, object)?)))
        .collect::<Vec<_>>();
    for (object, sighting) in decisions {
        conceal(scene, fog, object, sighting == Sighting::Hidden);
    }
}

/// Applies fog of war for `viewer`'s side to the whole scene, and reveals
/// whatever fog hid that has since left the scene's managed objects.
pub(crate) fn apply_fog(scene: &mut Scene, viewer: GameObject, fog: &mut FogState) {
    let vision = TeamVision::in_scene(scene, viewer);
    let objects = scene.objects().collect::<Vec<_>>();
    apply_fog_to(scene, viewer, fog, &vision, &objects);
    fog.hidden.retain(|id, _| scene.find(*id).is_some());
}

/// Re-applies the local player's fog of war after `objects` spawned or moved
/// this frame, so they hide or reappear the same frame whatever order the
/// update hooks run in. A move by a tank on the local player's side changes
/// what the side sees and refreshes the whole scene; any other object only
/// refreshes itself and the effects that follow it. The local player is the
/// tank showing the tank HUD.
pub(crate) fn refresh(scene: &mut Scene, objects: &[GameObject]) {
    let Some(viewer) = scene.objects().find(|&object| {
        scene
            .read_component::<ComponentTankHud, _, _>(object, |_| ())
            .is_some()
    }) else {
        return;
    };
    let Some(mut fog) =
        scene.write_component::<ComponentTankHud, _>(viewer, |hud| std::mem::take(&mut hud.fog))
    else {
        return;
    };
    if objects
        .iter()
        .any(|&object| is_live_tank(scene, object) && is_friendly(scene, viewer, object))
    {
        apply_fog(scene, viewer, &mut fog);
    } else {
        let ids = objects
            .iter()
            .map(|&object| scene.uuid(object))
            .collect::<Vec<_>>();
        let mut affected = objects.to_vec();
        affected.extend(scene.objects().filter(|&object| {
            scene
                .read_component::<ComponentCombatEffectLifetime, _, _>(object, |effect| {
                    ids.contains(&effect.follow.id())
                })
                .unwrap_or(false)
        }));
        affected.dedup();
        let vision = TeamVision::in_scene(scene, viewer);
        apply_fog_to(scene, viewer, &mut fog, &vision, &affected);
    }
    let _ = scene.write_component::<ComponentTankHud, _>(viewer, |hud| hud.fog = fog);
}

#[cfg(test)]
mod tests;
