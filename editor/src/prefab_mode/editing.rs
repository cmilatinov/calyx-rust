//! The prefab open in prefab mode, and the scene set aside until it closes.
//!
//! Opening a prefab sets the open scene aside, with its unsaved edits, undo
//! history, file and selection, and puts a scene built from the prefab's data
//! in the authoring slot under a document of its own. Saving writes the prefab
//! asset only. Closing the prefab puts the scene that was set aside back as it
//! was.

use std::mem;
use std::path::{Path, PathBuf};

use engine::context::GameContext;
use engine::core::Ref;
use engine::scene::{
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
    /// root.
    pub(crate) fn save(
        &self,
        game: &GameContext,
        document: &mut SceneDocument,
    ) -> Result<(), String> {
        document.flush_pending_values(game);
        let prefab = scene_prefab(game.scenes.current_scene())?;
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
    if meta.type_uuid != <Prefab as engine::utils::TypeUuid>::type_uuid() {
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
mod tests {
    use super::{scene_prefab, PrefabEditing};
    use crate::scene_document::SceneDocument;
    use crate::scene_edit::SceneEdit;
    use crate::scene_file::{write_prefab_file, write_scene_file};
    use crate::selection::{Selection, SelectionType};
    use engine::component::ComponentID;
    use engine::context::GameContext;
    use engine::scene::{GameObject, PrefabData, Scene};
    use engine::test_support::{test_asset_context_with_assets, test_game_context};
    use nalgebra_glm::{self as glm, Vec3};
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    const PROJECT: &str = "prefab-editing-test-project";

    /// A scratch asset root holding a crate prefab and a level scene with one
    /// instance of it, removed on drop.
    struct AssetRoot(PathBuf);

    impl AssetRoot {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("calyx-prefab-editing-test-{}", Uuid::new_v4()));
            fs::create_dir_all(root.join("prefabs")).expect("asset root should be created");
            let root = Self(dunce::canonicalize(&root).expect("asset root should canonicalize"));

            let mut authoring = test_game_context();
            let scene = authoring.scenes.current_scene_mut();
            let body = create(scene, "Crate", None);
            create(scene, "Lid", Some(body));
            let prefab = scene.create_prefab(body);
            write_prefab_file(&root.prefab_file(), &prefab).expect("prefab should be written");

            let mut level = test_game_context();
            let scene = level.scenes.current_scene_mut();
            let instance = scene
                .instantiate_prefab(&prefab, None)
                .expect("prefab should instantiate");
            scene.set_transform(instance, &glm::translation(&Vec3::new(3.0, 0.0, 0.0)));
            write_scene_file(&root.scene_file(), scene).expect("scene should be written");
            root
        }

        fn prefab_file(&self) -> PathBuf {
            self.0.join("prefabs").join("crate.cxprefab")
        }

        fn scene_file(&self) -> PathBuf {
            self.0.join("level.cxscene")
        }
    }

    impl Drop for AssetRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn create(scene: &mut Scene, name: &str, parent: Option<GameObject>) -> GameObject {
        let id = ComponentID {
            name: name.into(),
            ..Default::default()
        };
        scene.create(Some(id), parent)
    }

    /// An editor on the level scene of an [`AssetRoot`]: the game, its scene
    /// document and its selection.
    struct Editor {
        game: GameContext,
        document: SceneDocument,
        selection: Selection,
    }

    impl Editor {
        fn open_level(root: &AssetRoot) -> Self {
            let mut game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
            let mut document = SceneDocument::new(Path::new(PROJECT));
            assert!(document.open_file(&mut game, root.scene_file()));
            Self {
                game,
                document,
                selection: Selection::none(),
            }
        }

        fn asset_id(&self, file: &Path) -> Uuid {
            self.game
                .assets
                .registries
                .assets
                .read()
                .asset_id_from_path(file)
                .expect("asset should be registered")
        }

        fn open(&mut self, asset_id: Uuid) -> Result<PrefabEditing, String> {
            PrefabEditing::open(
                &mut self.game,
                &mut self.document,
                &mut self.selection,
                Path::new(PROJECT),
                asset_id,
            )
        }

        fn open_prefab(&mut self, root: &AssetRoot) -> PrefabEditing {
            let asset_id = self.asset_id(&root.prefab_file());
            self.open(asset_id).expect("prefab should open")
        }

        fn close(&mut self, prefab: PrefabEditing) {
            prefab.close(&mut self.game, &mut self.document, &mut self.selection);
        }

        fn scene(&self) -> &Scene {
            self.game.scenes.current_scene()
        }

        fn names(&self) -> Vec<String> {
            let mut names = self
                .scene()
                .objects()
                .map(|object| self.scene().name(object))
                .collect::<Vec<_>>();
            names.sort();
            names
        }

        fn find(&self, name: &str) -> GameObject {
            self.scene()
                .objects()
                .find(|object| self.scene().name(*object) == name)
                .unwrap_or_else(|| panic!("{name} should exist"))
        }

        /// Adds an object named `name` under `parent` as one undo step.
        fn add_object(&mut self, name: &str, parent: &str) {
            let parent = self.find(parent);
            let mut edit = SceneEdit::begin("Add object");
            create(edit.scene_mut(&mut self.game), name, Some(parent));
            edit.commit(&mut self.document, &self.game);
        }
    }

    fn read_prefab(file: &Path) -> PrefabData {
        serde_json::from_slice(&fs::read(file).expect("prefab should be readable"))
            .expect("prefab should parse")
    }

    fn prefab_names(prefab: &PrefabData) -> Vec<String> {
        let mut names = prefab
            .data
            .components
            .values()
            .filter_map(|components| {
                components
                    .values()
                    .find_map(|value| value.get("name")?.as_str().map(str::to_owned))
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn opening_a_prefab_makes_it_the_authoring_scene() {
        let root = AssetRoot::new();
        let mut editor = Editor::open_level(&root);
        let saved = read_prefab(&root.prefab_file());

        let prefab = editor.open_prefab(&root);

        assert_eq!(prefab.label(), "prefabs/crate.cxprefab");
        assert_eq!(prefab.file(), root.prefab_file());
        assert_eq!(editor.names(), ["Crate", "Lid"]);
        let scene_root = editor.scene().prefab_root().expect("prefab has a root");
        assert_eq!(editor.scene().uuid(scene_root), saved.root);
        assert!(editor
            .selection
            .contains(SelectionType::GameObject, saved.root));
        assert_eq!(editor.game.scenes.current_scene_meta().file, None);
        assert!(!editor.document.is_dirty());
        assert!(!editor.document.can_undo(&editor.game));
    }

    #[test]
    fn edits_save_back_to_the_prefab_asset_with_undo() {
        let root = AssetRoot::new();
        let mut editor = Editor::open_level(&root);
        let saved = read_prefab(&root.prefab_file());
        let prefab = editor.open_prefab(&root);

        editor.add_object("Handle", "Crate");
        assert!(editor.document.is_dirty());
        assert!(editor.document.can_undo(&editor.game));
        editor.document.undo(&mut editor.game);
        assert!(
            !editor.document.is_dirty(),
            "undo returns to the saved prefab"
        );
        editor.document.redo(&mut editor.game);

        prefab
            .save(&editor.game, &mut editor.document)
            .expect("prefab should save");

        assert!(!editor.document.is_dirty());
        let written = read_prefab(&root.prefab_file());
        assert_eq!(prefab_names(&written), ["Crate", "Handle", "Lid"]);
        assert_eq!(written.root, saved.root, "the root keeps its id");
        for id in saved.data.components.keys() {
            assert!(
                written.data.components.contains_key(id),
                "object {id} keeps its id"
            );
        }
        let handle = editor.scene().uuid(editor.find("Handle"));
        assert_eq!(written.data.hierarchy[&saved.root].last(), Some(&handle));

        // Saving the same prefab again writes the same file.
        let first = fs::read(root.prefab_file()).unwrap();
        prefab
            .save(&editor.game, &mut editor.document)
            .expect("prefab should save again");
        assert_eq!(fs::read(root.prefab_file()).unwrap(), first);
    }

    #[test]
    fn closing_returns_to_the_scene_with_its_instances_and_edits_unchanged() {
        let root = AssetRoot::new();
        let mut editor = Editor::open_level(&root);
        let scene_file = fs::read(root.scene_file()).unwrap();
        // An unsaved scene edit the prefab must not disturb.
        editor.add_object("Marker", "Crate");
        let scene_names = editor.names();
        let instance = editor.scene().uuid(editor.find("Crate"));
        editor.selection = Selection::from_id(SelectionType::GameObject, instance);
        let instance_position = |editor: &Editor| {
            let scene = editor.scene();
            scene
                .world_transform(scene.find(instance).unwrap())
                .position
        };
        assert!((instance_position(&editor) - Vec3::new(3.0, 0.0, 0.0)).norm() < 1e-5);

        let prefab = editor.open_prefab(&root);
        editor.add_object("Handle", "Lid");
        let lid = editor.find("Lid");
        let mut edit = SceneEdit::begin("Move lid");
        edit.scene_mut(&mut editor.game)
            .set_transform(lid, &glm::translation(&Vec3::new(0.0, 2.0, 0.0)));
        edit.commit(&mut editor.document, &editor.game);
        prefab
            .save(&editor.game, &mut editor.document)
            .expect("prefab should save");
        editor.close(prefab);

        assert_eq!(editor.names(), scene_names, "the scene is back");
        assert!(
            !editor.names().contains(&"Handle".to_owned()),
            "the instance did not take the prefab's new object"
        );
        assert!((instance_position(&editor) - Vec3::new(3.0, 0.0, 0.0)).norm() < 1e-5);
        let lid = editor.find("Lid");
        assert!(
            editor.scene().transform(lid).position.norm() < 1e-5,
            "the instance's lid did not move"
        );
        assert_eq!(
            editor.game.scenes.current_scene_meta().file.as_deref(),
            Some(root.scene_file().as_path())
        );
        assert!(editor
            .selection
            .contains(SelectionType::GameObject, instance));
        assert_eq!(fs::read(root.scene_file()).unwrap(), scene_file);
        assert!(
            editor.document.is_dirty(),
            "the scene edit is still unsaved"
        );
        editor.document.undo(&mut editor.game);
        assert!(!editor.names().contains(&"Marker".to_owned()));
        assert!(!editor.document.is_dirty(), "its undo history came back");
    }

    #[test]
    fn closing_without_saving_leaves_the_prefab_asset_unchanged() {
        let root = AssetRoot::new();
        let mut editor = Editor::open_level(&root);
        let prefab_file = fs::read(root.prefab_file()).unwrap();

        let prefab = editor.open_prefab(&root);
        editor.add_object("Handle", "Crate");
        editor.close(prefab);

        assert_eq!(fs::read(root.prefab_file()).unwrap(), prefab_file);
        let prefab = editor.open_prefab(&root);
        assert_eq!(editor.names(), ["Crate", "Lid"]);
        editor.close(prefab);
    }

    #[test]
    fn a_prefab_saves_only_with_exactly_one_root_object() {
        let mut game = test_game_context();
        let scene = game.scenes.current_scene_mut();
        assert!(scene_prefab(scene).is_err(), "no root object");
        let body = create(scene, "Crate", None);
        create(scene, "Lid", Some(body));
        let prefab = scene_prefab(scene).expect("one root object saves");
        assert_eq!(prefab.root, scene.uuid(body));
        create(scene, "Stray", None);
        let error = scene_prefab(scene)
            .err()
            .expect("two root objects do not save");
        assert!(error.contains("exactly one root object"), "{error}");
    }

    #[test]
    fn only_prefab_assets_open_in_prefab_mode() {
        let root = AssetRoot::new();
        let mut editor = Editor::open_level(&root);
        let scene_asset = editor.asset_id(&root.scene_file());

        let error = editor
            .open(scene_asset)
            .err()
            .expect("a scene does not open as a prefab");

        assert!(error.contains("is not a prefab"), "{error}");
        assert_eq!(
            editor.game.scenes.current_scene_meta().file.as_deref(),
            Some(root.scene_file().as_path()),
            "the scene stays open"
        );
    }
    #[test]
    fn a_saved_prefab_holds_no_lighting_of_prefab_mode() {
        use engine::component::{ComponentDirectionalLight, ComponentSkyLight};
        use engine::utils::TypeUuid;

        let root = AssetRoot::new();
        let mut editor = Editor::open_level(&root);
        let saved = read_prefab(&root.prefab_file());
        let prefab = editor.open_prefab(&root);
        editor.add_object("Handle", "Crate");
        prefab
            .save(&editor.game, &mut editor.document)
            .expect("prefab should save");

        // Prefab mode lights the view, not the prefab: the save holds the
        // prefab's objects and no light of the editor's.
        let written = read_prefab(&root.prefab_file());
        assert_eq!(
            written.data.components.len(),
            saved.data.components.len() + 1
        );
        let lights = [
            ComponentSkyLight::type_uuid(),
            ComponentDirectionalLight::type_uuid(),
        ];
        for components in written.data.components.values() {
            assert!(
                components
                    .keys()
                    .all(|component| !lights.contains(component)),
                "a saved object holds a light: {components:?}"
            );
        }
    }

    /// The names of `root` and its descendants in `scene`, sorted.
    fn subtree_names(scene: &Scene, root: GameObject) -> Vec<String> {
        let mut names = scene
            .descendants(root)
            .chain([root])
            .map(|object| scene.name(object))
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    }

    #[test]
    fn a_saved_prefab_reaches_linked_instances_only_through_the_update() {
        use crate::prefab_instances::{instantiate_prefab, update_prefab_instances};

        let root = AssetRoot::new();
        let mut editor = Editor::open_level(&root);
        let asset_id = editor.asset_id(&root.prefab_file());
        let mut edit = SceneEdit::begin("Import prefab");
        let instance = instantiate_prefab(&mut edit, &mut editor.game, asset_id)
            .expect("prefab should instantiate");
        edit.commit(&mut editor.document, &editor.game);
        let instance_object = editor.scene().find(instance).unwrap();
        let mut edit = SceneEdit::begin("Move instance");
        edit.scene_mut(&mut editor.game).set_transform(
            instance_object,
            &glm::translation(&Vec3::new(0.0, 0.0, 5.0)),
        );
        edit.commit(&mut editor.document, &editor.game);
        let instance_lid = |editor: &Editor| {
            let scene = editor.scene();
            let root = scene.find(instance).unwrap();
            scene
                .children(root)
                .find(|child| scene.name(*child) == "Lid")
                .expect("the instance has a lid")
        };
        let lid_id = editor.scene().uuid(instance_lid(&editor));

        let prefab = editor.open_prefab(&root);
        editor.add_object("Handle", "Lid");
        let lid = editor.find("Lid");
        let mut edit = SceneEdit::begin("Move lid");
        edit.scene_mut(&mut editor.game)
            .set_transform(lid, &glm::translation(&Vec3::new(0.0, 2.0, 0.0)));
        edit.commit(&mut editor.document, &editor.game);
        prefab
            .save(&editor.game, &mut editor.document)
            .expect("prefab should save");
        editor.close(prefab);

        // Saving the prefab left the instance as it was.
        let instance_object = editor.scene().find(instance).unwrap();
        assert_eq!(
            subtree_names(editor.scene(), instance_object),
            ["Crate", "Lid"]
        );
        assert!(
            editor
                .scene()
                .transform(instance_lid(&editor))
                .position
                .norm()
                < 1e-5
        );

        let mut edit = SceneEdit::begin("Update prefab instances");
        let update = update_prefab_instances(&mut edit, &mut editor.game, Some(asset_id))
            .expect("instances should update");
        edit.commit(&mut editor.document, &editor.game);

        assert_eq!(update.instances, 1, "the unlinked copy is not an instance");
        assert_eq!(update.added, 1);
        let scene = editor.scene();
        let instance_object = scene.find(instance).unwrap();
        assert_eq!(
            subtree_names(scene, instance_object),
            ["Crate", "Handle", "Lid"]
        );
        let lid = instance_lid(&editor);
        assert_eq!(scene.uuid(lid), lid_id, "the lid keeps its id");
        assert!((scene.transform(lid).position - Vec3::new(0.0, 2.0, 0.0)).norm() < 1e-5);
        assert!(
            (scene.transform(instance_object).position - Vec3::new(0.0, 0.0, 5.0)).norm() < 1e-5,
            "the instance keeps its root transform"
        );

        // The update is one undo step.
        editor.document.undo(&mut editor.game);
        let instance_object = editor.scene().find(instance).unwrap();
        assert_eq!(
            subtree_names(editor.scene(), instance_object),
            ["Crate", "Lid"]
        );
        assert!(
            editor
                .scene()
                .transform(instance_lid(&editor))
                .position
                .norm()
                < 1e-5
        );
    }
}
