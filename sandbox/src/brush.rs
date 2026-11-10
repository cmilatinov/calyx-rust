//! Tall-grass brush: patches that hide the tanks inside them, as brush does
//! in a MOBA.
//!
//! A patch is a game object with a [`ComponentBrush`]. Its shape is the
//! colliders on it and its descendants, each a primitive volume; give them
//! [`crate::collision::BRUSH`] so they stop nothing, since brush is not
//! cover: tanks drive and rounds fly through it, and it blocks no sight. Its
//! grass is drawn by a [`calyx_engine::component::ComponentFoliage`] of
//! `tall_grass` clumps, whose material's grass shader sways it and bends it
//! by the scene's [`ComponentGrassField`]: every frame the brush hands the
//! field the tanks the local player sees as benders ([`grass_benders`]).
//!
//! The rules, which [`crate::vision::TeamVision`] applies for every side,
//! the local player's and the sentries' alike:
//!
//! - a tank inside a patch is hidden from every viewer outside that patch:
//!   fog hides its model, and the HUD its health bar, minimap blip and damage
//!   numbers;
//! - viewers inside the same patch see it, by the ordinary rules of range and
//!   cover;
//! - firing from a patch reveals the shooter to everyone for the patch's
//!   [`ComponentBrush::reveal_time`] ([`ComponentBrushReveal`]).
//!
//! The grass fades around the local player's tank while it is in brush, and
//! around the tanks the player sees in the same patch, to a translucent
//! green, truly alpha blended, and most of the way over the tank itself.
//! Only the nearest faded blade blends over each pixel, so stacked blades
//! never tint a tank green and it reads in its own colours. A tank fog hides
//! neither bends nor fades the grass, which would give it away.

use crate::combat::is_tank_combatant;
use crate::hud::ComponentTankHud;
use crate::spawn::ComponentRespawnState;
use crate::tank::ComponentHealth;
use calyx_engine::component::{
    ColliderShape, Component, ComponentCollider, ComponentEventContext, ComponentGrassField,
    ComponentID, ComponentRef, ComponentUpdate, GrassBender, Orientation, ReflectComponent,
    ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid, Uuid};
use legion::IntoQuery;
use nalgebra::{Isometry3, Point3, Translation3};
use nalgebra_glm::{vec3, Vec3};
use rapier3d::geometry::SharedShape;
use serde::{Deserialize, Serialize};

/// How far above its root a unit's body is tested against brush volumes, in
/// metres, so a tank standing on the ground is inside a volume resting on it.
const BODY_HEIGHT: f32 = 0.4;

/// One patch of tall grass. See the [module docs](self).
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "5b0f7c2e-93a4-4d61-8e0b-2c7a9f1d4e36"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Brush")]
#[serde(default)]
pub struct ComponentBrush {
    /// How long firing from this patch reveals the shooter, in seconds.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub reveal_time: f32,
}

impl Default for ComponentBrush {
    fn default() -> Self {
        Self { reveal_time: 1.5 }
    }
}

impl Component for ComponentBrush {
    /// A disabled patch hides nothing.
    fn can_disable(&self) -> bool {
        true
    }
}

impl ComponentUpdate for ComponentBrush {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext { scene, .. }: ComponentEventContext,
        _resources: &mut ResourceMap,
        _input: &Input,
    ) {
        // The first patch hands the grass its benders for every patch.
        let first = <(&ComponentID, &ComponentBrush)>::query()
            .iter(scene.world())
            .next()
            .map(|(id, _)| id.id);
        if first == Some(scene.uuid(this.game_object())) {
            show_benders(scene);
        }
    }
}

/// Reveals a shooter that fired from brush to every side for a while.
/// Firing from a patch adds it, or restarts it.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "c41e8a7d-2f69-4b53-a0d8-6e19b7c3f5a2"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Brush Reveal")]
#[serde(default)]
pub struct ComponentBrushReveal {
    /// How much longer the shooter stays revealed, in seconds.
    pub remaining: f32,
}

impl Default for ComponentBrushReveal {
    fn default() -> Self {
        Self { remaining: 0.0 }
    }
}

impl Component for ComponentBrushReveal {}

impl ComponentUpdate for ComponentBrushReveal {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext { scene, .. }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let dt = resources.time().delta_time();
        let _ = this.write(scene, |reveal| reveal.advance(dt));
    }
}

impl ComponentBrushReveal {
    /// Lets `dt` seconds of the reveal pass.
    pub fn advance(&mut self, dt: f32) {
        if dt.is_finite() {
            self.remaining = (self.remaining - dt.max(0.0)).max(0.0);
        }
    }

