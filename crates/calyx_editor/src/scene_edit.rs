//! The transaction every editor write to the scene goes through.
//!
//! A [`SceneEdit`] begins with a label and touches nothing. The first write
//! through it captures the authoring scene, so an edit that writes nothing,
//! such as a menu left open, costs nothing. Committing records everything
//! written since as one undo step. While a simulation copy of the scene
//! exists, writes go to that copy and the edit records nothing.

use std::ops::Deref;

use calyx_engine::context::GameContext;
use calyx_engine::scene::Scene;

use crate::scene_document::SceneDocument;
use crate::scene_history::SceneEditSnapshot;

/// One undoable edit of the scene the editor shows. Ends with
/// [`Self::commit`] or [`Self::cancel`].
#[must_use = "a scene edit is recorded only when it is committed"]
pub struct SceneEdit {
    label: String,
    before: Option<SceneEditSnapshot>,
}

impl SceneEdit {
    pub fn begin(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            before: None,
        }
    }

    /// The scene to write: the authoring scene, captured before the edit's
    /// first write, or the simulation scene while one exists.
    pub fn scene_mut<'a>(&mut self, game: &'a mut GameContext) -> &'a mut Scene {
        self.capture(game);
        game.scenes.simulation_scene_mut()
    }

    /// The scene to read and write while `game` is borrowed: reads go
    /// through `Deref`, writes through [`SceneEditScene::get_mut`].
    pub fn scene<'a>(&'a mut self, game: &'a mut GameContext) -> SceneEditScene<'a> {
        SceneEditScene { edit: self, game }
    }

    /// Captures the authoring scene now, for writes that do not go through
    /// the edit, such as a remote command's.
    pub fn capture(&mut self, game: &GameContext) {
        if self.before.is_none() {
            self.before = SceneDocument::begin_edit(game);
        }
    }

    /// Records the edit as one undo step. An edit that wrote nothing, or
    /// wrote only the simulation scene, records nothing.
    pub fn commit(self, document: &mut SceneDocument, game: &GameContext) {
        if let Some(before) = self.before {
            document.commit_edit(game, self.label, before);
        }
    }

    /// Ends the edit without recording it. What it wrote stays in the scene.
    pub fn cancel(self) {}
}

/// A [`SceneEdit`]'s scene while the game is borrowed.
pub struct SceneEditScene<'a> {
    edit: &'a mut SceneEdit,
    game: &'a mut GameContext,
}

impl SceneEditScene<'_> {
    /// The scene to write, as [`SceneEdit::scene_mut`] gives it.
    pub fn get_mut(&mut self) -> &mut Scene {
        self.edit.scene_mut(self.game)
    }
}

impl Deref for SceneEditScene<'_> {
    type Target = Scene;

    fn deref(&self) -> &Scene {
        self.game.scenes.simulation_scene()
    }
}

#[cfg(test)]
mod tests;
