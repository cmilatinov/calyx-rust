use crate::scene::Scene;
use crate::ui::{
    render_commands, EguiUiBackend, PointerEvents, Theme, UiArena, UiFrame, UiInput, UiLength,
    UiNodeHandle, UiPoint, UiRect, UiRuntime, UiSize,
};

impl Scene {
    /// Builds the HUD of every [`ComponentHud`](crate::component::ComponentHud)
    /// in the scene into `ui`, stacked over `viewport`, skipping the components
    /// the scene holds disabled. Returns `None` when no component draws
    /// anything.
    pub fn build_hud(&self, ui: &mut UiArena, viewport: UiRect) -> Option<UiNodeHandle> {
        crate::profile_scope!("HUD build");
        let registry = self.registries().components.read();
        let layers: Vec<UiNodeHandle> = registry
            .components_with_hud()
            .flat_map(|(type_uuid, hook)| {
                let prototype = registry.component(type_uuid);
                crate::profile_scope!(
                    "HUD hook",
                    super::scene::hook_label(self.registries(), type_uuid)
                );
                // Only the objects holding the component are asked, not
                // every object of the scene.
                let objects = match prototype {
                    Some(prototype) => self.objects_holding(prototype.component_type_id()),
                    None => self.objects().collect(),
                };
                objects
                    .iter()
                    .filter(|&&game_object| self.runs_hooks(game_object, prototype))
                    .filter_map(|&game_object| hook.build_hud(self, game_object, ui, viewport))
                    .collect::<Vec<_>>()
            })
            .collect();
        if layers.is_empty() {
            return None;
        }
        Some(
            ui.stack()
                .id(ui, "scene-hud")
                .width(ui, UiLength::Px(viewport.width()))
                .height(ui, UiLength::Px(viewport.height()))
                .pointer_events(ui, PointerEvents::None)
                .children(ui, layers),
        )
    }
}

/// Retained runtime UI state for painting a scene's HUD over a game view.
///
/// The player and the editor's Game panel each own one and call
/// [`SceneHud::paint`] once per frame, after the scene update.
///
/// It keeps no [`UiArena`] between frames: HUD hooks fill the arena with
/// widget stores whose vtables live in the plugin that built them, and the
/// editor can unload that plugin between two frames.
#[derive(Default)]
pub struct SceneHud {
    runtime: UiRuntime,
    theme: Theme,
}

impl SceneHud {
    /// Lays out the scene's HUD for `viewport`, or returns `None` when the
    /// scene has none.
    pub fn frame(&mut self, scene: &Scene, viewport: UiRect, input: UiInput) -> Option<UiFrame> {
        let mut arena = UiArena::default();
        let root = scene.build_hud(&mut arena, viewport)?;
        Some(
            self.runtime
                .frame(&arena, root, viewport, input, &self.theme),
        )
    }

    /// Paints the scene's HUD with `painter` inside `rect`, in egui points.
    ///
    /// HUD images are not resolved yet; they paint with egui's default
    /// texture.
    pub fn paint(
        &mut self,
        scene: &Scene,
        painter: &egui::Painter,
        rect: egui::Rect,
        input: UiInput,
    ) {
        crate::profile_scope!("HUD");
        let viewport = UiRect::from_min_size(
            UiPoint::new(rect.min.x, rect.min.y),
            UiSize::new(rect.width(), rect.height()),
        );
        let Some(frame) = self.frame(scene, viewport, input) else {
            return;
        };
        let painter = painter.with_clip_rect(rect);
        render_commands(
            &mut EguiUiBackend::new(&painter),
            viewport,
            painter.ctx().pixels_per_point(),
            &frame.paint_commands,
            |_| egui::TextureId::default(),
        );
    }
}

#[cfg(test)]
mod tests;
