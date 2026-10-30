use std::fs;
use std::path::{Path, PathBuf};

use calyx_engine::assets::animation_graph::AnimationGraph;
use calyx_engine::component::ComponentID;
use calyx_engine::context::GameContext;
use calyx_engine::core::Ref;
use calyx_engine::input::ActionMap;
use calyx_engine::scene::Scene;
use calyx_engine::test_support::{test_asset_context_with_assets, test_game_context};
use uuid::Uuid;

use super::{
    AssetOpenContext, AssetOpenOutcome, AssetOpenerRegistry, SceneOpenTarget, SceneSwitch,
    SceneSwitchAnswer, OPEN_DURING_PLAY,
};
use crate::panel::{Panel, PanelAnimator};
use crate::prefab_mode::{PrefabMode, CLOSE_PREFAB_FIRST};
use crate::scene_document::SceneDocument;
use crate::scene_edit::SceneEdit;
use crate::scene_file::{write_prefab_file, write_scene_file};
use crate::selection::{Selection, SelectionType};
use crate::ProjectAssemblyStatus;

const PROJECT: &str = "asset-open-test-project";

/// A scratch asset root, removed on drop, holding two scenes, two prefabs, an
/// animation graph and an input map.
struct AssetRoot(PathBuf);

impl AssetRoot {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("calyx-asset-open-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("asset root should be created");
        let root = Self(dunce::canonicalize(&root).expect("asset root should canonicalize"));

        write_scene(&root.file("first.cxscene"), &["First"]);
        write_scene(&root.file("second.cxscene"), &["Second", "Other"]);

        let mut authoring = test_game_context();
        let scene = authoring.scenes.current_scene_mut();
        let body = create(scene, "Crate");
        let prefab = scene.create_prefab(body);
        write_prefab_file(&root.file("crate.cxprefab"), &prefab).expect("prefab should be written");
        write_prefab_file(&root.file("barrel.cxprefab"), &prefab)
            .expect("prefab should be written");

        let graph = serde_json::to_vec(&AnimationGraph::default()).expect("graph should serialize");
        fs::write(root.file("walk.cxanim"), graph).expect("graph should be written");
        let input = serde_json::to_vec(&ActionMap::default()).expect("input should serialize");
        fs::write(root.file("input.cxinput"), input).expect("input should be written");
        root
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for AssetRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn create(scene: &mut Scene, name: &str) -> calyx_engine::scene::GameObject {
    let id = ComponentID {
        name: name.into(),
        ..Default::default()
    };
    scene.create(Some(id), None)
}

fn write_scene(file: &Path, names: &[&str]) {
    let mut game = test_game_context();
    let scene = game.scenes.current_scene_mut();
    for name in names {
        create(scene, name);
    }
    write_scene_file(file, scene).expect("scene should be written");
}

/// The editor state an asset open changes, owned, with the openers.
struct Editor {
    game: GameContext,
    document: SceneDocument,
    prefab_mode: PrefabMode,
    selection: Selection,
    scene_switch: SceneSwitch,
    focus_panel: Option<&'static str>,
    openers: AssetOpenerRegistry,
}

impl Editor {
    /// An editor on `first.cxscene` of `root`, with project assemblies loaded.
    fn new(root: &AssetRoot) -> Self {
        let mut game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
        game.resources
            .insert(Ref::new(ProjectAssemblyStatus::loaded()));
        let openers = AssetOpenerRegistry::new(&game.assets.registries.types.read());
        let mut document = SceneDocument::new(Path::new(PROJECT));
        assert!(document.open_file(&mut game, root.file("first.cxscene")));
        Self {
            game,
            document,
            prefab_mode: PrefabMode::default(),
            selection: Selection::none(),
            scene_switch: SceneSwitch::default(),
            focus_panel: None,
            openers,
        }
    }

    fn context(&mut self) -> AssetOpenContext<'_> {
        AssetOpenContext {
            game: &mut self.game,
            scene_document: &mut self.document,
            prefab_mode: &mut self.prefab_mode,
            selection: &mut self.selection,
            project_path: Path::new(PROJECT),
            scene_switch: &mut self.scene_switch,
            focus_panel: &mut self.focus_panel,
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

    /// Opens the asset file `file` through the dispatch.
    fn open(&mut self, file: &Path) -> AssetOpenOutcome {
        let asset_id = self.asset_id(file);
        let openers = std::mem::take(&mut self.openers);
        let outcome = openers.open(&mut self.context(), asset_id);
        self.openers = openers;
        outcome
    }

    fn answer(&mut self, answer: SceneSwitchAnswer) -> AssetOpenOutcome {
        self.context().answer_scene_prompt(answer, || None)
    }

    fn scene_file(&self) -> Option<PathBuf> {
        self.game.scenes.current_scene_meta().file.clone()
    }

    fn names(&self) -> Vec<String> {
        let scene = self.game.scenes.current_scene();
        let mut names = scene
            .objects()
            .map(|object| scene.name(object))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    /// Adds an object as one undo step, which leaves the scene dirty.
    fn edit_scene(&mut self) {
        let mut edit = SceneEdit::begin("Add object");
        create(edit.scene_mut(&mut self.game), "Edit");
        edit.commit(&mut self.document, &self.game);
        assert!(self.document.is_dirty());
    }
}

#[test]
fn opening_a_scene_loads_it_as_the_authoring_scene() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);

    let outcome = editor.open(&root.file("second.cxscene"));

    assert_eq!(outcome, AssetOpenOutcome::Opened);
    assert_eq!(editor.names(), ["Other", "Second"]);
    assert!(!editor.document.is_dirty());
}

#[test]
fn opening_a_prefab_enters_prefab_mode() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    let prefab = editor.asset_id(&root.file("crate.cxprefab"));

    let outcome = editor.open(&root.file("crate.cxprefab"));

    assert_eq!(outcome, AssetOpenOutcome::Opened);
    let open = editor
        .prefab_mode
        .prefab()
        .expect("a prefab should be open");
    assert_eq!(open.asset_id(), prefab);
    assert_eq!(editor.names(), ["Crate"]);
}

#[test]
fn opening_another_prefab_over_unsaved_prefab_changes_prompts() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    let crate_prefab = editor.asset_id(&root.file("crate.cxprefab"));
    assert_eq!(
        editor.open(&root.file("crate.cxprefab")),
        AssetOpenOutcome::Opened
    );
    editor.edit_scene();

