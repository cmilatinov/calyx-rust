use super::*;

#[test]
fn a_zero_step_emits_only_the_burst() {
    let emission = ParticleEmission {
        burst_count: 3,
        rate: 100.0,
        ..Default::default()
    };
    let mut state = EmissionState::default();
    assert_eq!(emission.emit(&mut state, 0.0), 3);
    assert_eq!(emission.emit(&mut state, 0.0), 0);
}

#[test]
fn rate_carries_fractions_between_steps() {
    let emission = ParticleEmission {
        rate: 6.0,
        ..Default::default()
    };
    let mut state = EmissionState::default();
    let counts: Vec<u32> = (0..4).map(|_| emission.emit(&mut state, 0.25)).collect();
    assert_eq!(counts, vec![1, 2, 1, 2]);
}

#[test]
fn a_looping_emission_repeats_its_burst_each_loop() {
    let emission = ParticleEmission {
        rate: 0.0,
        burst_count: 2,
        duration: 1.0,
        looping: true,
        ..Default::default()
    };
    let mut state = EmissionState::default();
    assert_eq!(emission.emit(&mut state, 0.5), 2);
    assert_eq!(emission.emit(&mut state, 0.4), 0);
    assert_eq!(emission.emit(&mut state, 0.2), 2);
}

#[test]
fn a_one_shot_emission_stops_after_its_duration() {
    let emission = ParticleEmission {
        rate: 10.0,
        duration: 0.25,
        looping: false,
        ..Default::default()
    };
    let mut state = EmissionState::default();
    assert_eq!(emission.emit(&mut state, 0.2), 2);
    assert_eq!(emission.emit(&mut state, 0.2), 0);
    assert_eq!(emission.emit(&mut state, 1.0), 0);
}
