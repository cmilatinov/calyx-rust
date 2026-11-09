//! The sandbox's collision groups: what each static collider stops.
//!
//! A static collider stops the tank's hull, rounds and sight by default,
//! as it always has: [`CollisionGroups::ALL`] makes it a member of every
//! group. A collider that is a member of only some of the groups below
//! stops only those: [`TANK_BLOCKER`], for water, stops the hull and lets
//! rounds, sight and the aim through. Each check asks with its own groups,
//! [`HULL`], [`ROUNDS`] or [`SIGHT`], so it sees only the colliders that are
//! members of its group.

use calyx_engine::component::CollisionGroups;

/// Group 1: colliders the tank's hull cannot drive into.
pub const HULL_GROUP: u32 = 1 << 0;
/// Group 2: colliders that absorb rounds.
pub const ROUNDS_GROUP: u32 = 1 << 1;
/// Group 3: colliders that block sight, for fog of war and the minimap.
pub const SIGHT_GROUP: u32 = 1 << 2;
/// Group 4: the volumes of brush patches, which stop nothing.
pub const BRUSH_GROUP: u32 = 1 << 3;

/// What the hull's sweep and turn ask with: only colliders in
/// [`HULL_GROUP`] stop it.
pub const HULL: CollisionGroups = CollisionGroups::new(HULL_GROUP, HULL_GROUP);
/// What a round's flight asks with: only colliders in [`ROUNDS_GROUP`]
/// absorb it.
pub const ROUNDS: CollisionGroups = CollisionGroups::new(ROUNDS_GROUP, ROUNDS_GROUP);
/// What sight asks with: only colliders in [`SIGHT_GROUP`] block it.
pub const SIGHT: CollisionGroups = CollisionGroups::new(SIGHT_GROUP, SIGHT_GROUP);

/// The groups of a collider that stops tanks but not rounds or sight, such
/// as water.
pub const TANK_BLOCKER: CollisionGroups = CollisionGroups::new(HULL_GROUP, u32::MAX);

/// The groups of a brush patch's volumes ([`crate::brush`]): a member of
/// [`BRUSH_GROUP`] only, interacting with nothing, so they stop no tank,
/// round or sight and take part in no contact.
pub const BRUSH: CollisionGroups = CollisionGroups::new(BRUSH_GROUP, 0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_colliders_stop_everything() {
        for check in [HULL, ROUNDS, SIGHT] {
            assert!(CollisionGroups::ALL.interacts_with(&check));
        }
    }

    #[test]
    fn a_tank_blocker_stops_only_the_hull() {
        assert!(TANK_BLOCKER.interacts_with(&HULL));
        assert!(!TANK_BLOCKER.interacts_with(&ROUNDS));
        assert!(!TANK_BLOCKER.interacts_with(&SIGHT));
    }

    #[test]
    fn brush_stops_nothing() {
        for check in [HULL, ROUNDS, SIGHT, CollisionGroups::ALL] {
            assert!(!BRUSH.interacts_with(&check));
        }
    }
}
