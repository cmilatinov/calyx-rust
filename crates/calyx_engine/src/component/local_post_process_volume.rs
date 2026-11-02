use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::math::{compose_transform, Transform};
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::Gizmos;
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::{vec3, vec4, Vec3};
use serde::{Deserialize, Serialize};

/// The region a [`ComponentLocalPostProcessVolume`] covers, in its game
/// object's space.
#[derive(Clone, Copy, Debug, PartialEq, TypeUuid, Serialize, Deserialize, Reflect)]
#[uuid = "2f0b8f55-3f3e-4c63-9d0e-6a9a3c1d7b41"]
pub enum LocalVolumeShape {
    /// A box centred on the game object, rotated and scaled with it.
    Box {
        /// Half the box's size on each local axis.
        half_extents: Vec3,
    },
    /// A sphere centred on the game object. Its radius scales with the
    /// largest axis of the object's world scale.
    Sphere {
        /// Sphere radius, in the game object's space.
        radius: f32,
    },
}

impl Default for LocalVolumeShape {
    fn default() -> Self {
        Self::Box {
            half_extents: vec3(5.0, 5.0, 5.0),
        }
    }
}

/// A post-processing volume that covers a region of the scene.
///
/// Its effects are the post-process effect components on the same game
/// object, the same ones a
/// [`ComponentGlobalPostProcessVolume`](super::ComponentGlobalPostProcessVolume)
/// uses. While the render camera is inside the shape they blend over the
/// global volume's settings by `weight`; the blend falls linearly to zero
/// over `blend_distance` outside it. See
/// [`PostProcessSettings::resolve`](crate::render::postprocess::PostProcessSettings::resolve).
#[derive(Clone, Copy, Debug, PartialEq, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "8d3c6a2e-5b71-4f0a-9c84-1e2f7a6b9d53"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Local Post Process Volume", category = "Post Processing")]
#[serde(default)]
pub struct ComponentLocalPostProcessVolume {
    /// The region the volume covers, in its game object's space.
    pub shape: LocalVolumeShape,
    /// Distance outside the shape, in world units, over which the volume
    /// fades out. Zero switches it on and off at the shape's surface.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub blend_distance: f32,
    /// How strongly the volume applies inside the shape, from 0 (not at all)
    /// to 1 (its effects replace those below it).
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub weight: f32,
    /// Local volumes apply in ascending priority, so the highest applies
    /// last. It orders local volumes only, never the global one.
    pub priority: i32,
}

impl Default for ComponentLocalPostProcessVolume {
    fn default() -> Self {
        Self {
            shape: LocalVolumeShape::default(),
            blend_distance: 2.0,
            weight: 1.0,
            priority: 0,
        }
    }
}

impl ComponentLocalPostProcessVolume {
    /// How strongly the volume applies at `point`, a world position, given
    /// its game object's world `transform`: `weight` inside the shape and on
    /// its surface, falling linearly to zero at `blend_distance` outside it,
    /// and zero beyond.
    pub fn blend_factor(&self, transform: &Transform, point: &Vec3) -> f32 {
        let weight = self.weight.clamp(0.0, 1.0);
        let distance = self.distance(transform, point);
        if distance <= 0.0 {
            return weight;
        }
        let blend_distance = self.blend_distance.max(0.0);
        if distance >= blend_distance {
            return 0.0;
        }
        weight * (1.0 - distance / blend_distance)
    }

    /// World distance from `point` to the shape, zero inside it.
    fn distance(&self, transform: &Transform, point: &Vec3) -> f32 {
        match self.shape {
            LocalVolumeShape::Box { half_extents } => {
                // A transform without shear keeps the box's axes orthogonal,
                // so the nearest point is the local point clamped to the box.
                let local = transform.inverse_transform_position(point);
                let half_extents = half_extents.abs();
                let nearest = vec3(
                    local.x.clamp(-half_extents.x, half_extents.x),
                    local.y.clamp(-half_extents.y, half_extents.y),
                    local.z.clamp(-half_extents.z, half_extents.z),
                );
                (transform.transform_position(&nearest) - point).magnitude()
            }
            LocalVolumeShape::Sphere { radius } => {
                let radius = sphere_world_radius(transform, radius);
                ((point - transform.position).magnitude() - radius).max(0.0)
            }
        }
    }
}

/// `radius` in world units: scaled by the largest axis of `transform`'s
/// scale.
fn sphere_world_radius(transform: &Transform, radius: f32) -> f32 {
    radius.abs() * transform.scale.abs().max()
}

/// The volume's shape is drawn in this colour, the outer edge of its blend
/// distance more faintly.
const SHAPE_COLOR: [f32; 4] = [0.35, 0.8, 1.0, 1.0];
const BLEND_EDGE_COLOR: [f32; 4] = [0.35, 0.8, 1.0, 0.4];

impl Component for ComponentLocalPostProcessVolume {
    /// Post-process blending skips a disabled local volume.
    fn can_disable(&self) -> bool {
        true
    }

    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        let transform = scene.world_transform(game_object);
        let blend_distance = self.blend_distance.max(0.0);
        let [r, g, b, a] = SHAPE_COLOR;
        let [er, eg, eb, ea] = BLEND_EDGE_COLOR;
        match self.shape {
            LocalVolumeShape::Box { half_extents } => {
                let size = transform.scale.abs().component_mul(&half_extents.abs()) * 2.0;
                gizmos.set_color(&vec4(r, g, b, a));
                gizmos.wire_cube_transform(compose_transform(
                    &transform.position,
                    &transform.rotation,
                    &size,
                ));
                if blend_distance > 0.0 {
                    // The blend region's outer edge is the box grown by the
                    // blend distance on every side, with rounded edges and
                    // corners; the grown box traces its faces.
                    let outer = size + Vec3::repeat(blend_distance * 2.0);
                    gizmos.set_color(&vec4(er, eg, eb, ea));
                    gizmos.wire_cube_transform(compose_transform(
                        &transform.position,
                        &transform.rotation,
                        &outer,
                    ));
                }
            }
            LocalVolumeShape::Sphere { radius } => {
                let radius = sphere_world_radius(&transform, radius);
                gizmos.set_color(&vec4(r, g, b, a));
                gizmos.wire_sphere(&transform.position, radius);
                if blend_distance > 0.0 {
                    gizmos.set_color(&vec4(er, eg, eb, ea));
                    gizmos.wire_sphere(&transform.position, radius + blend_distance);
                }
            }
        }
    }
}
