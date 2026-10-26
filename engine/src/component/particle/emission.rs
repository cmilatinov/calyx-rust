//! When and how many particles a system spawns.

use crate as engine;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::TypeUuid;
use serde::{Deserialize, Serialize};

/// Emission module: a one-shot burst at the start of each loop plus a
/// continuous rate, capped by `max_particles` live particles.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "001d044d-f5cb-49f1-b89b-b2d9265e2567"]
#[reflect(Default)]
#[serde(default)]
pub struct ParticleEmission {
    /// Whether timed emission loops after `duration`.
    pub looping: bool,
    /// Continuous emission rate in particles per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub rate: f32,
    /// One-shot burst count emitted when the system starts, and again at the
    /// start of each loop.
    pub burst_count: u32,
    /// Maximum live particle count.
    pub max_particles: u32,
    /// Loop duration for timed emission. `0.0` means unbounded.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub duration: f32,
}

impl Default for ParticleEmission {
    fn default() -> Self {
        Self {
            looping: true,
            rate: 256.0,
            burst_count: 0,
            max_particles: 256,
            duration: 0.0,
        }
    }
}

/// Emission progress of one system. Reset with the system.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EmissionState {
    accumulator: f32,
    elapsed_time: f32,
    burst_emitted: bool,
}

impl ParticleEmission {
    /// Advances `state` by `delta_time` and returns how many particles to
    /// spawn now, before the `max_particles` cap. A zero `delta_time` only
    /// emits a pending burst.
    pub(crate) fn emit(&self, state: &mut EmissionState, delta_time: f32) -> u32 {
        if delta_time <= 0.0 {
            return self.take_burst(state);
        }

        let emission_delta_time = self.advance_time(state, delta_time);
        let mut count = self.take_burst(state);
        if emission_delta_time > 0.0 && self.rate > 0.0 {
            state.accumulator += emission_delta_time * self.rate.max(0.0);
            let from_rate = state.accumulator.floor() as u32;
            if from_rate > 0 {
                state.accumulator -= from_rate as f32;
                count = count.saturating_add(from_rate);
            }
        }
        count
    }

    fn take_burst(&self, state: &mut EmissionState) -> u32 {
        if state.burst_emitted {
            return 0;
        }
        state.burst_emitted = true;
        self.burst_count
    }

    /// Returns the part of `delta_time` that falls inside the emission window.
    fn advance_time(&self, state: &mut EmissionState, delta_time: f32) -> f32 {
        if self.duration <= 0.0 {
            state.elapsed_time += delta_time;
            return delta_time;
        }

        let previous_elapsed = state.elapsed_time;
        state.elapsed_time += delta_time;

        if self.looping {
            while state.elapsed_time >= self.duration {
                state.elapsed_time -= self.duration;
                state.burst_emitted = false;
            }
            delta_time
        } else {
            (self.duration - previous_elapsed).clamp(0.0, delta_time)
        }
    }
}

#[cfg(test)]
mod tests {
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
}
