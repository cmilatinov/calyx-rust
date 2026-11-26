use super::{ChangeJournal, ChangeSet};
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
fn the_journal_lists_the_changes_since_a_cursor_notes_and_departures_included() {
    let mut world = World::default();
    let a = world.push(());
    let b = world.push(());
    let body = ComponentTypeId::of::<ComponentRigidBody>();
    let collider = ComponentTypeId::of::<ComponentCollider>();
    let mut changes = ChangeSet::default();
    let start = changes.journal().end();
    changes.record(a, body);
    let middle = changes.journal().end();
    changes.note(b, collider);
    changes.forget(a);

    let all: Vec<_> = changes.journal().since(start).unwrap().collect();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0], (a, body));
    assert_eq!(all[1], (b, collider));
    assert_eq!(all[2].0, a);
    assert_eq!(changes.journal().since(middle).unwrap().count(), 2);
    let end = changes.journal().end();
    assert_eq!(changes.journal().since(end).unwrap().count(), 0);
    // A note is no write: it moves no version and never reaches `take`.
    assert_eq!(changes.version(b, collider), 0);
    assert!(changes.take::<ComponentCollider>().is_empty());
}

#[test]
fn a_reader_that_fell_behind_the_journal_is_told_to_start_over() {
    let mut world = World::default();
    let a = world.push(());
    let body = ComponentTypeId::of::<ComponentRigidBody>();
    let mut changes = ChangeSet::default();
    let start = changes.journal().end();
    for _ in 0..=ChangeJournal::CAPACITY {
        changes.note(a, body);
    }
    assert!(changes.journal().since(start).is_none());
    let end = changes.journal().end();
    assert_eq!(end, ChangeJournal::CAPACITY as u64 + 1);
    assert_eq!(changes.journal().since(end - 1).unwrap().count(), 1);
    assert!(changes.journal().since(end + 1).is_none());
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
