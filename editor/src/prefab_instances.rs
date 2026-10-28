//! Prefab instances in the open scene: instantiating a prefab asset with a
//! link back to it, and updating the instances from their prefab assets.
//!
//! An instance changes after its prefab only through
//! [`update_prefab_instances`], an explicit editor action. The rules it
//! follows for overrides and for objects the prefab added or removed are in
//! [`engine::scene::Scene::update_prefab_instances`]'s module docs: in short,
//! an instance keeps its root transform, its object ids and every component it
//! overrides.

use engine::context::GameContext;
use engine::scene::{dropped_components_report, Prefab, PrefabData, PrefabUpdate};
use engine::utils::TypeUuid;
use uuid::Uuid;

use crate::prefab_mode::CLOSE_PREFAB_FIRST;
use crate::scene_edit::SceneEdit;
use crate::EditorAppState;

/// Instantiates the prefab asset `asset_id` at the scene root through `edit`,
/// linked to the prefab so a later update reaches it. Returns the instance's
/// root object id.
pub(crate) fn instantiate_prefab(
    edit: &mut SceneEdit,
    game: &mut GameContext,
    asset_id: Uuid,
) -> Result<Uuid, String> {
    let prefab = load_prefab(game, asset_id)?;
    let (game_object, dropped) = edit
        .scene_mut(game)
        .instantiate_linked_prefab(asset_id, &prefab.prefab, None)
        .map_err(|err| format!("failed to instantiate prefab {}: {err}", prefab.label))?;
    if let Some(report) =
        dropped_components_report(&format!("the prefab {}", prefab.label), &dropped)
    {
        log::warn!("{report}");
    }
    let scene = game.scenes.simulation_scene();
    let root_id = scene.uuid(game_object);
    log::info!(
        "Instantiated prefab: prefab={} asset_id={asset_id} root_object={} ({root_id})",
        prefab.label,
        scene.name(game_object),
    );
    Ok(root_id)
}

/// Updates the open scene's instances of the prefab asset `prefab`, or of
/// every prefab the scene holds instances of when `prefab` is `None`, from
/// the prefab assets as last saved, through `edit`. Every prefab loads before
/// any instance changes, so a prefab that fails to load changes nothing.
pub(crate) fn update_prefab_instances(
    edit: &mut SceneEdit,
    game: &mut GameContext,
    prefab: Option<Uuid>,
) -> Result<PrefabUpdate, String> {
    let prefab_ids = match prefab {
        Some(asset_id) => vec![asset_id],
        None => edit.scene(game).linked_prefabs().into_iter().collect(),
    };
    let mut prefabs = Vec::new();
    for asset_id in prefab_ids {
        let prefab = load_prefab(game, asset_id)?;
        if !edit
            .scene(game)
            .prefab_instances(asset_id, prefab.prefab.root)
            .is_empty()
        {
            prefabs.push((asset_id, prefab));
        }
    }

    let mut total = PrefabUpdate::default();
    for (asset_id, prefab) in prefabs {
        let update = edit
            .scene_mut(game)
            .update_prefab_instances(asset_id, &prefab.prefab)
            .map_err(|err| format!("failed to update instances of {}: {err}", prefab.label))?;
        if let Some(report) =
            dropped_components_report(&format!("the prefab {}", prefab.label), &update.dropped)
        {
            log::warn!("{report}");
        }
        log::info!(
            "Updated {} instance(s) of prefab {}: {} object(s) added, {} removed, {} kept unlinked, {} override(s) kept",
            update.instances,
            prefab.label,
            update.added,
            update.removed,
            update.unlinked,
            update.kept_overrides
        );
        total.instances += update.instances;
        total.added += update.added;
        total.removed += update.removed;
        total.unlinked += update.unlinked;
        total.kept_overrides += update.kept_overrides;
        total.dropped.extend(update.dropped);
    }
    Ok(total)
}

/// A prefab asset as loaded for instancing.
struct LoadedPrefab {
    prefab: Prefab,
    label: String,
}

/// Loads the prefab asset `asset_id`. A `.cxprefab` file is read from disk,
/// so a prefab just saved in prefab mode is used before the asset registry
/// notices the change; a model file comes from the asset registry.
fn load_prefab(game: &GameContext, asset_id: Uuid) -> Result<LoadedPrefab, String> {
    let registry = game.assets.registries.assets.read();
    let meta = registry
        .asset_meta_from_id(asset_id)
        .ok_or_else(|| format!("asset {asset_id} is not registered"))?;
    if meta.type_uuid != Prefab::type_uuid() {
        return Err(format!("asset {} is not a prefab", meta.display_name));
    }
    let label = meta.display_name.clone();
    let file = meta
        .path
        .clone()
        .filter(|path| path.extension().is_some_and(|ext| ext == "cxprefab"));
    let prefab = match file {
        Some(file) => {
            drop(registry);
            let contents = std::fs::read(&file)
                .map_err(|err| format!("failed to read prefab {}: {err}", file.display()))?;
            let data: PrefabData = serde_json::from_slice(&contents)
                .map_err(|err| format!("failed to parse prefab {}: {err}", file.display()))?;
            Prefab::from(data)
        }
        None => {
            let asset = registry
                .load_dyn_by_id(asset_id)
                .map_err(|err| format!("failed to load prefab {label}: {err}"))?;
            let prefab_ref = asset
                .try_downcast::<Prefab>()
                .ok_or_else(|| format!("asset {label} is not a prefab"))?;
            let prefab = prefab_ref.read();
            Prefab {
                data: prefab.data.clone(),
                root: prefab.root,
            }
        }
    };
    Ok(LoadedPrefab { prefab, label })
}

/// Why the open scene's prefab instances cannot be updated now, if they
/// cannot: the scene is set aside while a prefab is open, and a simulation
/// copy is discarded when it stops.
pub(crate) fn update_unavailable_reason(state: &EditorAppState) -> Option<&'static str> {
    if state.prefab_mode.is_open() {
        Some(CLOSE_PREFAB_FIRST)
    } else if state.game.scenes.has_simulation_scene() {
        Some("Stop the simulation before updating prefab instances")
    } else {
        None
    }
}

impl EditorAppState {
    /// Updates the open scene's instances of the prefab asset `prefab`, or
    /// of every prefab when `None`, as one undo step. Refused while a prefab
    /// is open or a simulation runs.
    pub(crate) fn update_prefab_instances(
        &mut self,
        prefab: Option<Uuid>,
    ) -> Result<PrefabUpdate, String> {
        if let Some(reason) = update_unavailable_reason(self) {
            return Err(reason.to_owned());
        }
        self.flush_pending_inspector_value_edits();
        let mut edit = SceneEdit::begin("Update prefab instances");
        match update_prefab_instances(&mut edit, &mut self.game, prefab) {
            Ok(update) => {
                self.commit_scene_edit(edit);
                Ok(update)
            }
            Err(error) => {
                edit.cancel();
                Err(error)
            }
        }
    }
}
