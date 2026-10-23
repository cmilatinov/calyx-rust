use crate::scene::Scene;
use crate::ui::{
    render_commands, EguiUiBackend, PointerEvents, Theme, UiArena, UiFrame, UiInput, UiLength,
    UiNodeHandle, UiPoint, UiRect, UiRuntime, UiSize,
};

impl Scene {
    /// Builds the HUD of every [`ComponentHud`](crate::component::ComponentHud)
    /// in the scene into `ui`, stacked over `viewport`. Returns `None` when no
    /// component draws anything.
    pub fn build_hud(&self, ui: &mut UiArena, viewport: UiRect) -> Option<UiNodeHandle> {
        let registry = self.registries().components.read();
        let objects: Vec<_> = self.objects().collect();
        let layers: Vec<UiNodeHandle> = registry
            .components_with_hud()
            .flat_map(|(_, hud)| {
                objects
                    .iter()
                    .filter_map(|&game_object| hud.build_hud(self, game_object, ui, viewport))
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
#[derive(Default)]
pub struct SceneHud {
    runtime: UiRuntime,
    arena: UiArena,
    theme: Theme,
}

impl SceneHud {
    /// Lays out the scene's HUD for `viewport`, or returns `None` when the
    /// scene has none.
    pub fn frame(&mut self, scene: &Scene, viewport: UiRect, input: UiInput) -> Option<UiFrame> {
        self.arena.clear();
        let root = scene.build_hud(&mut self.arena, viewport)?;
        Some(
            self.runtime
                .frame(&self.arena, root, viewport, input, &self.theme),
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
mod tests {
    use super::SceneHud;
    use crate as engine;
    use crate::component::{Component, ComponentHud, ReflectComponent, ReflectComponentHud};
    use crate::reflect::{Reflect, ReflectDefault};
    use crate::scene::{GameObject, Scene};
    use crate::test_support::SceneBuilder;
    use crate::ui::{PaintCommand, UiArena, UiInput, UiNodeHandle, UiPoint, UiRect, UiSize};
    use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
    use serde::{Deserialize, Serialize};

    #[derive(Default, Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
    #[uuid = "4f0e8d55-6a7c-4b0e-9d61-2c7f6f1f3a10"]
    #[reflect(Default, TypeUuidDynamic, Component, ComponentHud)]
    #[repr(C)]
    struct TestHudCounter {
        value: u32,
        hidden: bool,
    }

    impl Component for TestHudCounter {}

    impl ComponentHud for TestHudCounter {
        fn build_hud(
            &self,
            scene: &Scene,
            game_object: GameObject,
            ui: &mut UiArena,
            _viewport: UiRect,
        ) -> Option<UiNodeHandle> {
            let counter =
                scene.read_component::<TestHudCounter, _, _>(game_object, |counter| *counter)?;
            (!counter.hidden).then(|| ui.text(format!("count {}", counter.value)))
        }
    }

    fn viewport() -> UiRect {
        UiRect::from_min_size(UiPoint::new(10.0, 20.0), UiSize::new(640.0, 360.0))
    }

    fn painted_text(scene: &Scene) -> Option<Vec<String>> {
        let frame = SceneHud::default().frame(scene, viewport(), UiInput::default())?;
        let mut text: Vec<String> = frame
            .paint_commands
            .iter()
            .filter_map(|command| match command {
                PaintCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        text.sort();
        Some(text)
    }

    #[test]
    fn scene_hud_paints_every_hud_component_from_its_live_value() {
        let mut builder = SceneBuilder::new(crate::test_support::test_registries());
        let first = builder.spawn("first");
        let second = builder.spawn("second");
        let hidden = builder.spawn("hidden");
        builder.add_component(
            first,
            TestHudCounter {
                value: 3,
                hidden: false,
            },
        );
        builder.add_component(
            second,
            TestHudCounter {
                value: 7,
                hidden: false,
            },
        );
        builder.add_component(
            hidden,
            TestHudCounter {
                value: 9,
                hidden: true,
            },
        );
        let mut scene = builder.finish();

        assert_eq!(
            painted_text(&scene),
            Some(vec!["count 3".to_string(), "count 7".to_string()])
        );

        let _ = scene.write_component::<TestHudCounter, _>(first, |counter| counter.value = 4);
        assert_eq!(
            painted_text(&scene),
            Some(vec!["count 4".to_string(), "count 7".to_string()])
        );
    }

    #[test]
    fn scene_without_hud_components_paints_nothing() {
        let mut builder = SceneBuilder::new(crate::test_support::test_registries());
        builder.spawn("plain");

        assert_eq!(painted_text(&builder.finish()), None);
    }
}
