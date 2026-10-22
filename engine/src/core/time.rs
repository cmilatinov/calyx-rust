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

/// Distance from a whole tick within which a tick count counts as reaching
/// it, so the last-bit rounding of a step or conversion never leaves a tick
/// behind. Tick counts this close to a boundary are indistinguishable in f64.
const TICK_SNAP: AbsoluteTime = 1e-9;

/// Rounds `ticks` down to a whole tick, snapping to the nearest tick when it
/// is within rounding error of it.
fn whole_ticks(ticks: AbsoluteTime) -> AbsoluteTime {
    let nearest = ticks.round();
    let tolerance = TICK_SNAP.max(ticks.abs() * 8.0 * AbsoluteTime::EPSILON);
    if (ticks - nearest).abs() <= tolerance {
        nearest
    } else {
        ticks.floor()
    }
}

/// Elapsed time counted in whole ticks plus a fraction of a tick, so a step
/// of one tick period adds exactly one tick however its seconds round.
#[derive(Clone, Copy, Default)]
struct TickClock {
    ticks: u64,
    fraction: AbsoluteTime,
}

impl TickClock {
    fn from_ticks(ticks: AbsoluteTime) -> Self {
        let ticks = ticks.max(0.0);
        let whole = whole_ticks(ticks);
        Self {
            ticks: whole as u64,
            fraction: (ticks - whole).max(0.0),
        }
    }

    fn advance(&mut self, ticks: AbsoluteTime) {
        let total = self.fraction + ticks.max(0.0);
        let whole = whole_ticks(total);
        self.ticks = self.ticks.saturating_add(whole as u64);
        self.fraction = (total - whole).max(0.0);
    }

    fn total(&self) -> AbsoluteTime {
        self.ticks as AbsoluteTime + self.fraction
    }

    /// Saturates at `u32::MAX`, like the float cast it replaces, so tick
    /// indices never wrap back to zero.
    fn index(&self) -> u32 {
        u32::try_from(self.ticks).unwrap_or(u32::MAX)
    }
}

/// Frame timer and fixed-tick conversion utility.
#[derive(Resource, TypeUuid)]
#[uuid = "b0a01abf-afee-4107-94e8-6e2fbab21f72"]
#[repr(C)]
pub struct Time {
    timers: RefCell<HashMap<&'static str, Instant>>,
    last_time: Instant,
    tick_rate: TimeType,
    /// Scaled elapsed runtime in ticks.
    time: TickClock,
    /// Unscaled elapsed runtime in ticks.
    static_time: TickClock,
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
            time: TickClock::default(),
            static_time: TickClock::default(),
            delta_time: 0.0,
            static_delta_time: 0.0,
            time_scale: 1.0,
        }
    }

    fn rate(&self) -> AbsoluteTime {
        AbsoluteTime::from(self.tick_rate)
    }

    /// Advances the timer using wall-clock elapsed time since the last update.
    pub fn update_time(&mut self) {
        let elapsed = self.last_time.elapsed().as_secs_f64();
        self.last_time = Instant::now();
        self.advance_by(elapsed);
    }

    /// Advances the timer by an explicit `delta` in seconds.
    pub fn advance_by(&mut self, delta: AbsoluteTime) {
        let scaled = delta * AbsoluteTime::from(self.time_scale);
        self.static_delta_time = delta as TimeType;
        self.delta_time = scaled as TimeType;
        self.static_time.advance(delta * self.rate());
        self.time.advance(scaled * self.rate());
    }

    /// Sets the scaled elapsed runtime, for example to follow a server clock.
    pub fn set_time(&mut self, time: AbsoluteTime) {
        self.time = TickClock::from_ticks(time * self.rate());
    }

    /// Returns the scaled elapsed runtime in seconds.
    pub fn time(&self) -> AbsoluteTime {
        self.time.total() / self.rate()
    }

    /// Returns the unscaled elapsed runtime in seconds.
    pub fn static_time(&self) -> AbsoluteTime {
        self.static_time.total() / self.rate()
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
        self.time.index()
    }

    /// Converts `time` in seconds into a fixed-tick index.
    pub fn time_to_tick(&self, time: AbsoluteTime) -> u32 {
        TickClock::from_ticks(time * self.rate()).index()
    }

    time_member_getter!(delta_time: TimeType);
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
}
