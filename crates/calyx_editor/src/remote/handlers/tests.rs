use super::*;
use crate::remote::runtime::tests::test_runtime;
use calyx_engine::component::{
    ComponentBone, ComponentCamera, ComponentMesh, ComponentPointLight, ComponentTransform,
};

#[test]
fn a_game_capture_renders_only_while_the_game_panel_has_no_size() {
    assert_eq!(
        game_capture_size(None, (800, 600), (1200, 700), (1920, 1080), 8192).unwrap(),
        None
    );
    assert_eq!(
        game_capture_size(None, (0, 0), (1200, 700), (1920, 1080), 8192).unwrap(),
        Some((1200, 700))
    );
    assert_eq!(
        game_capture_size(None, (0, 0), (0, 0), (1920, 1080), 8192).unwrap(),
        Some((1920, 1080))
    );
}

#[test]
fn a_requested_capture_size_wins_over_every_panel_size() {
    for game_panel in [(800, 600), (0, 0)] {
        assert_eq!(
            game_capture_size(Some((1920, 1080)), game_panel, (0, 0), (0, 0), 8192).unwrap(),
            Some((1920, 1080))
        );
    }
    assert_eq!(
        game_capture_size(Some((8192, 2)), (0, 0), (0, 0), (0, 0), 8192).unwrap(),
        Some((8192, 2))
    );
}

#[test]
fn a_requested_capture_size_out_of_range_is_a_bad_request() {
    for requested in [
        (0, 1080),
        (1920, 0),
        (1, 1080),
        (8193, 1080),
        (1920, 100_000),
    ] {
        let (kind, message) =
            game_capture_size(Some(requested), (800, 600), (0, 0), (0, 0), 8192).unwrap_err();
        assert_eq!(kind, ErrorKind::BadRequest);
        assert!(message.contains("2 to 8192 pixels"), "{message}");
    }
}

#[test]
fn a_game_capture_with_nothing_to_size_it_names_focus_game() {
    let (kind, message) = game_capture_size(None, (0, 0), (1, 1), (0, 0), 8192).unwrap_err();
    assert_eq!(kind, ErrorKind::BadRequest);
    assert!(message.contains("focus-game"), "{message}");
}

#[test]
fn setting_a_parent_transform_moves_the_child() {
    use nalgebra_glm as glm;
    let registries = calyx_engine::test_support::test_registries();
    let mut scene = registries.scene();
    let parent = scene.create(None, None);
    let child = scene.create(None, Some(parent));
    scene.set_transform(parent, &glm::translation(&Vec3::new(1.0, 0.0, 0.0)));
    scene.set_transform(child, &glm::translation(&Vec3::new(0.0, 1.0, 0.0)));
    assert!((scene.world_transform(child).position - Vec3::new(1.0, 1.0, 0.0)).norm() < 1e-5);

    let component_registry = registries.components.read();
    let prototype = component_registry
        .component(ComponentTransform::type_uuid())
        .expect("transform should be registered");
    let value = serde_json::json!({
        "transform": {
            "position": [5.0, 0.0, 0.0],
            "rotation": [0.0, 0.0, 0.0, 1.0],
            "scale": [1.0, 1.0, 1.0],
        }
    });
    set_component_value(
        &mut scene,
        parent,
        ComponentTransform::type_uuid(),
        prototype,
        &value,
    )
    .expect("the transform should be set");

    let child_world = scene.world_transform(child).position;
    assert!(
        (child_world - Vec3::new(5.0, 1.0, 0.0)).norm() < 1e-5,
        "child world position {child_world:?}"
    );
}

