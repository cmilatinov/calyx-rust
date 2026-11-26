use super::{document_label, scene_view_backdrop, PREFAB_MODE_BACKDROP, SCENE_VIEW_BACKDROP};
use crate::ActiveSceneState;

#[test]
fn prefab_mode_title_names_the_prefab_asset() {
    let mut state = ActiveSceneState::default();

    state.set_scene(Some("scene".into()), None);
    assert_eq!(
        state.title(
            "Sandbox",
            Some(&document_label("prefabs/crate.cxprefab")),
            true
        ),
        "Calyx - Sandbox - Prefab: prefabs/crate.cxprefab *"
    );
}

#[test]
fn only_prefab_mode_turns_the_scene_view_backdrop_blue() {
    assert_eq!(scene_view_backdrop(true), PREFAB_MODE_BACKDROP);
    assert_eq!(scene_view_backdrop(false), SCENE_VIEW_BACKDROP);
    let [r, g, b, _] = PREFAB_MODE_BACKDROP.to_array();
    assert!(b > g && g > r, "the prefab backdrop is blue");
}