    pub fn active(&self) -> bool {
        self.remaining > 0.0
    }
}

/// One volume of a patch: a primitive shape placed in the world.
#[derive(Clone)]
struct Volume {
    shape: SharedShape,
    pose: Isometry3<f32>,
}

impl std::fmt::Debug for Volume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Volume").field("pose", &self.pose).finish()
    }
}

/// One patch's volumes.
#[derive(Clone, Debug)]
pub(crate) struct Patch {
    pub id: Uuid,
    pub reveal_time: f32,
    volumes: Vec<Volume>,
}

impl Patch {
    fn contains(&self, point: Vec3) -> bool {
        let point = Point3::from(point);
        self.volumes
            .iter()
            .any(|volume| volume.shape.contains_point(&volume.pose, &point))
    }
}

/// Every patch of brush in a scene, read once and asked where units stand.
#[derive(Clone, Debug, Default)]
pub(crate) struct BrushPatches {
    patches: Vec<Patch>,
}

impl BrushPatches {
    /// Reads the scene's enabled patches and their volumes now.
    pub fn read(scene: &Scene) -> Self {
        let roots = <(&ComponentID, &ComponentBrush)>::query()
            .iter(scene.world())
            .map(|(id, brush)| (id.id, brush.reveal_time))
            .collect::<Vec<_>>();
        let patches = roots
            .into_iter()
            .filter_map(|(id, reveal_time)| {
                let root = scene.find(id)?;
                if !scene.is_component_enabled::<ComponentBrush>(root) {
                    return None;
                }
                let volumes = scene
                    .descendants(root)
                    .filter(|&part| scene.is_component_enabled::<ComponentCollider>(part))
                    .filter_map(|part| {
                        let shape = scene
                            .read_component::<ComponentCollider, _, _>(part, |collider| {
                                collider.shape
                            })?;
                        Some(volume(shape, &scene.world_transform(part)))
                    })
                    .collect::<Vec<_>>();
                (!volumes.is_empty()).then_some(Patch {
                    id,
                    reveal_time,
                    volumes,
                })
            })
            .collect();
        Self { patches }
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    /// The patch a unit standing at `position` is in, if any. Where patches
    /// overlap, the first of them in scene order.
    pub fn patch_at(&self, position: Vec3) -> Option<&Patch> {
        let body = position + vec3(0.0, BODY_HEIGHT, 0.0);
        self.patches.iter().find(|patch| patch.contains(body))
    }

    /// The patch `object` stands in, if any.
    pub fn patch_of(&self, scene: &Scene, object: GameObject) -> Option<Uuid> {
        if self.is_empty() {
            return None;
        }
        self.patch_at(scene.world_transform(object).position)
            .map(|patch| patch.id)
    }
}

/// The volume a collider of `shape` makes at `transform`, as physics builds
/// it: placed by position and rotation, unscaled.
fn volume(shape: ColliderShape, transform: &Transform) -> Volume {
    let shape = match shape {
        ColliderShape::Sphere { radius } => SharedShape::ball(radius),
        ColliderShape::Capsule {
            orientation,
            height,
            radius,
        } => match orientation {
            Orientation::X => SharedShape::capsule_x(height, radius),
            Orientation::Y => SharedShape::capsule_y(height, radius),
            Orientation::Z => SharedShape::capsule_z(height, radius),
        },
        ColliderShape::Cuboid { half_extents } => {
            SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z)
        }
        ColliderShape::Cone { height, radius } => SharedShape::cone(height, radius),
    };
    Volume {
        shape,
        pose: Isometry3::from_parts(Translation3::from(transform.position), transform.rotation),
    }
}

/// Whether brush hides a unit standing in `patch` from viewers standing in
/// `viewer_patches`: it does when the unit is in a patch, is not revealed,
/// and none of the viewers shares its patch.
pub(crate) fn conceals(
    patch: Option<Uuid>,
    revealed: bool,
    mut viewer_patches: impl Iterator<Item = Option<Uuid>>,
) -> bool {
    match patch {
        Some(patch) => !revealed && !viewer_patches.any(|viewer| viewer == Some(patch)),
        None => false,
    }
}

/// Whether firing from brush has revealed `object` for now.
pub(crate) fn is_revealed(scene: &Scene, object: GameObject) -> bool {
    scene
        .read_component::<ComponentBrushReveal, _, _>(object, |reveal| reveal.active())
        .unwrap_or(false)
}

