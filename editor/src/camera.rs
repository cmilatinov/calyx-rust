use engine::core::Time;
use engine::egui::{Key, PointerButton, Response};
use engine::glm::Vec3;
use engine::math::Transform;
use engine::render::{Camera, CameraLike};
use engine::{egui, glm};

pub struct EditorCamera {
    pub camera: Camera,
    pub transform: Transform,
}

impl Default for EditorCamera {
    fn default() -> Self {
        let transform = Transform::from_components(
            Vec3::new(10.0, 10.0, 10.0),
            Vec3::new(40.0f32.to_radians(), 225.0f32.to_radians(), 0.0),
            Vec3::new(1.0, 1.0, 1.0),
        );
        Self {
            camera: Default::default(),
            transform,
        }
    }
}

impl CameraLike for EditorCamera {
    fn update(&mut self, ui: &mut egui::Ui, res: &Response) {
        const TRANSLATION_SPEED: f32 = 10.0;
        const ROTATION_SPEED: f32 = 0.5;
        const GIGA_SPEED_FACTOR: f32 = 5.0;

        if !res.dragged_by(PointerButton::Secondary) {
            return;
        }

        let drag = res.drag_delta();
        self.transform.rotate(
            &glm::vec3(drag.y, drag.x, 0.0).scale(Time::static_delta_time() * ROTATION_SPEED),
        );

        let movement = ui.input(|i| {
            let forward = (i.key_down(Key::W) as u8 as f32) - (i.key_down(Key::S) as u8 as f32);
            let lateral = (i.key_down(Key::D) as u8 as f32) - (i.key_down(Key::A) as u8 as f32);
            let vertical = (i.key_down(Key::Space) as u8 as f32) - (i.modifiers.shift as u8 as f32);
            (self.transform.forward().scale(forward)
                + self.transform.right().scale(lateral)
                + Vec3::y_axis().scale(vertical))
            .scale(if i.modifiers.ctrl {
                GIGA_SPEED_FACTOR
            } else {
                1.0
            })
            .scale(Time::static_delta_time() * TRANSLATION_SPEED)
        });

        self.transform.translate(&movement);
    }
}
