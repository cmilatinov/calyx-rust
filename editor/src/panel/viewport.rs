use egui::Ui;

use engine::*;
use engine::core::Time;
use engine::egui::{Image, Key, Margin, PointerButton, Sense};
use engine::egui_dock::TabStyle;
use engine::glm::{vec3, Vec3};

use crate::EditorAppState;
use crate::panel::Panel;

#[derive(Default)]
pub struct PanelViewport;

impl Panel for PanelViewport {
    fn name() -> &'static str {
        "Viewport"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get();

        let res = ui.add(Image::new(
            app_state.scene_renderer.as_ref()
                .unwrap()
                .read()
                .unwrap().scene_texture_handle.id(),
            egui::Vec2::new(ui.available_width(), ui.available_height()),
        ).sense(Sense::drag()));
        if !res.dragged_by(PointerButton::Secondary) { return; }

        // TODO: Put all this logic inside the camera
        const TRANSLATION_SPEED: f32 = 10.0;
        const ROTATION_SPEED: f32 = 0.5;
        const GIGA_SPEED_FACTOR: f32 = 5.0;

        let drag = res.drag_delta();
        app_state.camera.transform.rotate(
            &vec3(drag.y, drag.x, 0.0).scale(
                Time::static_delta_time() *
                    ROTATION_SPEED
            )
        );

        let movement = ui.input(|i| {
            let forward = (i.key_down(Key::W) as u8 as f32) - (i.key_down(Key::S) as u8 as f32);
            let lateral = (i.key_down(Key::D) as u8 as f32) - (i.key_down(Key::A) as u8 as f32);
            let vertical = (i.key_down(Key::Space) as u8 as f32) - (i.modifiers.shift as u8 as f32);
            (app_state.camera.transform.forward().scale(forward) +
                app_state.camera.transform.right().scale(lateral) +
                Vec3::y_axis().scale(vertical))
                .scale(if i.modifiers.ctrl { GIGA_SPEED_FACTOR } else { 1.0 })
                .scale(Time::static_delta_time() * TRANSLATION_SPEED)
        });
        app_state.camera.transform.translate(&movement);
    }

    fn tab_style_override(&self, _global_style: &TabStyle) -> Option<TabStyle> {
        Some(TabStyle {
            inner_margin: Margin {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 0.0,
            },
            ..*_global_style
        })
    }
}