    let outcome = editor.open(&root.file("barrel.cxprefab"));

    assert_eq!(outcome, AssetOpenOutcome::Prompted);
    let open = editor.prefab_mode.prefab().expect("the prefab stays open");
    assert_eq!(open.asset_id(), crate_prefab);
}

#[test]
fn opening_an_animation_graph_selects_it_in_the_animator() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    let graph = editor.asset_id(&root.file("walk.cxanim"));

    let outcome = editor.open(&root.file("walk.cxanim"));

    assert_eq!(outcome, AssetOpenOutcome::Opened);
    assert!(editor.selection.contains(SelectionType::Asset, graph));
    assert_eq!(editor.focus_panel, Some(PanelAnimator::name()));
}

#[test]
fn a_type_without_an_opener_is_selected_for_the_inspector() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    let input = editor.asset_id(&root.file("input.cxinput"));

    let outcome = editor.open(&root.file("input.cxinput"));

    assert_eq!(outcome, AssetOpenOutcome::Selected);
    assert!(editor.selection.contains(SelectionType::Asset, input));
    assert_eq!(editor.focus_panel, None);
    assert_eq!(editor.scene_file(), Some(root.file("first.cxscene")));
}

#[test]
fn opening_a_scene_over_unsaved_changes_waits_on_the_prompt() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    editor.edit_scene();

    let outcome = editor.open(&root.file("second.cxscene"));

    assert_eq!(outcome, AssetOpenOutcome::Prompted);
    assert_eq!(editor.scene_file(), Some(root.file("first.cxscene")));
    assert!(editor.scene_switch.pending().is_some());
}

#[test]
fn save_writes_the_open_scene_then_opens_the_other() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    editor.edit_scene();
    editor.open(&root.file("second.cxscene"));

    let outcome = editor.answer(SceneSwitchAnswer::Save);

    assert_eq!(outcome, AssetOpenOutcome::Opened);
    assert_eq!(editor.names(), ["Other", "Second"]);
    assert!(editor.scene_switch.pending().is_none());
    let saved = fs::read_to_string(root.file("first.cxscene")).expect("scene should read");
    assert!(saved.contains("\"Edit\""), "the edit should be saved");
}