/// Whether brush hides `target` from a viewer standing at
/// `viewer_position`, such as a sentry, which belongs to no side.
pub(crate) fn hides_from(scene: &Scene, viewer_position: Vec3, target: GameObject) -> bool {
    let patches = BrushPatches::read(scene);
    if patches.is_empty() {
        return false;
    }
    let viewer = patches.patch_at(viewer_position).map(|patch| patch.id);
    conceals(
        patches.patch_of(scene, target),
        is_revealed(scene, target),
        std::iter::once(viewer),
    )
}

/// Reveals `shooter` for its patch's reveal time when it fires from brush.
pub(crate) fn reveal_shooter(scene: &mut Scene, shooter: GameObject) {
    let patches = BrushPatches::read(scene);
    let Some(reveal_time) = patches
        .patch_at(scene.world_transform(shooter).position)
        .map(|patch| patch.reveal_time.max(0.0))
    else {
        return;
    };
    let revealed = scene
        .write_component::<ComponentBrushReveal, _>(shooter, |reveal| {
            reveal.remaining = reveal.remaining.max(reveal_time);
        })
        .is_some();
    if !revealed {
        scene.add_component(
            shooter,
            ComponentBrushReveal {
                remaining: reveal_time,
            },
        );
    }
}

/// Whether `object` is a tank alive and in play.
fn is_live_tank(scene: &Scene, object: GameObject) -> bool {
    is_tank_combatant(scene, object)
        && scene
            .read_component::<ComponentRespawnState, _, _>(object, |state| state.alive)
            .unwrap_or(true)
        && !scene
            .read_component::<ComponentHealth, _, _>(object, |health| health.dead)
            .unwrap_or(false)
}

/// How far from a tank, in metres, the grass bends away from it.
pub(crate) const BEND_RADIUS: f32 = 1.8;
/// How far from a tank the player sees in brush, in metres, the grass fades
/// so the tank shows through it.
pub(crate) const REVEAL_RADIUS: f32 = 3.2;

/// What bends and fades the grass this frame, as the local player sees it:
/// every live tank the player's fog shows bends it, and the player's own
/// tank, while in a patch, and the tanks it sees in that same patch fade it.
/// A tank fog hides does neither. Without a local player, every live tank
/// bends the grass and none fades it.
pub(crate) fn grass_benders(scene: &Scene) -> Vec<GrassBender> {
    let patches = BrushPatches::read(scene);
    let player = <(&ComponentID, &ComponentTankHud)>::query()
        .iter(scene.world())
        .find_map(|(id, _)| scene.find(id.id))
        .filter(|&player| is_live_tank(scene, player));
    let player_patch = player.and_then(|player| patches.patch_of(scene, player));
    let tanks = <(&ComponentID, &crate::tank::ComponentTankController)>::query()
        .iter(scene.world())
        .map(|(id, _)| id.id)
        .collect::<Vec<_>>();
    let mut benders = tanks
        .into_iter()
        .filter_map(|id| scene.find(id))
        .filter(|&tank| is_live_tank(scene, tank) && scene.is_visible_in_hierarchy(tank))
        .map(|tank| {
            let reveals = player_patch.is_some()
                && (Some(tank) == player || patches.patch_of(scene, tank) == player_patch);
            let bender = GrassBender {
                position: scene.world_transform(tank).position,
                bend_radius: BEND_RADIUS,
                reveal_radius: if reveals { REVEAL_RADIUS } else { 0.0 },
            };
            // The player's own first, then the others that fade the grass,
            // so neither is ever past the shader's limit.
            let rank = if Some(tank) == player {
                0
            } else {
                1 + u8::from(!reveals)
            };
            (rank, bender)
        })
        .collect::<Vec<_>>();
    benders.sort_by_key(|(rank, _)| *rank);
    benders
        .into_iter()
        .take(calyx_engine::component::MAX_GRASS_BENDERS)
        .map(|(_, bender)| bender)
        .collect()
}

/// Hands [`grass_benders`] to every grass field in `scene`.
fn show_benders(scene: &mut Scene) {
    let fields = <(&ComponentID, &ComponentGrassField)>::query()
        .iter(scene.world())
        .map(|(id, _)| id.id)
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return;
    }
    let benders = grass_benders(scene);
    for id in fields {
        if let Some(field) = scene.find(id) {
            let _ = scene.write_component::<ComponentGrassField, _>(field, |field| {
                field.benders.clone_from(&benders);
            });
        }
    }
}

#[cfg(test)]
mod tests;
