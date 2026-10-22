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
#[derive(Default)]
pub(crate) struct ChangeSet {
    written: HashMap<ComponentTypeId, HashSet<Entity>>,
}

impl ChangeSet {
    /// Records that `entity`'s component of type `component` was added,
    /// written or removed.
    pub(crate) fn record(&mut self, entity: Entity, component: ComponentTypeId) {
        self.written.entry(component).or_default().insert(entity);
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
