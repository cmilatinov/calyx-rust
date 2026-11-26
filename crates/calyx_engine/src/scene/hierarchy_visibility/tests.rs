use super::HierarchyVisibility;
use crate::component::ComponentID;
use crate::test_support::test_scene;

#[test]
fn agrees_with_the_scene_for_every_object_of_a_hierarchy() {
    let mut scene = test_scene();
    let root = scene.create(None, None);
    let hidden = scene.create(None, Some(root));
    let under_hidden = scene.create(None, Some(hidden));
    let shown = scene.create(None, Some(root));
    let leaf = scene.create(None, Some(shown));
    let _ = scene.write_component::<ComponentID, _>(hidden, |id| id.visible = false);

    let mut visibility = HierarchyVisibility::default();
    // Ask leaves first, so their answers fill in their ancestors'.
    for object in [under_hidden, leaf, hidden, shown, root] {
        assert_eq!(
            visibility.is_visible(&scene, object),
            scene.is_visible_in_hierarchy(object),
            "object {object:?}"
        );
    }
    assert!(!visibility.is_visible(&scene, under_hidden));
    assert!(visibility.is_visible(&scene, leaf));
}

#[test]
fn answers_for_a_hierarchy_deeper_than_the_stack_allows_recursion() {
    // A small stack and a deep chain: one stack frame per ancestor would
    // overflow it, so this goes red if the walk recurses.
    let result = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut scene = test_scene();
            let root = scene.create(None, None);
            let _ = scene.write_component::<ComponentID, _>(root, |id| id.visible = false);
            let mut leaf = root;
            for _ in 0..20_000 {
                leaf = scene.create(None, Some(leaf));
            }
            let mut visibility = HierarchyVisibility::default();
            (
                visibility.is_visible(&scene, leaf),
                scene.is_visible_in_hierarchy(leaf),
            )
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(result, (false, false));
}
