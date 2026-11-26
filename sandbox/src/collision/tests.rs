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
