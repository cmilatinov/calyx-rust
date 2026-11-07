use super::{GameObject, Scene};

/// Answers [`Scene::is_visible_in_hierarchy`] for many objects of one scene in
/// one pass, reading each object's own visibility once instead of once for
/// every descendant that asks.
///
/// It remembers what it read, so it holds for one pass over a scene that does
/// not change meanwhile: make a new one, or [`Self::clear`] it, for the next.
#[derive(Default)]
pub struct HierarchyVisibility {
    /// Whether each scene-graph node, by index, and all its ancestors are
    /// visible, once read.
    visible: Vec<Option<bool>>,
}

impl HierarchyVisibility {
    /// Forgets everything read, for a pass over a scene that may have changed.
    pub fn clear(&mut self) {
        self.visible.clear();
    }

    /// Whether `game_object` and every ancestor of it are visible in `scene`,
    /// as [`Scene::is_visible_in_hierarchy`] says.
    pub fn is_visible(&mut self, scene: &Scene, game_object: GameObject) -> bool {
        let index = game_object.node.index();
        if let Some(Some(visible)) = self.visible.get(index) {
            return *visible;
        }
        let visible = scene.is_visible(game_object)
            && scene
                .parent(game_object)
                .is_none_or(|parent| self.is_visible(scene, parent));
        if self.visible.len() <= index {
            self.visible.resize(index + 1, None);
        }
        self.visible[index] = Some(visible);
        visible
    }
}

#[cfg(test)]
mod tests {
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
}
