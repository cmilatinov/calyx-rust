use super::*;
use crate::scene_document::SceneDocument;
use calyx_engine::component::{ComponentBloom, ComponentVignette};
use calyx_engine::render::postprocess::PostProcessSettings;
use calyx_engine::test_support::test_asset_context_with_assets;

/// A temporary asset root, deleted when dropped.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("calyx-profile-editor-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("the temp root is creatable");
        Self(dunce::canonicalize(dir).expect("the temp root exists"))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bloom(intensity: f32) -> ComponentBloom {
    ComponentBloom {
        intensity,
        ..Default::default()
    }
}

/// A global volume with an enabled bloom and a disabled vignette, added
/// outside the undo history.
fn volume_with_effects(game: &mut GameContext) -> GameObject {
    let scene = game.scenes.current_scene_mut();
    let object = scene.create(None, None);
    scene.add_component(object, ComponentGlobalPostProcessVolume::default());
    scene.add_component(object, bloom(0.9));
    scene.add_component(object, ComponentVignette::default());
    scene.set_component_enabled::<ComponentVignette>(object, false);
    object
}

/// The profile saved at `path`, read through `game`'s registries.
fn read_profile(game: &GameContext, path: &Path) -> PostProcessProfile {
    let data = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let registries = &game.assets.registries;
    PostProcessProfile::from_data(
        data,
        &registries.types.read(),
        &registries.components.read(),
    )
}

fn has<T: calyx_engine::component::Component>(game: &GameContext, object: GameObject) -> bool {
    game.scenes
        .current_scene()
        .read_component::<T, _, _>(object, |_| ())
        .is_some()
}

#[test]
fn create_profile_moves_the_volumes_effects_into_a_new_profile_it_references() {
    let root = TempRoot::new();
    let mut game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    let mut document = SceneDocument::new(Path::new("profile-test-project"));
    let object = volume_with_effects(&mut game);
    let before = PostProcessSettings::from_game_object(game.scenes.current_scene(), object);
    let path = root.0.join("look.cxpostfx");

    let mut edit = SceneEdit::begin("Create post-process profile");
    let id = PostProcessProfileInspector::create_from_volume(&mut edit, &mut game, object, &path)
        .expect("the profile is created");
    edit.commit(&mut document, &game);

    let scene = game.scenes.current_scene();
    assert_eq!(
        PostProcessProfile::volume_profile_id(scene, object),
        Some(id)
    );
    assert!(!has::<ComponentBloom>(&game, object));
    assert!(!has::<ComponentVignette>(&game, object));
    assert_eq!(PostProcessSettings::from_game_object(scene, object), before);
    let saved = read_profile(&game, &path);
    let scene = game.scenes.current_scene();
    assert_eq!(saved, PostProcessProfile::from_game_object(scene, object));
    assert_eq!(
        saved
            .entry(ComponentBloom::type_uuid())
            .and_then(|entry| entry.settings::<ComponentBloom>()),
        Some(&bloom(0.9))
    );

    let uuid = scene.uuid(object);
    document.undo(&mut game);
    let object = game
        .scenes
        .current_scene()
        .find(uuid)
        .expect("undo keeps the volume");
    assert!(has::<ComponentBloom>(&game, object), "undo restores bloom");
    assert!(
        has::<ComponentVignette>(&game, object),
        "undo restores vignette"
    );
    assert_eq!(
        PostProcessProfile::volume_profile_id(game.scenes.current_scene(), object),
        None
    );
}

#[test]
fn create_profile_refuses_an_object_without_a_volume() {
    let root = TempRoot::new();
    let mut game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    let object = game.scenes.current_scene_mut().create(None, None);
    let path = root.0.join("look.cxpostfx");

    let mut edit = SceneEdit::begin("Create post-process profile");
    let result =
        PostProcessProfileInspector::create_from_volume(&mut edit, &mut game, object, &path);
    edit.cancel();

    assert!(result.is_err());
    assert!(!path.exists());
}

