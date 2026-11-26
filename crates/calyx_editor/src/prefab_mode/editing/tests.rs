use super::{scene_prefab, PrefabEditing};
use crate::scene_document::SceneDocument;
use crate::scene_edit::SceneEdit;
use crate::scene_file::{write_prefab_file, write_scene_file};
use crate::selection::{Selection, SelectionType};
use calyx_engine::component::ComponentID;
use calyx_engine::context::GameContext;
use calyx_engine::scene::{GameObject, Prefab, PrefabData, Scene};
use calyx_engine::test_support::{test_asset_context_with_assets, test_game_context};
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
fn a_prefab_saves_only_with_its_root_object_kept() {
    let root = AssetRoot::new();
    let mut editor = Editor::open_level(&root);
    let prefab_file = fs::read(root.prefab_file()).unwrap();
    let prefab = editor.open_prefab(&root);
    let body = editor.find("Crate");
    let mut edit = SceneEdit::begin("Wrap crate");
    let scene = edit.scene_mut(&mut editor.game);
    let pallet = create(scene, "Pallet", None);
    scene
        .set_parent(body, Some(pallet))
        .expect("the crate should move under the pallet");
    edit.commit(&mut editor.document, &editor.game);

    let error = prefab
        .save(&editor.game, &mut editor.document)
        .err()
        .expect("a new root object does not save");

    assert!(error.contains("root object"), "{error}");
    assert_eq!(fs::read(root.prefab_file()).unwrap(), prefab_file);
    assert!(editor.document.is_dirty());
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
    use calyx_engine::component::{ComponentDirectionalLight, ComponentSkyLight};
    use calyx_engine::utils::TypeUuid;

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

#[test]
fn an_invalid_prefab_stops_the_update_before_any_instance_changes() {
    use crate::prefab_instances::{instantiate_prefab, update_prefab_instances};

    let root = AssetRoot::new();
    let copy_file = root.0.join("prefabs").join("crate-copy.cxprefab");
    fs::copy(root.prefab_file(), &copy_file).expect("prefab should copy");
    let mut editor = Editor::open_level(&root);
    let mut files = [root.prefab_file(), copy_file];
    // Every prefab updates in asset id order: the first takes an edit,
    // the second turns out invalid.
    files.sort_by_key(|file| editor.asset_id(file));
    let [first, second] = files;
    let mut edit = SceneEdit::begin("Import prefabs");
    for file in [&first, &second] {
        let asset_id = editor.asset_id(file);
        instantiate_prefab(&mut edit, &mut editor.game, asset_id)
            .expect("prefab should instantiate");
    }
    edit.commit(&mut editor.document, &editor.game);

    let mut edited = read_prefab(&first);
    for components in edited.data.components.values_mut() {
        for value in components.values_mut() {
            if value.get("name").and_then(|name| name.as_str()) == Some("Lid") {
                value["name"] = "Hatch".into();
            }
        }
    }
    write_prefab_file(&first, &Prefab::from(edited)).expect("prefab should be written");
    // Well-formed JSON whose hierarchy names an object it does not hold.
    let mut broken = read_prefab(&second);
    broken
        .data
        .hierarchy
        .entry(broken.root)
        .or_default()
        .push(Uuid::new_v4());
    write_prefab_file(&second, &Prefab::from(broken)).expect("prefab should be written");
    let names = editor.names();

    // As the editor runs it: an update that fails is cancelled.
    let mut edit = SceneEdit::begin("Update prefab instances");
    let error = update_prefab_instances(&mut edit, &mut editor.game, None)
        .err()
        .expect("an invalid prefab fails the update");
    edit.cancel();

    assert!(error.contains("is missing"), "{error}");
    assert_eq!(editor.names(), names, "no instance changed");
}
