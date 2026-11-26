//! The prefab open in prefab mode, and the scene set aside until it closes.
//!
//! Opening a prefab sets the open scene aside, with its unsaved edits, undo
//! history, file and selection, and puts a scene built from the prefab's data
//! in the authoring slot under a document of its own. Saving writes the prefab
//! asset only. Closing the prefab puts the scene that was set aside back as it
//! was.

use std::mem;
use std::path::{Path, PathBuf};

use calyx_engine::context::GameContext;
use calyx_engine::core::Ref;
use calyx_engine::scene::{
    dropped_components_report, Prefab, PrefabData, Scene, SceneMeta, SceneSnapshot,
};
use uuid::Uuid;

use crate::scene_document::SceneDocument;
use crate::scene_file::write_prefab_file;
use crate::selection::{Selection, SelectionType};

/// The prefab file extension prefab mode can edit and save.
const PREFAB_EXTENSION: &str = "cxprefab";

/// The scene set aside while a prefab is open.
struct SetAsideScene {
    document: SceneDocument,
    snapshot: SceneSnapshot,
    meta: SceneMeta,
    selection: Selection,
}

/// An open prefab asset and the scene to return to when it closes.
pub(crate) struct PrefabEditing {
    asset_id: Uuid,
    file: PathBuf,
    label: String,
    /// The prefab's root object as opened. Linked instances are made from
    /// it, so a save keeps it.
    root: Uuid,
    set_aside: SetAsideScene,
}

impl PrefabEditing {
    /// Opens the prefab asset `asset_id` as the authoring scene, setting the
    /// open scene aside with its `document` and `selection`. Leaves
    /// everything as it was and returns why when the asset is not a
    /// `.cxprefab` file, does not load, or the open scene's recovery
    /// autosave fails.
    pub(crate) fn open(
        game: &mut GameContext,
        document: &mut SceneDocument,
        selection: &mut Selection,
        project_path: &Path,
        asset_id: Uuid,
    ) -> Result<Self, String> {
        let (file, label) = prefab_file(game, asset_id)?;
        let data = read_prefab(&file)?;
        let (scene, dropped) = Scene::load(&game.assets.registries.lock_read(), data.data);
        if let Some(report) = dropped_components_report(&format!("the prefab {label}"), &dropped) {
            log::warn!("{report}");
        }
        if !document.set_aside(game) {
            return Err(format!(
                "kept the open scene because its recovery autosave failed; {label} was not opened"
            ));
        }

        game.scenes.stop_simulation();
        let set_aside = SetAsideScene {
            snapshot: game.scenes.current_scene_snapshot(),
            meta: game.scenes.current_scene_meta().clone(),
            document: mem::replace(document, SceneDocument::new(project_path)),
            selection: mem::take(selection),
        };
        game.scenes.load_scene(Ref::new(scene).readonly());
        game.scenes.set_current_scene_meta(SceneMeta::default());
        document.opened_detached(game);
        let scene = game.scenes.current_scene();
        *selection = match scene.find(data.root) {
            Some(root) => Selection::from_id(SelectionType::GameObject, scene.uuid(root)),
            None => Selection::none(),
        };
        log::info!(
            "Opened prefab {label} ({} objects)",
            scene.objects().count()
        );
        Ok(Self {
            asset_id,
            file,
            label,
            root: data.root,
            set_aside,
        })
    }

    /// The prefab asset's id.
    pub(crate) fn asset_id(&self) -> Uuid {
        self.asset_id
    }

    /// The prefab's file.
    pub(crate) fn file(&self) -> &Path {
        &self.file
    }

    /// The prefab's path inside its asset root, such as
    /// `prefabs/crate.cxprefab`.
    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    /// Writes the authoring scene to the prefab's file and marks `document`
    /// clean. The scene must hold exactly one top-level object, the prefab's
    /// root as it was opened: linked instances are made from that object and
    /// cannot follow a new root.
    pub(crate) fn save(
        &self,
        game: &GameContext,
        document: &mut SceneDocument,
    ) -> Result<(), String> {
        document.flush_pending_values(game);
        let scene = game.scenes.current_scene();
        let prefab = scene_prefab(scene)?;
        if prefab.root != self.root {
            let name = |id: Uuid| {
                scene
                    .find(id)
                    .map(|object| scene.name(object))
                    .unwrap_or_else(|| id.to_string())
            };
            return Err(format!(
                "the prefab's root object must stay {}, which its instances are made from, \
                 and the prefab's root is now {}; move {} back to the top",
                name(self.root),
                name(prefab.root),
                name(self.root)
            ));
        }
        write_prefab_file(&self.file, &prefab)
            .map_err(|error| format!("failed to save prefab {}: {error}", self.label))?;
        document.saved_detached(game);
        log::info!(
            "Saved prefab {} ({} objects)",
            self.label,
            prefab.data.components.len()
        );
        Ok(())
    }

    /// Closes the prefab, dropping its unsaved edits, and puts back the scene
    /// it set aside with its `document` and `selection`.
    pub(crate) fn close(
        self,
        game: &mut GameContext,
        document: &mut SceneDocument,
        selection: &mut Selection,
    ) {
        let SetAsideScene {
            document: scene_document,
            snapshot,
            meta,
            selection: scene_selection,
        } = self.set_aside;
        game.scenes.restore_current_scene_snapshot(snapshot);
        game.scenes.set_current_scene_meta(meta);
        *document = scene_document;
        document.resumed();
        *selection = scene_selection;
        log::info!("Closed prefab {}", self.label);
    }
}

/// The prefab `scene` holds: its one top-level object and that object's
/// descendants.
pub(crate) fn scene_prefab(scene: &Scene) -> Result<Prefab, String> {
    let mut roots = scene.root_objects();
    match (roots.next(), roots.next()) {
        (Some(root), None) => Ok(scene.create_prefab(root)),
        (None, _) => Err("a prefab needs a root object, and the prefab has none".into()),
        (Some(_), Some(_)) => Err(format!(
            "a prefab has exactly one root object, and the prefab has {}; \
             move the others under the root",
            scene.root_objects().count()
        )),
    }
}

/// The file and label of the `.cxprefab` asset `asset_id`.
fn prefab_file(game: &GameContext, asset_id: Uuid) -> Result<(PathBuf, String), String> {
    let registry = game.assets.registries.assets.read();
    let meta = registry
        .asset_meta_from_id(asset_id)
        .ok_or_else(|| format!("asset {asset_id} is not registered"))?;
    let file = meta
        .path
        .clone()
        .ok_or_else(|| format!("asset {} has no file", meta.name))?;
    if meta.type_uuid != <Prefab as calyx_engine::utils::TypeUuid>::type_uuid() {
        return Err(format!("{} is not a prefab", file.display()));
    }
    let label = registry
        .asset_paths()
        .iter()
        .find_map(|root| file.strip_prefix(root).ok())
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| meta.display_name.clone());
    if file.extension().and_then(|ext| ext.to_str()) != Some(PREFAB_EXTENSION) {
        return Err(format!(
            "{label} is imported from a model file and cannot be edited; \
             only .{PREFAB_EXTENSION} prefabs open in prefab mode"
        ));
    }
    Ok((file, label))
}

/// Reads the prefab file at `file` from disk, so prefab mode starts from what
/// was last saved.
fn read_prefab(file: &Path) -> Result<PrefabData, String> {
    let contents = std::fs::read(file)
        .map_err(|error| format!("failed to read prefab {}: {error}", file.display()))?;
    serde_json::from_slice(&contents)
        .map_err(|error| format!("failed to parse prefab {}: {error}", file.display()))
}

#[cfg(test)]
mod tests;
