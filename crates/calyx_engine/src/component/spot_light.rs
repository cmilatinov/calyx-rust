use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::{GizmoIcon, Gizmos};
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use nalgebra_glm::{Vec3, Vec4};
use serde::{Deserialize, Serialize};

/// The widest cone half-angle, in degrees, a spot light accepts.
pub const SPOT_LIGHT_MAX_ANGLE: f32 = 89.0;

/// Local spot-light component: a point light limited to a cone along its
/// object's world +Z axis.
///
/// The light is full inside `inner_angle`, zero outside `outer_angle` and
/// falls off smoothly between them. Both angles are half-angles from the cone
/// axis, in degrees.
///
/// The scene's enabled state switches it on and off; see
/// [`Scene::set_component_enabled`].
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "0d4f0a8e-3a3c-4c47-9f0e-5b8f2d7c6a41"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Spot Light")]
#[serde(default)]
pub struct ComponentSpotLight {
    /// Light color.
    pub color: Color32,
    /// Scalar intensity multiplier.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub intensity: f32,
    /// Distance from the light past which it lights nothing.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub range: f32,
    /// Half-angle of the fully lit cone, in degrees. Never wider than
    /// `outer_angle`; a wider value is treated as `outer_angle`.
    #[reflect_attr(min = 0.0, max = 89.0, speed = 0.1)]
    pub inner_angle: f32,
    /// Half-angle of the cone past which the light is zero, in degrees.
    #[reflect_attr(min = 0.0, max = 89.0, speed = 0.1)]
    pub outer_angle: f32,
}

impl Default for ComponentSpotLight {
    fn default() -> Self {
        Self {
            color: Color32::WHITE,
            intensity: 1.0,
            range: 10.0,
            inner_angle: 20.0,
            outer_angle: 30.0,
        }
    }
}

impl ComponentSpotLight {
    /// The inner and outer half-angles in degrees, clamped so that
    /// `0 <= inner <= outer <= SPOT_LIGHT_MAX_ANGLE`.
    pub fn cone_angles(&self) -> (f32, f32) {
        let outer = finite_or_zero(self.outer_angle).clamp(0.0, SPOT_LIGHT_MAX_ANGLE);
        let inner = finite_or_zero(self.inner_angle).clamp(0.0, outer);
        (inner, outer)
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

impl Component for ComponentSpotLight {
    /// Light gathering skips a disabled spot light.
    fn can_disable(&self) -> bool {
        true
    }

    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        let transform = scene.world_transform(game_object);
        let direction = transform.transform_direction(&Vec3::z_axis());
        let (inner, outer) = self.cone_angles();
        let range = self.range.max(0.0);
        let color = Vec4::from(self.color.to_normalized_gamma_f32());
        gizmos.set_color(&color);
        gizmos.icon(GizmoIcon::Light, &transform.position, 0.75);
        gizmos.wire_cone(&transform.position, &direction, outer.to_radians(), range);
        gizmos.set_color(&Vec4::new(color.x, color.y, color.z, color.w * 0.35));
        gizmos.wire_cone(&transform.position, &direction, inner.to_radians(), range);
        gizmos.set_color(&color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cone_angles_keep_inner_within_outer_and_both_within_the_maximum() {
        let light = |inner_angle, outer_angle| ComponentSpotLight {
            inner_angle,
            outer_angle,
            ..Default::default()
        };
        assert_eq!(light(20.0, 30.0).cone_angles(), (20.0, 30.0));
        assert_eq!(light(40.0, 30.0).cone_angles(), (30.0, 30.0));
        assert_eq!(light(-5.0, 120.0).cone_angles(), (0.0, 89.0));
        assert_eq!(light(f32::NAN, f32::INFINITY).cone_angles(), (0.0, 0.0));
    }
}
