use engine::context::GameContext;
use engine::scene::{GameObject, Scene};
use uuid::Uuid;

use crate::inspector::component_inspector::type_display_name;
use crate::scene_document::SceneDocument;
use crate::scene_history::SceneEditSnapshot;

/// The components the inspector adds to and removes from the selected
/// object in one frame, recorded as one undo step.
///
/// The scene is captured before the first change and the step is committed
/// once, so a frame that adds and removes components is undone at once.
/// While the scene cannot be edited, as in play mode, the changes still
/// apply to the simulation scene but nothing is recorded.
#[derive(Default)]
pub(super) struct StructuralEdit {
    before: Option<SceneEditSnapshot>,
    changed: bool,
}

impl StructuralEdit {
    const LABEL: &'static str = "Edit inspector components";

    /// Adds a default component of type `type_uuid` to `game_object`.
    pub fn add(&mut self, game: &mut GameContext, game_object: GameObject, type_uuid: Uuid) {
        self.capture(game);
        let scene = game.scenes.simulation_scene_mut();
        scene.bind_component_dyn(game_object, type_uuid);
        self.changed = true;
        log::info!(
            "Added component to game object: component={} type_uuid={type_uuid} object={}",
            component_name(game, type_uuid),
            object_label(game.scenes.simulation_scene(), game_object)
        );
    }

    /// Removes the component of type `type_uuid` from `game_object`.
    pub fn remove(&mut self, game: &mut GameContext, game_object: GameObject, type_uuid: Uuid) {
        let component_name = component_name(game, type_uuid);
        let object_label = object_label(game.scenes.simulation_scene(), game_object);
        self.capture(game);
        if game
            .scenes
            .simulation_scene_mut()
            .remove_component_dyn(game_object, type_uuid)
        {
            self.changed = true;
            log::info!(
                "Removed component from game object: component={component_name} type_uuid={type_uuid} object={object_label}"
            );
        }
    }

    /// Records the changes as one undo step, if there were any.
    pub fn commit(self, document: &mut SceneDocument, game: &GameContext) {
        if self.changed {
            document.commit_edit(game, Self::LABEL, self.before);
        }
    }

    fn capture(&mut self, game: &GameContext) {
        if self.before.is_none() {
            self.before = SceneDocument::begin_edit(game);
        }
    }
}

fn component_name(game: &GameContext, type_uuid: Uuid) -> String {
    type_display_name(&game.assets.registries.types.read(), type_uuid)
        .unwrap_or_else(|| type_uuid.to_string())
}

fn object_label(scene: &Scene, game_object: GameObject) -> String {
    format!("{} ({})", scene.name(game_object), scene.uuid(game_object))
}

#[cfg(test)]
mod tests {
    use super::StructuralEdit;
    use crate::scene_document::SceneDocument;
    use engine::component::{ComponentCamera, ComponentPointLight};
    use engine::context::GameContext;
    use engine::scene::GameObject;
    use engine::test_support::test_game_context;
    use engine::utils::TypeUuid;
    use std::path::Path;

    fn document() -> SceneDocument {
        SceneDocument::new(Path::new("structural-edit-test-project"))
    }

    /// An object with a camera, added outside the undo history.
    fn object_with_camera(game: &mut GameContext) -> GameObject {
        let scene = game.scenes.current_scene_mut();
        let object = scene.create(None, None);
        scene.add_component(object, ComponentCamera::default());
        object
    }

    fn has<T: engine::component::Component>(game: &GameContext, object: GameObject) -> bool {
        game.scenes
            .current_scene()
            .read_component::<T, _, _>(object, |_| ())
            .is_some()
    }

    #[test]
    fn additions_and_removals_are_one_undo_step() {
        let mut game = test_game_context();
        let mut document = document();
        let object = object_with_camera(&mut game);

        let mut edit = StructuralEdit::default();
        edit.add(&mut game, object, ComponentPointLight::type_uuid());
        edit.remove(&mut game, object, ComponentCamera::type_uuid());
        edit.commit(&mut document, &game);
        assert!(has::<ComponentPointLight>(&game, object));
        assert!(!has::<ComponentCamera>(&game, object));

        document.undo(&mut game);
        let object = game
            .scenes
            .current_scene()
            .objects()
            .find(|&object| has::<ComponentCamera>(&game, object))
            .expect("undo puts the camera back");
        assert!(!has::<ComponentPointLight>(&game, object));
        assert!(!document.can_undo(&game), "the edit was one step");
    }

    #[test]
    fn an_edit_without_changes_records_nothing() {
        let mut game = test_game_context();
        let mut document = document();
        let object = object_with_camera(&mut game);

        let mut edit = StructuralEdit::default();
        // An object keeps its ComponentID, so this removal changes nothing.
        edit.remove(
            &mut game,
            object,
            engine::component::ComponentID::type_uuid(),
        );
        edit.commit(&mut document, &game);

        assert!(!document.can_undo(&game));
        assert!(!document.is_dirty());
    }

    #[test]
    fn simulation_changes_are_not_recorded() {
        let mut game = test_game_context();
        let mut document = document();
        object_with_camera(&mut game);
        game.scenes.start_simulation();
        let object = game
            .scenes
            .simulation_scene()
            .objects()
            .find(|&object| {
                game.scenes
                    .simulation_scene()
                    .read_component::<ComponentCamera, _, _>(object, |_| ())
                    .is_some()
            })
            .expect("the simulation copies the object");

        let mut edit = StructuralEdit::default();
        edit.add(&mut game, object, ComponentPointLight::type_uuid());
        edit.commit(&mut document, &game);
        game.scenes.stop_simulation();

        assert!(!document.can_undo(&game));
    }
}
