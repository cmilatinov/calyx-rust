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
mod tests;
