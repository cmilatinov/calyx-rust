use legion::storage::ComponentTypeId;
use legion::Entity;
use std::collections::{HashMap, HashSet};

use crate::component::Component;

/// Records which entities had a component of each type written since a
/// consumer last took that type's entries.
///
/// The scene records every component write here from its one observation
/// point, so components carry no change flag and writers keep no books.
/// Taking a type's entries removes them, so each type supports one consumer.
///
/// Each write also moves the component's write version, which any number of
/// readers can compare against one they saw before, without taking anything.
#[derive(Default)]
pub(crate) struct ChangeSet {
    written: HashMap<ComponentTypeId, HashSet<Entity>>,
    versions: HashMap<(ComponentTypeId, Entity), u64>,
    last_version: u64,
}

impl ChangeSet {
    /// Records that `entity`'s component of type `component` was added,
    /// written or removed.
    pub(crate) fn record(&mut self, entity: Entity, component: ComponentTypeId) {
        self.written.entry(component).or_default().insert(entity);
        self.last_version += 1;
        self.versions.insert((component, entity), self.last_version);
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

    /// Drops every entry for `entity`, once it has left the world.
    pub(crate) fn forget(&mut self, entity: Entity) {
        self.written.retain(|_, entities| {
            entities.remove(&entity);
            !entities.is_empty()
        });
        self.versions.retain(|(_, written), _| *written != entity);
    }
}

#[cfg(test)]
mod tests {
    use super::ChangeSet;
    use crate::component::{ComponentCollider, ComponentRigidBody};
    use legion::storage::ComponentTypeId;
    use legion::World;

    #[test]
    fn take_returns_written_entities_once() {
        let mut world = World::default();
        let a = world.push(());
        let b = world.push(());
        let mut changes = ChangeSet::default();
        changes.record(a, ComponentTypeId::of::<ComponentRigidBody>());
        changes.record(b, ComponentTypeId::of::<ComponentCollider>());

        let bodies = changes.take::<ComponentRigidBody>();
        assert_eq!(bodies.len(), 1);
        assert!(bodies.contains(&a));
        assert!(changes.take::<ComponentRigidBody>().is_empty());
        assert!(changes.take::<ComponentCollider>().contains(&b));
    }

    #[test]
    fn versions_move_on_every_write_and_survive_take() {
        let mut world = World::default();
        let a = world.push(());
        let b = world.push(());
        let body = ComponentTypeId::of::<ComponentRigidBody>();
        let mut changes = ChangeSet::default();
        assert_eq!(changes.version(a, body), 0);

        changes.record(a, body);
        let first = changes.version(a, body);
        assert_ne!(first, 0);
        changes.take::<ComponentRigidBody>();
        assert_eq!(changes.version(a, body), first);
        changes.record(a, body);
        assert_ne!(changes.version(a, body), first);
        assert_eq!(changes.version(b, body), 0);

        changes.forget(a);
        assert_eq!(changes.version(a, body), 0);
    }

    #[test]
    fn forget_drops_an_entity_from_every_type() {
        let mut world = World::default();
        let a = world.push(());
        let mut changes = ChangeSet::default();
        changes.record(a, ComponentTypeId::of::<ComponentRigidBody>());
        changes.record(a, ComponentTypeId::of::<ComponentCollider>());

        changes.forget(a);

        assert!(changes.take::<ComponentRigidBody>().is_empty());
        assert!(changes.take::<ComponentCollider>().is_empty());
    }
}
