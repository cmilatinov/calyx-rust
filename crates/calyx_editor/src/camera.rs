use calyx_engine::core::Time;
use calyx_engine::ext::nalgebra::UnitQuaternionExt;
use calyx_engine::input::{ActionMap, Input, InputBinding, InputState};
use calyx_engine::math::Transform;
use calyx_engine::render::{Camera, CameraLike};
use egui::{Key, Modifiers, PointerButton};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{Vec2, Vec3};

pub struct EditorCamera {
    pub camera: Camera,
    pub transform: Transform,
    /// Input state interpreting viewport input through the editor's fly-camera map.
    pub input: InputState,
    rotation: Vec2,
}

const MOVE_FORWARD: &str = "editor_camera_forward";
const MOVE_RIGHT: &str = "editor_camera_right";
const MOVE_UP: &str = "editor_camera_up";
const BOOST: &str = "editor_camera_boost";
/// Steepest pitch, in degrees, the camera looks up or down.
const MAX_PITCH: f32 = 89.0;

impl EditorCamera {
    /// Action map for flying the editor camera, independent of the project's map.
    pub fn action_map() -> ActionMap {
        let mut map = ActionMap::new();
        map.bind_axis(
            MOVE_FORWARD,
            InputBinding::Key(Key::W),
            InputBinding::Key(Key::S),
        );
        map.bind_axis(
            MOVE_RIGHT,
            InputBinding::Key(Key::D),
            InputBinding::Key(Key::A),
        );
        map.bind_axis(
            MOVE_UP,
            InputBinding::Key(Key::Space),
            InputBinding::Modifier(Modifiers::SHIFT),
        );
        map.bind_action(BOOST, InputBinding::Modifier(Modifiers::CTRL));
        map
    }

    /// Turns the camera to face `target`, a world point, upright. Its pitch
    /// stays within the ±89° flying allows, so a target straight above or
    /// below is faced as nearly as that. Returns `false`, and leaves the
    /// camera as it was, when `target` is its own position.
    pub fn look_at(&mut self, target: &Vec3) -> bool {
        let direction = target - self.transform.position;
        if direction.magnitude() <= f32::EPSILON {
            return false;
        }
        let direction = direction.normalize();
        // The same angles flying turns the camera by: the rotation is
        // `from_euler_angles(pitch, yaw, 0)`, whose forward axis is
        // `(cos(pitch) sin(yaw), -sin(pitch), cos(pitch) cos(yaw))`.
        let pitch = (-direction.y)
            .asin()
            .clamp(-MAX_PITCH.to_radians(), MAX_PITCH.to_radians());
        let yaw = direction.x.atan2(direction.z);
        self.rotation = Vec2::new(pitch, yaw);
        self.transform.rotation = UnitQuaternion::from_euler_angles(pitch, yaw, 0.0);
        true
    }
}

impl Default for EditorCamera {
    fn default() -> Self {
        let transform = Transform::from_components(
            Vec3::new(10.0, 10.0, -10.0),
            UnitQuaternion::face_towards(&Vec3::new(-1.0, -1.0, 1.0), &Vec3::y_axis()),
            Vec3::new(1.0, 1.0, 1.0),
        );
        Self {
            camera: Default::default(),
            transform,
            input: InputState::new(Self::action_map()),
            rotation: transform.rotation.pitch_yaw(),
        }
    }
}

impl CameraLike for EditorCamera {
    fn update(&mut self, time: &Time, input: &Input) {
        const TRANSLATION_SPEED: f32 = 10.0;
        const ROTATION_SPEED: f32 = 0.5;
        const GIGA_SPEED_FACTOR: f32 = 5.0;

        let Some(drag) = input.drag_delta(PointerButton::Secondary) else {
            return;
        };

        let delta = Vec2::new(drag.y, drag.x).scale(time.static_delta_time() * ROTATION_SPEED);
        self.rotation += delta;
        self.rotation.x = nalgebra::clamp(
            self.rotation.x,
            -MAX_PITCH.to_radians(),
            MAX_PITCH.to_radians(),
        );
        self.transform.rotation =
            UnitQuaternion::from_euler_angles(self.rotation.x, self.rotation.y, 0.0);

        let speed = if input.action(BOOST).pressed() {
            GIGA_SPEED_FACTOR
        } else {
            1.0
        };
        let movement = (self.transform.forward().scale(input.axis(MOVE_FORWARD))
            + self.transform.right().scale(input.axis(MOVE_RIGHT))
            + Vec3::y_axis().scale(input.axis(MOVE_UP)))
        .scale(speed * time.static_delta_time() * TRANSLATION_SPEED);

        self.transform.translate(&movement);
    }
}

#[cfg(test)]
mod tests;
