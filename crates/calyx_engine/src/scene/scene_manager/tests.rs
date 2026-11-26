use crate::component::ComponentID;
use crate::core::Ref;
use crate::test_support::{test_game_context, test_scene};

#[test]
fn suspending_scenes_includes_a_default_scene_the_cache_evicted() {
    let mut game = test_game_context();
    // A default scene the asset registry no longer caches, as after its
    // file was deleted, which the manager still holds.
    let mut default_scene = test_scene();
    default_scene.create(
        Some(ComponentID {
            name: "Default".into(),
            ..Default::default()
        }),
        None,
    );
    let default_scene = Ref::new(default_scene);
    game.scenes.default_scene = default_scene.clone();
    let objects = |scene: &Ref<crate::scene::Scene>| scene.read().objects().count();
    assert_eq!(objects(&default_scene), 1);

    let suspended = game.scenes.suspend_scenes();
    assert_eq!(objects(&default_scene), 0);

    game.scenes.resume_scenes(suspended);
    assert_eq!(objects(&default_scene), 1);
}