#[test]
fn disabling_a_component_that_cannot_be_disabled_is_a_bad_request() {
    let registries = calyx_engine::test_support::test_registries();
    let components = registries.components.read();
    for (type_uuid, name) in [
        (ComponentCamera::type_uuid(), "camera"),
        (ComponentMesh::type_uuid(), "mesh"),
        (ComponentPointLight::type_uuid(), "point light"),
    ] {
        assert!(ensure_can_disable(&components, type_uuid, name).is_ok());
    }
    for (type_uuid, name) in [
        (ComponentBone::type_uuid(), "bone"),
        (ComponentTransform::type_uuid(), "transform"),
    ] {
        let (kind, message) = ensure_can_disable(&components, type_uuid, name).unwrap_err();
        assert_eq!(kind, ErrorKind::BadRequest);
        assert!(
            message.contains(&format!("{name} cannot be disabled")),
            "{message}"
        );
    }
}

#[test]
fn deleting_takes_effect_before_the_edit_is_snapshotted() {
    let mut scene = calyx_engine::test_support::test_scene();
    let parent = scene.create(None, None);
    let child = scene.create(None, Some(parent));
    let ids = [scene.uuid(parent), scene.uuid(child)];
    let before = crate::scene_history::SceneEditSnapshot::capture(&scene);

    let removed = delete_now(&mut scene, parent);

    let after = crate::scene_history::SceneEditSnapshot::capture(&scene);
    assert_ne!(
        before.fingerprint(),
        after.fingerprint(),
        "the undo entry would record no change"
    );
    assert_eq!(removed, ids);
    assert!(ids.iter().all(|id| scene.find(*id).is_none()));
}

#[test]
fn oversized_waits_are_rejected_instead_of_allocating() {
    let runtime = test_runtime();
    let events = [InputEventSpec::Wait { frames: u32::MAX }];
    let (kind, message) = compile_input_events(&events, &runtime)
        .expect_err("an unbounded wait must not allocate a bucket per frame");
    assert_eq!(kind, ErrorKind::BadRequest);
    assert!(message.contains("frames"), "unexpected message: {message}");
}

#[test]
fn oversized_key_holds_are_rejected() {
    let runtime = test_runtime();
    let events = [InputEventSpec::KeyPress {
        key: "W".to_string(),
        hold_frames: Some(u32::MAX),
    }];
    let (kind, _) = compile_input_events(&events, &runtime)
        .expect_err("an unbounded hold must not allocate a bucket per frame");
    assert_eq!(kind, ErrorKind::BadRequest);
}

#[test]
fn key_press_schedules_the_release_after_the_hold() {
    let runtime = test_runtime();
    let events = [InputEventSpec::KeyPress {
        key: "W".to_string(),
        hold_frames: Some(3),
    }];
    let buckets = compile_input_events(&events, &runtime).expect("script should compile");
    assert_eq!(buckets.len(), 4, "press at 0, release at 3");
    assert_eq!(buckets[0].len(), 1);
    assert_eq!(buckets[3].len(), 1);
    assert!(buckets[1].is_empty() && buckets[2].is_empty());
}

#[test]
fn a_trailing_wait_extends_the_schedule() {
    let runtime = test_runtime();
    let events = [
        InputEventSpec::KeyDown {
            key: "W".to_string(),
            modifiers: None,
        },
        InputEventSpec::Wait { frames: 4 },
    ];
    let buckets = compile_input_events(&events, &runtime).expect("script should compile");
    // The response must not land before the waited-out frames elapse.
    assert_eq!(buckets.len(), 4);
    assert_eq!(buckets[0].len(), 1);
}

/// A scratch asset root holding a crate prefab, a "Crate" at z = 7 with a
/// "Lid" child, removed on drop.
struct PrefabRoot(std::path::PathBuf);

impl PrefabRoot {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("calyx-remote-prefab-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("asset root should be created");
        let root = Self(dunce::canonicalize(&dir).expect("asset root should canonicalize"));
        let mut authoring = calyx_engine::test_support::test_game_context();
        let scene = authoring.scenes.current_scene_mut();
        let body = named(scene, "Crate", None);
        named(scene, "Lid", Some(body));
        scene.set_transform(body, &nalgebra_glm::translation(&Vec3::new(0.0, 0.0, 7.0)));
        let prefab = scene.create_prefab(body);
        crate::scene_file::write_prefab_file(&root.prefab_file(), &prefab)
            .expect("prefab should be written");
        root
    }

