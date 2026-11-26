use legion::storage::ComponentTypeId;
use legion::Entity;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::component::{Component, ComponentID};

/// Records which entities had a component of each type written since a
/// consumer last took that type's entries.
///
/// The scene records every component write here from its one observation
/// point, so components carry no change flag and writers keep no books.
/// Taking a type's entries removes them, so each type supports one consumer.
///
/// Each write also moves the component's write version, which any number of
/// readers can compare against one they saw before, without taking anything,
/// and joins the scene's [`ChangeJournal`], which any number of readers
/// follow from a cursor of their own, also without taking anything.
#[derive(Default)]
pub(crate) struct ChangeSet {
    written: HashMap<ComponentTypeId, HashSet<Entity>>,
    versions: HashMap<(ComponentTypeId, Entity), u64>,
    last_version: u64,
    journal: ChangeJournal,
}

/// The scene's changes in the order they happened, each an entity and the
/// component type written, so a reader that keeps state per object, such as
/// the draw collection, follows the scene by reading what changed since its
/// last read instead of rescanning every object.
///
/// A reader keeps a cursor, the [`Self::end`] of its last read. The journal
/// holds its latest [`Self::CAPACITY`] entries: a reader whose cursor fell
/// further behind gets `None` from [`Self::since`] and starts over from the
/// scene as it is.
#[derive(Default)]
pub(crate) struct ChangeJournal {
    /// The cursor of `entries[0]`.
    start: u64,
    entries: VecDeque<(Entity, ComponentTypeId)>,
}

impl ChangeJournal {
    /// How many entries the journal holds.
    pub(crate) const CAPACITY: usize = 1 << 16;

    fn push(&mut self, entity: Entity, component: ComponentTypeId) {
        if self.entries.len() == Self::CAPACITY {
            self.entries.pop_front();
            self.start += 1;
        }
        self.entries.push_back((entity, component));
    }

    /// The cursor after the latest entry.
    pub(crate) fn end(&self) -> u64 {
        self.start + self.entries.len() as u64
    }

    /// The entries after `cursor`, oldest first, or `None` when the journal
    /// no longer holds them all.
    pub(crate) fn since(
        &self,
        cursor: u64,
    ) -> Option<impl Iterator<Item = (Entity, ComponentTypeId)> + '_> {
        if cursor < self.start || cursor > self.end() {
            return None;
        }
        Some(
            self.entries
                .iter()
                .skip((cursor - self.start) as usize)
                .copied(),
        )
    }
}

impl ChangeSet {
    /// Records that `entity`'s component of type `component` was added,
    /// written or removed.
    pub(crate) fn record(&mut self, entity: Entity, component: ComponentTypeId) {
        self.written.entry(component).or_default().insert(entity);
        self.last_version += 1;
        self.versions.insert((component, entity), self.last_version);
        self.journal.push(entity, component);
    }

    /// Adds a change to the journal only, for a change that is no component
    /// write, such as a move to another parent, which changes what the
    /// object's `component` means without writing it.
    pub(crate) fn note(&mut self, entity: Entity, component: ComponentTypeId) {
        self.journal.push(entity, component);
    }

    /// The scene's changes in order; see [`ChangeJournal`].
    pub(crate) fn journal(&self) -> &ChangeJournal {
        &self.journal
    }

    /// The write version of `entity`'s component of type `component`: it
    /// changes on every write and is 0 when none was recorded. Versions are
    /// unique across the scene, so a forgotten and reused entity never
    /// repeats one.
    pub(crate) fn version(&self, entity: Entity, component: ComponentTypeId) -> u64 {
        self.versions
            .get(&(component, entity))
            .copied()
            .unwrap_or_default()
    }

    /// Removes and returns the entities whose `T` was written since the last
    /// take of `T`.
    pub(crate) fn take<T: Component + 'static>(&mut self) -> HashSet<Entity> {
        self.written
            .remove(&ComponentTypeId::of::<T>())
            .unwrap_or_default()
    }

    /// Drops every entry for `entity`, once it has left the world. The
    /// journal keeps its entries and gains one, of the entity's
    /// [`ComponentID`], so its readers learn that the entity left.
    pub(crate) fn forget(&mut self, entity: Entity) {
        self.journal
            .push(entity, ComponentTypeId::of::<ComponentID>());
        self.written.retain(|_, entities| {
            entities.remove(&entity);
            !entities.is_empty()
        });
        self.versions.retain(|(_, written), _| *written != entity);
    }
}

#[cfg(test)]
mod tests;
