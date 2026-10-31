use calyx_engine::context::GameContext;
use calyx_engine::scene::{GameObject, Scene};
use uuid::Uuid;

use crate::inspector::component_inspector::type_display_name;
use crate::scene_edit::SceneEdit;

/// The label of the undo step for the components the inspector adds to and
/// removes from the selected object in one frame.
pub(super) const LABEL: &str = "Edit inspector components";

/// Adds a default component of type `type_uuid` to `game_object` through
/// `edit`.
pub(super) fn add(
    edit: &mut SceneEdit,
    game: &mut GameContext,
    game_object: GameObject,
    type_uuid: Uuid,
) {
    edit.scene_mut(game)
        .bind_component_dyn(game_object, type_uuid);
    log::info!(
        "Added component to game object: component={} type_uuid={type_uuid} object={}",
        component_name(game, type_uuid),
        object_label(game.scenes.simulation_scene(), game_object)
    );
}

/// Removes the component of type `type_uuid` from `game_object` through
/// `edit`.
pub(super) fn remove(
    edit: &mut SceneEdit,
    game: &mut GameContext,
    game_object: GameObject,
    type_uuid: Uuid,
) {
    let component_name = component_name(game, type_uuid);
    let object_label = object_label(game.scenes.simulation_scene(), game_object);
    if edit
        .scene_mut(game)
        .remove_component_dyn(game_object, type_uuid)
    {
        log::info!(
            "Removed component from game object: component={component_name} type_uuid={type_uuid} object={object_label}"
        );
    }
}

/// The label of the undo step that switches the component of type
/// `type_uuid` to `enabled`.
pub(super) fn enabled_label(game: &GameContext, type_uuid: Uuid, enabled: bool) -> String {
    let verb = if enabled { "Enable" } else { "Disable" };
    format!("{verb} {}", component_name(game, type_uuid))
}

/// Enables or disables `game_object`'s component of type `type_uuid` through
/// `edit`.
pub(super) fn set_enabled(
    edit: &mut SceneEdit,
    game: &mut GameContext,
    game_object: GameObject,
    type_uuid: Uuid,
    enabled: bool,
) {
    let component_name = component_name(game, type_uuid);
    let object_label = object_label(game.scenes.simulation_scene(), game_object);
    if edit
        .scene_mut(game)
        .set_component_enabled_dyn(game_object, type_uuid, enabled)
    {
        log::info!(
            "Set component enabled: component={component_name} type_uuid={type_uuid} object={object_label} enabled={enabled}"
        );
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
    use super::{add, enabled_label, remove, set_enabled, LABEL};
    use crate::scene_document::SceneDocument;
    use crate::scene_edit::SceneEdit;
    use calyx_engine::component::{ComponentCamera, ComponentPointLight};
    use calyx_engine::context::GameContext;
    use calyx_engine::scene::GameObject;
    use calyx_engine::test_support::test_game_context;
    use calyx_engine::utils::TypeUuid;
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

    fn has<T: calyx_engine::component::Component>(game: &GameContext, object: GameObject) -> bool {
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

        let mut edit = SceneEdit::begin(LABEL);
        add(
            &mut edit,
            &mut game,
            object,
            ComponentPointLight::type_uuid(),
        );
        remove(&mut edit, &mut game, object, ComponentCamera::type_uuid());
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

    fn camera_enabled(game: &GameContext) -> bool {
        let scene = game.scenes.current_scene();
        let object = scene
            .objects()
            .find(|&object| has::<ComponentCamera>(game, object))
            .expect("the object keeps its camera");
        scene.is_component_enabled::<ComponentCamera>(object)
    }

    #[test]
    fn a_toggle_is_one_undo_step() {
        let mut game = test_game_context();
        let mut document = document();
        let object = object_with_camera(&mut game);

        let label = enabled_label(&game, ComponentCamera::type_uuid(), false);
        assert_eq!(label, "Disable Camera");
        let mut edit = SceneEdit::begin(label);
        set_enabled(
            &mut edit,
            &mut game,
            object,
            ComponentCamera::type_uuid(),
            false,
        );
        edit.commit(&mut document, &game);
        assert!(!camera_enabled(&game));
        assert!(document.is_dirty());

        document.undo(&mut game);
        assert!(camera_enabled(&game), "undo enables the camera again");
        assert!(!document.can_undo(&game), "the toggle was one step");
        document.redo(&mut game);
        assert!(!camera_enabled(&game), "redo disables it again");
    }

    #[test]
    fn an_edit_without_changes_records_nothing() {
        let mut game = test_game_context();
        let mut document = document();
        let object = object_with_camera(&mut game);

        let mut edit = SceneEdit::begin(LABEL);
        // An object keeps its ComponentID, so this removal changes nothing.
        remove(
            &mut edit,
            &mut game,
            object,
            calyx_engine::component::ComponentID::type_uuid(),
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

        let mut edit = SceneEdit::begin(LABEL);
        add(
            &mut edit,
            &mut game,
            object,
            ComponentPointLight::type_uuid(),
        );
        edit.commit(&mut document, &game);
        game.scenes.stop_simulation();

        assert!(!document.can_undo(&game));
    }
}