    fn prefab_file(&self) -> std::path::PathBuf {
        self.0.join("crate.cxprefab")
    }

    /// A game whose assets include the prefab, and the prefab's asset id.
    fn game(&self) -> (GameContext, Uuid) {
        let game = GameContext::new(calyx_engine::test_support::test_asset_context_with_assets(
            vec![self.0.clone()],
        ));
        let asset_id = game
            .assets
            .registries
            .assets
            .read()
            .asset_id_from_path(&self.prefab_file())
            .expect("prefab should be registered");
        (game, asset_id)
    }
}

impl Drop for PrefabRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn named(scene: &mut Scene, name: &str, parent: Option<GameObject>) -> GameObject {
    let id = ComponentID {
        name: name.into(),
        ..Default::default()
    };
    scene.create(Some(id), parent)
}

fn document() -> crate::scene_document::SceneDocument {
    crate::scene_document::SceneDocument::new(std::path::Path::new("remote-test-project"))
}

fn near(actual: Vec3, expected: [f32; 3]) -> bool {
    (actual - Vec3::new(expected[0], expected[1], expected[2])).norm() < 1e-4
}

/// Runs `command` on the scene as the remote handler runs a scene
/// command: inside one scene edit, committed when it succeeds.
fn as_one_edit(
    game: &mut GameContext,
    document: &mut crate::scene_document::SceneDocument,
    command: impl FnOnce(&mut Scene) -> CommandResult,
) -> Result<serde_json::Value, (ErrorKind, String)> {
    let mut edit = SceneEdit::begin("Remote test edit");
    edit.capture(game);
    let result = command(game.scenes.simulation_scene_mut());
    match result {
        Ok(_) => edit.commit(document, game),
        Err(_) => edit.cancel(),
    }
    result.map(|data| data.expect("a scene command responds at once"))
}

/// A "House" at x = 10 turned 90 degrees about y, and a "Roof" at the
/// scene root at (0, 5, 0).
fn house_and_roof(game: &mut GameContext) -> (GameObject, GameObject) {
    let scene = game.scenes.current_scene_mut();
    let house = named(scene, "House", None);
    let transform = calyx_engine::math::Transform {
        position: Vec3::new(10.0, 0.0, 0.0),
        rotation: UnitQuaternion::from_euler_angles(0.0, 90f32.to_radians(), 0.0),
        ..Default::default()
    };
    scene.set_transform(house, &transform.matrix());
    let roof = named(scene, "Roof", None);
    scene.set_transform(roof, &nalgebra_glm::translation(&Vec3::new(0.0, 5.0, 0.0)));
    (house, roof)
}

#[test]
fn a_prefab_instantiates_under_a_parent_at_a_local_transform() {
    let root = PrefabRoot::new();
    let (mut game, asset_id) = root.game();
    let mut document = document();
    let house = named(game.scenes.current_scene_mut(), "House", None);
    game.scenes.current_scene_mut().set_transform(
        house,
        &nalgebra_glm::translation(&Vec3::new(10.0, 0.0, 0.0)),
    );

    let mut edit = SceneEdit::begin("Remote instantiate prefab");
    let data = instantiate_prefab_under(
        &mut edit,
        &mut game,
        asset_id,
        Some("House"),
        [
            Some([1.0, 0.0, 0.0]),
            Some([0.0, 90.0, 0.0]),
            Some([2.0, 2.0, 2.0]),
        ],
    )
    .expect("the prefab should instantiate");
    edit.commit(&mut document, &game);

    let scene = game.scenes.current_scene();
    let id: Uuid = data["id"].as_str().unwrap().parse().unwrap();
    let instance = scene.find(id).expect("the instance exists");
    assert_eq!(scene.name(instance), "Crate");
    assert_eq!(scene.parent(instance), Some(house));
    let local = scene.transform(instance);
    assert!(near(local.position, [1.0, 0.0, 0.0]), "{local:?}");
    assert!(near(local.scale, [2.0, 2.0, 2.0]), "{local:?}");
    // 90 degrees about y turns +x into -z.
    assert!(
        near(local.rotation * Vec3::x(), [0.0, 0.0, -1.0]),
        "{local:?}"
    );
    assert!(near(
        scene.world_transform(instance).position,
        [11.0, 0.0, 0.0]
    ));
    assert!(
        scene
            .children(instance)
            .any(|child| scene.name(child) == "Lid"),
        "the instance holds the prefab's objects"
    );

    // The instance is one undo step.
    document.undo(&mut game);
    assert!(game.scenes.current_scene().find(id).is_none());
}

