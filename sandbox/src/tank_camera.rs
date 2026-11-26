//! The gameplay camera rig: the tank's camera framing, aim-ahead and zoom.

use calyx_engine::component::{Component, ComponentCamera, ReflectComponent};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::render::Ray;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec3};
use serde::{Deserialize, Serialize};

/// Frames the player's tank from above: the camera sits `height` up and
/// `back` behind its focus, looking down at it, leads towards the point the
/// player aims at, and zooms between a near and a far framing with the
/// scroll wheel. The tank controller whose `camera` names this object moves
/// it every frame, before it aims through it.
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "7463cb88-5789-490e-a46d-50e30a6d61d9"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Tank Camera")]
#[serde(default)]
pub struct ComponentTankCamera {
    /// Height above the focus at a zoom of 1, in metres.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub height: f32,
    /// Distance behind the focus at a zoom of 1, in metres. The camera
    /// looks down at the focus, so its pitch is `atan(height / back)`.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub back: f32,
    /// Zoom the camera starts at: a scale on `height` and `back`.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub zoom: f32,
    /// Nearest zoom the scroll wheel reaches.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub min_zoom: f32,
    /// Farthest zoom the scroll wheel reaches.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub max_zoom: f32,
    /// Zoom change per point of scroll: scrolling up zooms in.
    #[reflect_attr(min = 0.0, speed = 0.0001)]
    pub zoom_sensitivity: f32,
    /// Time constant the zoom eases towards its target with, in seconds;
    /// 0 snaps.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub zoom_smoothing: f32,
    /// Share of the distance from the tank to the aim point, beyond
    /// `aim_ahead_dead_zone`, that the focus leads by.
    #[reflect_attr(min = 0.0, max = 0.9, speed = 0.01)]
    pub aim_ahead: f32,
    /// Farthest the focus leads from the tank, in metres.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub aim_ahead_max: f32,
    /// Distance from the tank within which aiming does not move the focus,
    /// in metres: aiming near the tank recentres it.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub aim_ahead_dead_zone: f32,
    /// Time constant the lead eases towards its target with, in seconds;
    /// 0 snaps.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub aim_ahead_smoothing: f32,
    /// Where the rig is this frame; set on its first update.
    #[serde(skip)]
    #[reflect_skip]
    state: Option<RigState>,
}

/// The rig's eased values.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RigState {
    /// Ground offset of the focus from the tank.
    lead: Vec3,
    /// Zoom the scroll wheel asks for.
    target_zoom: f32,
    /// Zoom the camera shows.
    zoom: f32,
}

impl Default for ComponentTankCamera {
    fn default() -> Self {
        Self {
            height: 34.0,
            back: 13.7,
            zoom: 1.0,
            min_zoom: 0.6,
            max_zoom: 1.0,
            zoom_sensitivity: 0.002,
            zoom_smoothing: 0.12,
            aim_ahead: 0.25,
            aim_ahead_max: 6.0,
            aim_ahead_dead_zone: 4.0,
            aim_ahead_smoothing: 0.35,
            state: None,
        }
    }
}

impl Component for ComponentTankCamera {}

impl ComponentTankCamera {
    /// The ground offset the focus leads the tank at `tank` by, while the
    /// player aims at `aim`.
    pub fn aim_ahead_target(&self, tank: Vec3, aim: Vec3) -> Vec3 {
        let ground = vec3(aim.x - tank.x, 0.0, aim.z - tank.z);
        let distance = ground.magnitude();
        let past_dead_zone = distance - self.aim_ahead_dead_zone.max(0.0);
        if past_dead_zone <= 0.0 || distance <= f32::EPSILON {
            return Vec3::zeros();
        }
        let lead =
            (past_dead_zone * self.aim_ahead.clamp(0.0, 0.9)).min(self.aim_ahead_max.max(0.0));
        ground * (lead / distance)
    }

    /// The zoom the scroll wheel asks for after `scroll` points of scroll
    /// from `zoom`, within the zoom limits.
    pub fn zoom_after_scroll(&self, zoom: f32, scroll: f32) -> f32 {
        self.clamp_zoom(zoom - scroll * self.zoom_sensitivity)
    }

    /// `zoom` within the zoom limits.
    fn clamp_zoom(&self, zoom: f32) -> f32 {
        let near = self.min_zoom.max(0.0);
        zoom.clamp(near, self.max_zoom.max(near))
    }

    /// The camera's transform when it frames `focus` at `zoom`, turned
    /// `yaw` radians about the vertical axis.
    pub fn camera_transform(&self, focus: Vec3, yaw: f32, zoom: f32) -> Transform {
        let turn = UnitQuaternion::from_euler_angles(0.0, yaw, 0.0);
        let offset = turn * vec3(0.0, self.height, -self.back) * zoom;
        let pitch = self.height.atan2(self.back);
        Transform::from_components(
            focus + offset,
            UnitQuaternion::from_euler_angles(pitch, yaw, 0.0),
            vec3(1.0, 1.0, 1.0),
        )
    }
}

