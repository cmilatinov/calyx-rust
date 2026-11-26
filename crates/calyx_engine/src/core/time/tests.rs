use super::Time;

#[test]
fn new_starts_at_zero() {
    let t = Time::new(60.0);
    assert_eq!(t.time(), 0.0);
    assert_eq!(t.delta_time(), 0.0);
    assert_eq!(t.time_scale(), 1.0);
}

#[test]
fn current_tick_increases_with_time() {
    let t = Time::new(100.0);
    assert_eq!(t.tick_rate(), 100.0);
    assert_eq!(t.current_tick(), 0);
}

#[test]
fn time_to_tick_formula() {
    let t = Time::new(10.0);
    assert_eq!(t.time_to_tick(0.0), 0);
    assert_eq!(t.time_to_tick(0.25), 2);
    assert_eq!(t.time_to_tick(1.0), 10);
}

#[test]
fn tenth_second_steps_and_exact_seconds_reach_their_tick() {
    // Ten 0.1 s steps sum to just under 1.0 in f64; counted in ticks
    // they reach tick 10 exactly, as does an exact second.
    let mut t = Time::new(10.0);
    t.advance_by(1.0);
    assert_eq!(t.current_tick(), 10);

    let mut t = Time::new(10.0);
    for _ in 0..10 {
        t.advance_by(0.1);
    }
    assert_eq!(t.current_tick(), 10);
}

#[test]
fn exact_times_do_not_drift_ahead_over_long_sessions() {
    // Converting seconds to ticks has no period rounding to run ahead of.
    let t = Time::new(100.0);
    assert_eq!(t.time_to_tick(864_000.0), 86_400_000);
    assert_eq!(t.time_to_tick(864_000.0 - 0.005), 86_399_999);
}

#[test]
fn full_fixed_steps_reach_their_tick_when_the_period_is_inexact() {
    // 0.01 is inexact in f32 and f64, but each step adds one whole tick.
    let mut t = Time::new(100.0);
    t.advance_by(0.01);
    assert_eq!(t.current_tick(), 1);
    for _ in 1..1_000 {
        t.advance_by(0.01);
    }
    assert_eq!(t.current_tick(), 1_000);
}

#[test]
fn hours_of_fixed_steps_stay_on_their_tick() {
    // Summing 0.01 in f64 drifts past a fixed epsilon by this point.
    let mut t = Time::new(100.0);
    let steps = 1_839_063;
    for _ in 0..steps {
        t.advance_by(0.01);
    }
    assert_eq!(t.current_tick(), steps);
    assert_eq!(t.time_to_tick(t.time()), steps);
}

#[test]
fn periods_that_are_not_whole_nanoseconds_stay_on_their_tick() {
    // One 120 Hz period is 8_333_333.33 ns, which whole nanoseconds or
    // binary fractions cannot hold exactly.
    let mut t = Time::new(120.0);
    t.advance_by(1.0 / 120.0);
    assert_eq!(t.current_tick(), 1);
    for _ in 1..1_000_000 {
        t.advance_by(1.0 / 120.0);
    }
    assert_eq!(t.current_tick(), 1_000_000);
    assert_eq!(t.time_to_tick(t.time()), 1_000_000);
}

#[test]
fn tick_indices_saturate_instead_of_wrapping() {
    let mut t = Time::new(120.0);
    let beyond_u32 = (f64::from(u32::MAX) + 10.0) / 120.0;
    t.set_time(beyond_u32);
    assert_eq!(t.current_tick(), u32::MAX);
    assert_eq!(t.time_to_tick(beyond_u32), u32::MAX);
}

#[test]
fn tick_indices_stay_exact_after_more_than_a_day() {
    // 128 Hz and 0.25 s steps are exactly representable, so any drift
    // comes from the accumulator precision alone.
    let mut t = Time::new(128.0);
    let steps = 400_000u32; // 100_000 s, longer than a day
    for _ in 0..steps {
        t.advance_by(0.25);
    }
    assert!(t.time() > 86_400.0);
    assert_eq!(t.time(), 100_000.0);
    assert_eq!(t.static_time(), 100_000.0);
    assert_eq!(t.current_tick(), 100_000 * 128);
    assert_eq!(t.time_to_tick(t.time() + 1.0 / 128.0), 100_000 * 128 + 1);
    assert_eq!(t.time_to_tick(t.time() - 1.0 / 128.0), 100_000 * 128 - 1);
}

#[test]
fn time_scale_affects_delta() {
    let mut t = Time::new(60.0);
    t.time_scale = 2.0;
    t.update_time();
    assert!((t.delta_time() - t.static_delta_time() * 2.0).abs() < 1e-4);
}