#[test]
fn a_prefab_keeps_its_root_transform_where_none_is_given() {
    let root = PrefabRoot::new();
    let (mut game, asset_id) = root.game();
    let mut edit = SceneEdit::begin("Remote instantiate prefab");
    let data = instantiate_prefab_under(
        &mut edit,
        &mut game,
        asset_id,
        None,
        [None, None, Some([3.0, 3.0, 3.0])],
    )
    .expect("the prefab should instantiate");
    edit.cancel();

    let scene = game.scenes.current_scene();
    let instance = scene
        .find(data["id"].as_str().unwrap().parse().unwrap())
        .unwrap();
    assert_eq!(
        scene.parent(instance),
        Some(scene.root()),
        "at the scene root"
    );
    let local = scene.transform(instance);
    assert!(near(local.position, [0.0, 0.0, 7.0]), "{local:?}");
    assert!(near(local.scale, [3.0, 3.0, 3.0]), "{local:?}");
}

#[test]
fn a_prefab_under_an_unknown_parent_instantiates_nothing() {
    let root = PrefabRoot::new();
    let (mut game, asset_id) = root.game();
    let before = game.scenes.current_scene().objects().count();
    let mut edit = SceneEdit::begin("Remote instantiate prefab");
    let (kind, message) =
        instantiate_prefab_under(&mut edit, &mut game, asset_id, Some("Nowhere"), [None; 3])
            .expect_err("an unknown parent is refused");
    edit.cancel();

    assert_eq!(kind, ErrorKind::NotFound);
    assert!(message.contains("Nowhere"), "{message}");
    assert_eq!(game.scenes.current_scene().objects().count(), before);
}

#[test]
fn reparenting_keeps_the_world_transform_as_one_undo_step() {
    let mut game = calyx_engine::test_support::test_game_context();
    let mut document = document();
    let (house, roof) = house_and_roof(&mut game);

    let data = as_one_edit(&mut game, &mut document, |scene| {
        set_parent_keeping_world(scene, "Roof", Some("House"))
    })
    .expect("the roof should move under the house");

    let scene = game.scenes.current_scene();
    assert_eq!(scene.parent(roof), Some(house));
    assert!(near(scene.world_transform(roof).position, [0.0, 5.0, 0.0]));
    // (-10, 5, 0) from the house, which is turned 90 degrees about y, is
    // (0, 5, -10) in the house's frame.
    assert!(
        near(scene.transform(roof).position, [0.0, 5.0, -10.0]),
        "{:?}",
        scene.transform(roof)
    );
    assert_eq!(data["parent"], scene.uuid(house).to_string());

    document.undo(&mut game);
    let scene = game.scenes.current_scene();
    let roof = scene
        .objects()
        .find(|object| scene.name(*object) == "Roof")
        .unwrap();
    assert_eq!(
        scene.parent(roof),
        Some(scene.root()),
        "undo puts the roof back at the root"
    );
    assert!(near(scene.transform(roof).position, [0.0, 5.0, 0.0]));
}