/// Eases `current` towards `target` over `dt` seconds with the time
/// constant `smoothing`: the same total for any split of the time into
/// frames.
pub fn ease<T>(current: T, target: T, smoothing: f32, dt: f32) -> T
where
    T: std::ops::Sub<Output = T>
        + std::ops::Add<Output = T>
        + std::ops::Mul<f32, Output = T>
        + Copy,
{
    if smoothing <= 0.0 {
        return target;
    }
    let share = 1.0 - (-dt.max(0.0) / smoothing).exp();
    current + (target - current) * share
}

/// Moves `camera_object`'s rig to frame the tank at `tank`, easing its lead
/// and zoom over `dt`. Returns false, changing nothing, when the object has
/// no [`ComponentTankCamera`].
pub(crate) fn update_tank_camera(
    scene: &mut Scene,
    camera_object: GameObject,
    input: &Input,
    dt: f32,
    tank: Vec3,
) -> bool {
    let Some(rig) = scene.read_component::<ComponentTankCamera, _, _>(camera_object, Clone::clone)
    else {
        return false;
    };
    let mut state = rig.state.unwrap_or_else(|| {
        let zoom = rig.clamp_zoom(rig.zoom);
        RigState {
            lead: Vec3::zeros(),
            target_zoom: zoom,
            zoom,
        }
    });

    // Keep the yaw the camera was placed with.
    let forward = scene.world_transform(camera_object).forward();
    let yaw = if forward.x.abs() + forward.z.abs() > f32::EPSILON {
        forward.x.atan2(forward.z)
    } else {
        0.0
    };

    let scroll = if input.action("scoreboard").pressed() {
        0.0
    } else {
        input.scroll_delta().y
    };
    state.target_zoom = rig.zoom_after_scroll(state.target_zoom, scroll);
    state.zoom = rig.clamp_zoom(ease(state.zoom, state.target_zoom, rig.zoom_smoothing, dt));

    // Aim through the camera as the lead left it, then lead from there: the
    // lead settles where it is `aim_ahead` of the way to the aim point.
    let placed = rig.camera_transform(tank + state.lead, yaw, state.zoom);
    let target_lead = cursor_ground_point(scene, input, camera_object, &placed, tank.y)
        .map(|aim| rig.aim_ahead_target(tank, aim))
        .unwrap_or_else(Vec3::zeros);
    state.lead = ease(state.lead, target_lead, rig.aim_ahead_smoothing, dt);

    let transform = rig.camera_transform(tank + state.lead, yaw, state.zoom);
    scene.set_world_transform(camera_object, transform.matrix());
    let _ = scene.write_component::<ComponentTankCamera, _>(camera_object, |rig| {
        rig.state = Some(state);
    });
    true
}

/// Where the cursor's ray through `camera_object`, placed at
/// `camera_transform`, meets the horizontal plane at `ground_y`.
pub(crate) fn cursor_ground_point(
    scene: &Scene,
    input: &Input,
    camera_object: GameObject,
    camera_transform: &Transform,
    ground_y: f32,
) -> Option<Vec3> {
    ground_intersection(
        &cursor_ray(scene, input, camera_object, camera_transform)?,
        ground_y,
    )
}

/// The ray from `camera_object`, placed at `camera_transform`, through the
/// cursor.
pub(crate) fn cursor_ray(
    scene: &Scene,
    input: &Input,
    camera_object: GameObject,
    camera_transform: &Transform,
) -> Option<Ray> {
    let rect = input.surface_rect()?;
    let cursor = input.pointer_pos()?;
    let aspect = rect.width() / rect.height().max(f32::EPSILON);
    let camera = scene
        .read_component::<ComponentCamera, _, _>(camera_object, |component| {
            component.camera(aspect)
        })
        .unwrap_or_else(|| ComponentCamera::default().camera(aspect));
    camera.screen_ray(
        camera_transform,
        vec2(cursor.x - rect.left(), cursor.y - rect.top()),
        vec2(rect.width(), rect.height()),
    )
}

/// Where `ray` meets the horizontal plane at `ground_y`, ahead of its
/// origin.
pub(crate) fn ground_intersection(ray: &Ray, ground_y: f32) -> Option<Vec3> {
    if ray.direction.y.abs() <= f32::EPSILON {
        return None;
    }

    let t = (ground_y - ray.origin.y) / ray.direction.y;
    (t >= 0.0).then_some(ray.origin + ray.direction * t)
}

#[cfg(test)]
mod tests;
