use super::super::*;
fn advance(h: &mut ComponentHealth, dt: f32) {
    h.damage_trail
        .advance(dt, h.current_health, h.max_health, h.dead);
}

fn edge(h: &ComponentHealth) -> f32 {
    h.damage_trail.edge(h.current_health, h.max_health, h.dead)
}

#[test]
fn damage_chunk_holds_drains_and_restarts_from_the_current_edge() {
    let mut h = ComponentHealth::default();
    h.apply_damage(40.0);
    assert_eq!(edge(&h), 100.0);
    advance(&mut h, 0.30);
    assert_eq!(edge(&h), 100.0);
    for invalid in [0.0, -2.0, f32::NAN, f32::INFINITY] {
        h.apply_damage(invalid);
    }
    advance(&mut h, 0.35);
    assert!((edge(&h) - 80.0).abs() < 0.001);
    h.apply_damage(20.0);
    assert_eq!(h.current_health, 40.0);
    assert!((edge(&h) - 80.0).abs() < 0.001);
    advance(&mut h, 0.35);
    assert!((edge(&h) - 80.0).abs() < 0.001);
    advance(&mut h, 0.30);
    assert!((edge(&h) - 60.0).abs() < 0.001);
    advance(&mut h, 0.31);
    assert_eq!(edge(&h), 40.0);
}

#[test]
fn healing_death_respawn_and_deserialization_clear_damage_chunks() {
    let mut h = ComponentHealth::default();
    h.apply_damage(40.0);
    h.current_health = 75.0;
    assert_eq!(edge(&h), 75.0);
    advance(&mut h, 0.1);
    assert_eq!(edge(&h), 75.0);
    h.apply_damage(20.0);
    h.restore_full();
    assert_eq!(edge(&h), 100.0);
    h.apply_damage(100.0);
    assert!(h.dead);
    assert_eq!(edge(&h), 0.0);
    h.restore_full();
    assert!(!h.dead);
    assert_eq!(edge(&h), 100.0);
    h.apply_damage(30.0);
    let saved = serde_json::to_value(h).unwrap();
    assert!(saved.get("damage_trail").is_none());
    let loaded: ComponentHealth = serde_json::from_value(saved).unwrap();
    assert_eq!(edge(&loaded), 70.0);
}