#[test]
fn reparenting_to_the_root_keeps_the_world_transform() {
    let mut game = calyx_engine::test_support::test_game_context();
    let mut document = document();
    let (house, _) = house_and_roof(&mut game);
    let chimney = named(game.scenes.current_scene_mut(), "Chimney", Some(house));
    game.scenes.current_scene_mut().set_transform(
        chimney,
        &nalgebra_glm::translation(&Vec3::new(1.0, 2.0, 0.0)),
    );
    let world = game
        .scenes
        .current_scene()
        .world_transform(chimney)
        .position;
    // The house turns the chimney's local +x into world -z.
    assert!(near(world, [10.0, 2.0, -1.0]), "{world:?}");

    let data = as_one_edit(&mut game, &mut document, |scene| {
        set_parent_keeping_world(scene, "Chimney", None)
    })
    .expect("the chimney should move to the root");

    let scene = game.scenes.current_scene();
    assert_eq!(scene.parent(chimney), Some(scene.root()));
    assert!(near(scene.transform(chimney).position, [10.0, 2.0, -1.0]));
    assert_eq!(data["parent"], scene.uuid(scene.root()).to_string());
}

#[test]
fn reparenting_under_itself_or_a_descendant_is_refused() {
    let mut game = calyx_engine::test_support::test_game_context();
    let mut document = document();
    let (house, roof) = house_and_roof(&mut game);
    game.scenes
        .current_scene_mut()
        .set_parent(roof, Some(house))
        .unwrap();

    for parent in ["Roof", "House"] {
        let (kind, message) = as_one_edit(&mut game, &mut document, |scene| {
            set_parent_keeping_world(scene, "House", Some(parent))
        })
        .expect_err("a cycle is refused");
        assert_eq!(kind, ErrorKind::BadRequest);
        assert!(
            message.contains("\"House\"") && message.contains(&format!("\"{parent}\"")),
            "{message}"
        );
    }
    let scene = game.scenes.current_scene();
    assert_eq!(scene.parent(house), Some(scene.root()));
    assert_eq!(scene.parent(roof), Some(house));
    assert!(!document.can_undo(&game), "a refused move records nothing");
}

/// Under a parent with non-uniform scale, an object turned off the
/// parent's axes would need shear, which a local transform cannot hold.
#[test]
fn reparenting_that_would_shear_the_object_is_refused() {
    let mut game = calyx_engine::test_support::test_game_context();
    let mut document = document();
    let scene = game.scenes.current_scene_mut();
    let stretched = named(scene, "Stretched", None);
    scene.set_transform(stretched, &nalgebra_glm::scaling(&Vec3::new(2.0, 1.0, 1.0)));
    let turn = |degrees: f32| calyx_engine::math::Transform {
        rotation: UnitQuaternion::from_euler_angles(0.0, 0.0, degrees.to_radians()),
        ..Default::default()
    };
    let tilted = named(scene, "Tilted", None);
    scene.set_transform(tilted, &turn(45.0).matrix());
    let square = named(scene, "Square", None);
    scene.set_transform(square, &turn(90.0).matrix());
    // Far from the origin, the translation must not hide the shear.
    let far = named(scene, "Far", None);
    let far_transform = calyx_engine::math::Transform {
        position: Vec3::new(10000.0, 0.0, 0.0),
        ..turn(45.0)
    };
    scene.set_transform(far, &far_transform.matrix());
    // A long axis must not hide the shear of the short ones either.
    let long = named(scene, "Long", None);
    let long_transform = calyx_engine::math::Transform {
        scale: Vec3::new(1.0, 1.0, 10000.0),
        ..turn(45.0)
    };
    scene.set_transform(long, &long_transform.matrix());

    for object in ["Tilted", "Far", "Long"] {
        let (kind, message) = as_one_edit(&mut game, &mut document, |scene| {
            set_parent_keeping_world(scene, object, Some("Stretched"))
        })
        .expect_err("a sheared object is refused");
        assert_eq!(kind, ErrorKind::BadRequest);
        assert!(
            message.contains(&format!("\"{object}\"")) && message.contains("\"Stretched\""),
            "{message}"
        );
    }
    let scene = game.scenes.current_scene();
    assert_eq!(scene.parent(tilted), Some(scene.root()));
    assert_eq!(scene.parent(far), Some(scene.root()));
    assert_eq!(scene.parent(long), Some(scene.root()));
    assert!(!document.can_undo(&game), "a refused move records nothing");

    // A turn that stays on the parent's axes needs no shear.
    as_one_edit(&mut game, &mut document, |scene| {
        set_parent_keeping_world(scene, "Square", Some("Stretched"))
    })
    .expect("an object on the parent's axes moves");
    let scene = game.scenes.current_scene();
    assert_eq!(scene.parent(square), Some(stretched));
    let world = scene.world_transform(square);
    assert!(
        near(world.rotation * Vec3::x(), [0.0, 1.0, 0.0]),
        "{world:?}"
    );
    assert!(near(world.scale, [1.0, 1.0, 1.0]), "{world:?}");
}

