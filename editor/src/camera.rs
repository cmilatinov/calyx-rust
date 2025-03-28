use engine::core::Time;
use engine::egui::{Key, PointerButton};
use engine::ext::nalgebra::UnitQuaternionExt;
use engine::glm::{Vec2, Vec3};
use engine::input::Input;
use engine::math::Transform;
use engine::nalgebra;
use engine::nalgebra::UnitQuaternion;
use engine::render::{Camera, CameraLike};

pub struct EditorCamera {
    pub camera: Camera,
    pub transform: Transform,
    rotation: Vec2,
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
            rotation: transform.rotation.pitch_yaw(),
        }
    }
}

impl CameraLike for EditorCamera {
    fn update(&mut self, time: &Time, input: &Input) {
        const TRANSLATION_SPEED: f32 = 10.0;
        const ROTATION_SPEED: f32 = 0.5;
        const GIGA_SPEED_FACTOR: f32 = 5.0;

        if let Some(res) = input.res() {
            if !res.dragged_by(PointerButton::Secondary) {
                return;
            }

            let drag = res.drag_delta();
            let delta = Vec2::new(drag.y, drag.x).scale(time.static_delta_time() * ROTATION_SPEED);
            self.rotation += delta;
            self.rotation.x =
                nalgebra::clamp(self.rotation.x, -89.0f32.to_radians(), 89.0f32.to_radians());
            self.transform.rotation =
                UnitQuaternion::from_euler_angles(self.rotation.x, self.rotation.y, 0.0);
            self.transform.update_matrix();

            let movement = input.input(|i| {
                let forward = (i.key_down(Key::W) as u8 as f32) - (i.key_down(Key::S) as u8 as f32);
                let lateral = (i.key_down(Key::D) as u8 as f32) - (i.key_down(Key::A) as u8 as f32);
                let vertical =
                    (i.key_down(Key::Space) as u8 as f32) - (i.modifiers.shift as u8 as f32);
                (self.transform.forward().scale(forward)
                    + self.transform.right().scale(lateral)
                    + Vec3::y_axis().scale(vertical))
                .scale(if i.modifiers.ctrl {
                    GIGA_SPEED_FACTOR
                } else {
                    1.0
                })
                .scale(time.static_delta_time() * TRANSLATION_SPEED)
            });

            self.transform.translate(&movement);
        }
    }
}
