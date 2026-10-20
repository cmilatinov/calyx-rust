use crate as engine;
use crate::resource::Resource;
use crate::utils::TypeUuid;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Floating-point time representation used across the engine.
pub type TimeType = f32;

/// Double-precision representation for absolute time and tick indices, which
/// must stay accurate over long sessions.
pub type AbsoluteTime = f64;

/// Frame timer and fixed-tick conversion utility.
#[derive(Resource, TypeUuid)]
#[uuid = "b0a01abf-afee-4107-94e8-6e2fbab21f72"]
#[repr(C)]
pub struct Time {
    timers: RefCell<HashMap<&'static str, Instant>>,
    last_time: Instant,
    tick_rate: TimeType,
    tick_period: AbsoluteTime,
    /// Scaled elapsed runtime in seconds.
    pub time: AbsoluteTime,
    /// Unscaled elapsed runtime in seconds.
    pub static_time: AbsoluteTime,
    /// Scaled frame delta in seconds.
    pub delta_time: TimeType,
    /// Unscaled frame delta in seconds.
    pub static_delta_time: TimeType,
    /// Multiplier applied to scaled time accumulation.
    pub time_scale: TimeType,
}

macro_rules! time_member_getter {
    ($member:ident: $ty:ty) => {
        #[doc = concat!("Returns `self.", stringify!($member), "`.")]
        pub fn $member(&self) -> $ty {
            self.$member
        }
    };
}

impl Default for Time {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TICK_RATE_HZ)
    }
}

impl Time {
    const DEFAULT_TICK_RATE_HZ: TimeType = 120.0;

    /// Creates a timer with the given fixed tick rate.
    pub fn new(tick_rate: TimeType) -> Self {
        Self {
            timers: RefCell::new(HashMap::new()),
            last_time: Instant::now(),
            tick_rate,
            tick_period: 1.0 / AbsoluteTime::from(tick_rate),
            time: 0.0,
            static_time: 0.0,
            delta_time: 0.0,
            static_delta_time: 0.0,
            time_scale: 1.0,
        }
    }

    /// Fraction of a tick within which a time counts as on the boundary, so
    /// rounding in accumulated f64 steps does not leave a tick behind.
    const TICK_BOUNDARY_EPSILON: AbsoluteTime = 1e-6;

    /// Advances the timer using wall-clock elapsed time since the last update.
    pub fn update_time(&mut self) {
        let elapsed = self.last_time.elapsed().as_secs_f64();
        self.last_time = Instant::now();
        self.advance_by(elapsed);
    }

    /// Advances the timer by an explicit `delta` in seconds. Absolute time
    /// accumulates the f64 delta; the frame deltas are its f32 rounding.
    pub fn advance_by(&mut self, delta: AbsoluteTime) {
        let scaled = delta * AbsoluteTime::from(self.time_scale);
        self.static_delta_time = delta as TimeType;
        self.static_time += delta;
        self.delta_time = scaled as TimeType;
        self.time += scaled;
    }

    /// Returns the elapsed time since the named timer was first observed or
    /// last reset.
    pub fn timer(&self, name: &'static str) -> TimeType {
        let mut timers = self.timers.borrow_mut();
        let instant = timers.entry(name).or_insert(Instant::now());
        instant.elapsed().as_secs_f32()
    }

    /// Resets the named timer to the current instant.
    pub fn reset_timer(&self, name: &'static str) {
        self.timers.borrow_mut().insert(name, Instant::now());
    }

    /// Returns the unscaled frame delta as a [`Duration`].
    pub fn static_duration(&self) -> Duration {
        Duration::from_secs_f32(self.static_delta_time)
    }

    /// Returns the configured fixed tick rate in hertz.
    pub fn tick_rate(&self) -> TimeType {
        self.tick_rate
    }

    /// Returns the current fixed-tick index for scaled time.
    pub fn current_tick(&self) -> u32 {
        self.time_to_tick(self.time)
    }

    /// Converts `time` in seconds into a fixed-tick index.
    pub fn time_to_tick(&self, time: AbsoluteTime) -> u32 {
        let ticks = time / self.tick_period;
        let nearest = ticks.round();
        if (ticks - nearest).abs() <= Self::TICK_BOUNDARY_EPSILON {
            nearest as u32
        } else {
            ticks as u32
        }
    }

    time_member_getter!(time: AbsoluteTime);
    time_member_getter!(delta_time: TimeType);
    time_member_getter!(static_time: AbsoluteTime);
    time_member_getter!(static_delta_time: TimeType);
    time_member_getter!(time_scale: TimeType);
}

#[cfg(test)]
mod tests {
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
        // Ten 0.1 s steps sum to just under 1.0 in f64; they still reach
        // tick 10, as does an exact second.
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
        // The tick period is exact, so exact times never run ahead of it.
        let t = Time::new(100.0);
        assert_eq!(t.time_to_tick(864_000.0), 86_400_000);
        assert_eq!(t.time_to_tick(864_000.0 - 0.005), 86_399_999);
    }

    #[test]
    fn full_fixed_steps_reach_their_tick_when_the_period_is_inexact() {
        // 0.01 is inexact even in f64, so accumulated steps land a rounding
        // error short of each boundary and must still reach their tick.
        let mut t = Time::new(100.0);
        t.advance_by(0.01);
        assert_eq!(t.current_tick(), 1);
        for _ in 1..1_000 {
            t.advance_by(0.01);
        }
        assert_eq!(t.current_tick(), 1_000);
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
}