#[test]
fn new_profiles_from_the_content_browser_take_free_names() {
    let root = TempRoot::new();

    let first = PostProcessProfileInspector::create_in_folder(&root.0).unwrap();
    let second = PostProcessProfileInspector::create_in_folder(&root.0).unwrap();

    assert_eq!(first, root.0.join("post_process_profile.cxpostfx"));
    assert_eq!(second, root.0.join("post_process_profile_1.cxpostfx"));
    let game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    assert_eq!(read_profile(&game, &first), PostProcessProfile::default());
}

/// The profile inspector edits the loaded profile in place: a header's
/// checkbox switches its entry, a field edits its settings, "Add Effect"
/// adds an entry and Remove in a header's menu removes one.
#[test]
fn the_profile_inspector_switches_edits_adds_and_removes_entries() {
    use crate::inspector::test_harness::InspectorHarness;
    use crate::inspector::type_inspector::InspectorResponse;
    use calyx_engine::assets::post_process_profile::ProfileEntry;
    use egui::accesskit::Role;

    let root = TempRoot::new();
    let path = root.0.join("look.cxpostfx");
    PostProcessProfile::from_entries([ProfileEntry::new(bloom(0.5))])
        .to_file(&path)
        .unwrap();
    let mut game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    let id = game
        .assets
        .registries
        .assets
        .read()
        .asset_id_from_path(&path)
        .expect("the profile is registered");
    let profile = game
        .assets
        .registries
        .assets
        .read()
        .load_by_id::<PostProcessProfile>(id)
        .unwrap();
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = PostProcessProfileInspector;
    let mut draw = |ui: &mut Ui, _: &InspectorContext| {
        inspector.show_inspector(ui, &mut game, &registry, id);
        InspectorResponse::NONE
    };
    h.idle(3, &mut draw);

    let checkbox = h.widget(Role::CheckBox, 0);
    h.click(checkbox, &mut draw);
    let bloom_enabled = || {
        profile
            .read()
            .entry(ComponentBloom::type_uuid())
            .map(|entry| entry.enabled)
    };
    assert_eq!(bloom_enabled(), Some(false));
    h.click(checkbox, &mut draw);
    assert_eq!(bloom_enabled(), Some(true));

    let field = h.widget(Role::SpinButton, 0);
    h.drag(field, egui::vec2(40.0, 0.0), &mut draw);
    let edited = *profile
        .read()
        .entry(ComponentBloom::type_uuid())
        .and_then(|entry| entry.settings::<ComponentBloom>())
        .unwrap();
    assert_ne!(edited, bloom(0.5), "dragging a field edits the entry");

    let add = h.labelled_any("Add Effect");
    h.click(add, &mut draw);
    let vignette = h.labelled_any("Vignette");
    h.click(vignette, &mut draw);
    assert_eq!(
        *profile.read(),
        PostProcessProfile::from_entries([
            ProfileEntry::new(edited),
            ProfileEntry::new(ComponentVignette::default()),
        ])
    );
    let header = h.labelled_any("Bloom");
    h.secondary_click(header, &mut draw);
    let remove = h.labelled_any("Remove");
    h.click(remove, &mut draw);
    assert_eq!(
        *profile.read(),
        PostProcessProfile::from_entries([ProfileEntry::new(ComponentVignette::default())]),
        "Remove drops the bloom entry"
    );
}

/// Create Profile checks its destination before writing, so a refused path
/// never overwrites a file: one outside every asset root, or of another type.
#[test]
fn create_profile_refuses_a_destination_outside_the_asset_roots_or_of_another_type() {
    let root = TempRoot::new();
    let outside = TempRoot::new();
    let mut game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    let object = volume_with_effects(&mut game);
    let outside_file = outside.0.join("look.cxpostfx");
    std::fs::write(&outside_file, "keep").unwrap();
    let scene_file = root.0.join("level.cxscene");
    std::fs::write(&scene_file, "keep").unwrap();

    for path in [&outside_file, &scene_file] {
        let mut edit = SceneEdit::begin("Create post-process profile");
        let result =
            PostProcessProfileInspector::create_from_volume(&mut edit, &mut game, object, path);
        edit.cancel();

        assert!(result.is_err(), "{} is refused", path.display());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "keep");
    }
    assert!(
        has::<ComponentBloom>(&game, object),
        "the volume is untouched"
    );
}
