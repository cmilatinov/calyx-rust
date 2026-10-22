use legion::Entity;
use std::collections::HashMap;

/// Last component value a system applied to its external object, per entity.
///
/// A system that mirrors component data into another world (physics, audio,
/// rendering) asks [`AppliedState::take_if_changed`] each sync; it returns
/// true only when the component differs from what was last applied,
/// regardless of who wrote it.
pub struct AppliedState<T: Clone + PartialEq> {
    applied: HashMap<Entity, T>,
}

impl<T: Clone + PartialEq> Default for AppliedState<T> {
    fn default() -> Self {
        Self {
            applied: HashMap::new(),
        }
    }
}

impl<T: Clone + PartialEq> AppliedState<T> {
    /// Returns true and records `current` when it differs from the value last
    /// recorded for `entity`, or when `entity` has no recorded value yet.
    pub fn take_if_changed(&mut self, entity: Entity, current: &T) -> bool {
        if self.applied.get(&entity) == Some(current) {
            return false;
        }
        self.applied.insert(entity, current.clone());
        true
    }

    /// Drops the value recorded for `entity`, so its next check reports a change.
    pub fn forget(&mut self, entity: Entity) {
        self.applied.remove(&entity);
    }

    /// Drops every recorded value.
    pub fn clear(&mut self) {
        self.applied.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::AppliedState;
    use legion::{Entity, World};

    fn entity(world: &mut World) -> Entity {
        world.push((0u32,))
    }

    #[test]
    fn first_sight_reports_a_change() {
        let mut world = World::default();
        let e = entity(&mut world);
        let mut state = AppliedState::<u32>::default();

        assert!(state.take_if_changed(e, &1));
    }

    #[test]
    fn equal_value_reports_no_change() {
        let mut world = World::default();
        let e = entity(&mut world);
        let mut state = AppliedState::<u32>::default();
        state.take_if_changed(e, &1);

        assert!(!state.take_if_changed(e, &1));
    }

    #[test]
    fn changed_value_reports_a_change() {
        let mut world = World::default();
        let e = entity(&mut world);
        let mut state = AppliedState::<u32>::default();
        state.take_if_changed(e, &1);

        assert!(state.take_if_changed(e, &2));
        assert!(!state.take_if_changed(e, &2));
    }

    #[test]
    fn forget_makes_the_next_check_report_a_change() {
        let mut world = World::default();
        let e = entity(&mut world);
        let mut state = AppliedState::<u32>::default();
        state.take_if_changed(e, &1);

        state.forget(e);

        assert!(state.take_if_changed(e, &1));
    }

    #[test]
    fn clear_makes_every_next_check_report_a_change() {
        let mut world = World::default();
        let a = entity(&mut world);
        let b = entity(&mut world);
        let mut state = AppliedState::<u32>::default();
        state.take_if_changed(a, &1);
        state.take_if_changed(b, &1);

        state.clear();

        assert!(state.take_if_changed(a, &1));
        assert!(state.take_if_changed(b, &1));
    }
}