/// A parent scaled to zero on an axis flattens anything under it, so no
/// local transform can keep the object's world transform.
#[test]
fn reparenting_under_a_zero_scale_parent_is_refused() {
    let mut game = calyx_engine::test_support::test_game_context();
    let mut document = document();
    let (_, roof) = house_and_roof(&mut game);
    let scene = game.scenes.current_scene_mut();
    let flat = named(scene, "Flat", None);
    scene.set_transform(flat, &nalgebra_glm::scaling(&Vec3::new(1.0, 0.0, 1.0)));

    let (kind, message) = as_one_edit(&mut game, &mut document, |scene| {
        set_parent_keeping_world(scene, "Roof", Some("Flat"))
    })
    .expect_err("a zero-scale parent is refused");
    assert_eq!(kind, ErrorKind::BadRequest);
    assert!(message.contains("zero scale"), "{message}");
    let scene = game.scenes.current_scene();
    assert_eq!(scene.parent(roof), Some(scene.root()));
    assert!(near(scene.transform(roof).position, [0.0, 5.0, 0.0]));
    assert!(!document.can_undo(&game), "a refused move records nothing");
}

/// Rounding grows with the parent's distance from the origin, so an
/// object near the origin still moves under a distant parent.
#[test]
fn reparenting_under_a_distant_parent_keeps_the_world_transform() {
    let mut game = calyx_engine::test_support::test_game_context();
    let mut document = document();
    let scene = game.scenes.current_scene_mut();
    let distant = named(scene, "Distant", None);
    let transform = calyx_engine::math::Transform {
        position: Vec3::new(123456.7, -98765.4, 54321.1),
        rotation: UnitQuaternion::from_euler_angles(0.3, 1.1, -0.7),
        ..Default::default()
    };
    scene.set_transform(distant, &transform.matrix());
    let pebble = named(scene, "Pebble", None);
    scene.set_transform(
        pebble,
        &nalgebra_glm::translation(&Vec3::new(0.0, 0.0, 0.0)),
    );

    as_one_edit(&mut game, &mut document, |scene| {
        set_parent_keeping_world(scene, "Pebble", Some("Distant"))
    })
    .expect("the pebble should move under the distant parent");

    let scene = game.scenes.current_scene();
    assert_eq!(scene.parent(pebble), Some(distant));
    let world = scene.world_transform(pebble).position;
    assert!(world.norm() < 0.05, "{world:?}");
}

#[test]
fn reparenting_an_unknown_object_or_parent_is_not_found() {
    let mut game = calyx_engine::test_support::test_game_context();
    house_and_roof(&mut game);
    for (object, parent) in [("Nowhere", Some("House")), ("Roof", Some("Nowhere"))] {
        let (kind, message) =
            set_parent_keeping_world(game.scenes.current_scene_mut(), object, parent)
                .expect_err("an unknown selector is refused");
        assert_eq!(kind, ErrorKind::NotFound);
        assert!(message.contains("Nowhere"), "{message}");
    }
}