#[test]
fn save_of_a_scene_without_a_file_uses_the_picked_file() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    editor.document.open_default(&mut editor.game);
    editor.edit_scene();
    editor.open(&root.file("second.cxscene"));
    let picked = root.file("picked.cxscene");

    let outcome = editor
        .context()
        .answer_scene_prompt(SceneSwitchAnswer::Save, || Some(picked.clone()));

    assert_eq!(outcome, AssetOpenOutcome::Opened);
    assert!(picked.is_file(), "the scene should be saved where picked");
    assert_eq!(editor.names(), ["Other", "Second"]);
}

#[test]
fn save_without_a_file_or_a_pick_keeps_the_open_scene() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    editor.document.open_default(&mut editor.game);
    editor.edit_scene();
    editor.open(&root.file("second.cxscene"));

    let outcome = editor.answer(SceneSwitchAnswer::Save);

    assert!(matches!(outcome, AssetOpenOutcome::Refused(_)));
    assert_eq!(editor.scene_file(), None);
    assert!(editor.document.is_dirty());
}

#[test]
fn dont_save_opens_the_other_scene_leaving_the_file_unchanged() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    let before = fs::read_to_string(root.file("first.cxscene")).expect("scene should read");
    editor.edit_scene();
    editor.open(&root.file("second.cxscene"));

    let outcome = editor.answer(SceneSwitchAnswer::Discard);

    assert_eq!(outcome, AssetOpenOutcome::Opened);
    assert_eq!(editor.names(), ["Other", "Second"]);
    let after = fs::read_to_string(root.file("first.cxscene")).expect("scene should read");
    assert_eq!(before, after);
}

#[test]
fn cancel_keeps_the_open_scene_and_its_changes() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    editor.edit_scene();
    editor.open(&root.file("second.cxscene"));

    let outcome = editor.answer(SceneSwitchAnswer::Cancel);

    assert!(matches!(outcome, AssetOpenOutcome::Refused(_)));
    assert_eq!(editor.scene_file(), Some(root.file("first.cxscene")));
    assert_eq!(editor.names(), ["Edit", "First"]);
    assert!(editor.document.is_dirty());
    assert!(editor.scene_switch.pending().is_none());
}

#[test]
fn file_open_goes_through_the_same_prompt() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    editor.edit_scene();

    let outcome = editor
        .context()
        .request_scene_open(SceneOpenTarget::File(root.file("second.cxscene")));
    assert_eq!(outcome, AssetOpenOutcome::Prompted);
    assert_eq!(
        editor.answer(SceneSwitchAnswer::Discard),
        AssetOpenOutcome::Opened
    );
    assert_eq!(editor.scene_file(), Some(root.file("second.cxscene")));
}

#[test]
fn scene_and_prefab_opens_are_refused_during_play() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    editor.game.scenes.start_simulation();

    let scene = editor.open(&root.file("second.cxscene"));
    let prefab = editor.open(&root.file("crate.cxprefab"));

    assert_eq!(scene, AssetOpenOutcome::Refused(OPEN_DURING_PLAY.into()));
    assert_eq!(prefab, AssetOpenOutcome::Refused(OPEN_DURING_PLAY.into()));
    assert!(!editor.prefab_mode.is_open());
    assert_eq!(editor.scene_file(), Some(root.file("first.cxscene")));
    let reason = {
        let openers = std::mem::take(&mut editor.openers);
        let asset_id = editor.asset_id(&root.file("crate.cxprefab"));
        let reason = openers.open_unavailable_reason(&editor.context(), asset_id);
        editor.openers = openers;
        reason
    };
    assert_eq!(reason, Some(OPEN_DURING_PLAY));
}

#[test]
fn inspector_only_opens_still_work_during_play() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    editor.game.scenes.start_simulation();

    assert_eq!(
        editor.open(&root.file("walk.cxanim")),
        AssetOpenOutcome::Opened
    );
    assert_eq!(
        editor.open(&root.file("input.cxinput")),
        AssetOpenOutcome::Selected
    );
}

#[test]
fn opening_a_scene_while_a_prefab_is_open_is_refused() {
    let root = AssetRoot::new();
    let mut editor = Editor::new(&root);
    editor.open(&root.file("crate.cxprefab"));

    let outcome = editor.open(&root.file("second.cxscene"));

    assert_eq!(
        outcome,
        AssetOpenOutcome::Refused(CLOSE_PREFAB_FIRST.into())
    );
    assert!(editor.prefab_mode.is_open());
}
