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
        if let Some(visible) = self.cached(game_object) {
            return visible;
        }
        // Walk up iteratively to the first ancestor already read, or the
        // root, so a deep hierarchy cannot overflow the stack.
        let mut unread = vec![game_object];
        let mut visible = true;
        let mut current = game_object;
        while let Some(parent) = scene.parent(current) {
            if let Some(parent_visible) = self.cached(parent) {
                visible = parent_visible;
                break;
            }
            unread.push(parent);
            current = parent;
        }
        // Fill in from the topmost unread ancestor down to `game_object`.
        for object in unread.into_iter().rev() {
            visible = visible && scene.is_visible(object);
            let index = object.node.index();
            if self.visible.len() <= index {
                self.visible.resize(index + 1, None);
            }
            self.visible[index] = Some(visible);
        }
        visible
    }

    fn cached(&self, game_object: GameObject) -> Option<bool> {
        self.visible
            .get(game_object.node.index())
            .copied()
            .flatten()
    }
}

#[cfg(test)]
mod tests;
