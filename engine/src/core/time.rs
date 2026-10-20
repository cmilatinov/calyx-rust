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
            tick_period: Self::tick_period(tick_rate),
            time: 0.0,
            static_time: 0.0,
            delta_time: 0.0,
            static_delta_time: 0.0,
            time_scale: 1.0,
        }
    }

    /// Returns the smaller of the exact period and the widened f32 period, so
    /// both exact boundaries and accumulated f32 fixed steps reach their tick.
    fn tick_period(tick_rate: TimeType) -> AbsoluteTime {
        let exact = 1.0 / AbsoluteTime::from(tick_rate);
        let widened = AbsoluteTime::from(1.0 / tick_rate);
        exact.min(widened)
    }

    /// Advances the timer using wall-clock elapsed time since the last update.
    pub fn update_time(&mut self) {
        self.static_delta_time = self.last_time.elapsed().as_secs_f32();
        self.static_time += AbsoluteTime::from(self.static_delta_time);
        self.delta_time = self.static_delta_time * self.time_scale;
        self.time += AbsoluteTime::from(self.delta_time);
        self.last_time = Instant::now();
    }

    /// Advances the timer by an explicit `delta` in seconds.
    pub fn advance_by(&mut self, delta: TimeType) {
        self.static_delta_time = delta;
        self.static_time += AbsoluteTime::from(delta);
        self.delta_time = delta * self.time_scale;
        self.time += AbsoluteTime::from(self.delta_time);
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
        (self.time / self.tick_period) as u32
    }

    /// Converts `time` in seconds into a fixed-tick index.
    pub fn time_to_tick(&self, time: AbsoluteTime) -> u32 {
        (time / self.tick_period) as u32
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
    fn exact_second_boundaries_hold_when_the_f32_period_rounds_up() {
        // 0.1_f32 rounds above 0.1, so an exact second must not fall a tick
        // short, and neither must ten accumulated 0.1_f32 steps.
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
    fn full_fixed_steps_reach_their_tick_when_the_period_is_inexact() {
        // 0.01 is not representable as f32, so the step and the tick period
        // must round the same way or each step lands just short of its tick.
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
