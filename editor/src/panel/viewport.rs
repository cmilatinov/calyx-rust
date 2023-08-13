use egui::Ui;

use engine::*;
use engine::egui::{Image, Margin, Sense};
use engine::egui_dock::TabStyle;
use engine::render::CameraLike;

use crate::EditorAppState;
use crate::panel::Panel;

#[derive(Default)]
pub struct PanelViewport;

impl Panel for PanelViewport {
    fn name() -> &'static str {
        "Viewport"
    }

    fn ui(&self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get_mut();
        let res = ui.add(Image::new(
            app_state.scene_renderer.as_ref()
                .unwrap()
                .read()
                .unwrap().scene_texture_handle.id(),
            egui::Vec2::new(ui.available_width(), ui.available_height()),
        ).sense(Sense::drag()));
        app_state.camera.update(ui, &res);
    }

    fn tab_style_override(&self, global_style: &TabStyle) -> Option<TabStyle> {
        Some(TabStyle {
            inner_margin: Margin {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 0.0,
            },
            ..*global_style
        })
    }
}