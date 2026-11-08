//! The channel from game code to whatever hosts the game.

use crate::resource::Resource;

/// A request from game code to the host running it: the editor's play mode
/// or the standalone player.
///
/// This is the single channel for such requests. Game code pushes them into
/// the [`HostCommands`] resource during a scene update, and the host gets the
/// frame's commands back from
/// [`GameContext::update_scene`](crate::context::GameContext::update_scene)
/// and handles each one. Hosts match on it without a catch-all arm, so a new
/// variant (loading a scene, showing the cursor, ...) makes every host decide
/// what it does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostCommand {
    /// Leave play: the editor stops the simulation, as its Stop button does,
    /// and the player closes its window.
    Quit,
}

/// The commands game code issued this frame, in order, until the update
/// returns them to the host.
#[derive(Debug, Default)]
pub struct HostCommands {
    commands: Vec<HostCommand>,
}

impl Resource for HostCommands {}

impl HostCommands {
    /// Queues `command` for the host.
    pub fn push(&mut self, command: HostCommand) {
        self.commands.push(command);
    }

    /// Removes and returns the queued commands, oldest first.
    pub fn drain(&mut self) -> Vec<HostCommand> {
        std::mem::take(&mut self.commands)
    }
}
