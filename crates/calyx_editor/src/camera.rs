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

    /// Turns the camera to face `target`, a world point, upright. Returns
    /// `false`, and leaves it as it was, when `target` is its own position.
    pub fn look_at(&mut self, target: &Vec3) -> bool {
        let direction = target - self.transform.position;
        if direction.magnitude() <= f32::EPSILON {
            return false;
        }
        self.transform.rotation = UnitQuaternion::face_towards(&direction, &Vec3::y_axis());
        // Flying turns the camera from these angles, so they follow the new
        // rotation.
        self.rotation = self.transform.rotation.pitch_yaw();
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
        self.rotation.x =
            nalgebra::clamp(self.rotation.x, -89.0f32.to_radians(), 89.0f32.to_radians());
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
mod tests {
    use super::*;
    use egui::{Event, RawInput};

    #[test]
    fn editor_camera_map_flies_with_keys_and_modifiers() {
        let ctx = egui::Context::default();
        let mut state = InputState::new(EditorCamera::action_map());
        let key = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        ctx.begin_pass(RawInput {
            modifiers: Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
            events: vec![key(Key::W), key(Key::A)],
            ..Default::default()
        });
        let input = Input::from_ctx(&ctx, None, &mut state, true);

        assert_eq!(input.axis(MOVE_FORWARD), 1.0);
        assert_eq!(input.axis(MOVE_RIGHT), -1.0);
        assert_eq!(input.axis(MOVE_UP), -1.0);
        assert!(input.action(BOOST).pressed());
        let _ = ctx.end_pass();
    }

    #[test]
    fn look_at_faces_the_target_and_flying_turns_from_there() {
        let mut camera = EditorCamera::default();
        camera.transform.position = Vec3::new(0.0, 2.0, 0.0);
        assert!(camera.look_at(&Vec3::new(3.0, 2.0, 4.0)));
        let forward = camera.transform.forward();
        assert!(
            (forward - Vec3::new(0.6, 0.0, 0.8)).magnitude() < 1e-5,
            "{forward}"
        );
        // The angles flying starts from match the new rotation.
        let rebuilt = UnitQuaternion::from_euler_angles(camera.rotation.x, camera.rotation.y, 0.0);
        assert!(rebuilt.angle_to(&camera.transform.rotation) < 1e-4);

        let before = camera.transform;
        assert!(!camera.look_at(&Vec3::new(0.0, 2.0, 0.0)));
        assert_eq!(camera.transform.rotation, before.rotation);
    }
}
