use crate as engine;
use crate::resource::Resource;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub type TimeType = f32;

#[derive(Resource)]
pub struct Time {
    timers: RefCell<HashMap<&'static str, Instant>>,
    last_time: Instant,
    tick_rate: TimeType,
    tick_period: TimeType,
    pub time: TimeType,
    pub static_time: TimeType,
    pub delta_time: TimeType,
    pub static_delta_time: TimeType,
    pub time_scale: TimeType,
}

macro_rules! time_member_getter {
    ($member:ident) => {
        pub fn $member(&self) -> TimeType {
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

    pub fn new(tick_rate: TimeType) -> Self {
        Self {
            timers: RefCell::new(HashMap::new()),
            last_time: Instant::now(),
            tick_rate,
            tick_period: 1.0 / tick_rate,
            time: 0.0,
            static_time: 0.0,
            delta_time: 0.0,
            static_delta_time: 0.0,
            time_scale: 1.0,
        }
    }

    pub fn update_time(&mut self) {
        self.static_delta_time = self.last_time.elapsed().as_secs_f32();
        self.static_time += self.static_delta_time;
        self.delta_time = self.static_delta_time * self.time_scale;
        self.time += self.delta_time;
        self.last_time = Instant::now();
    }

    pub fn timer(&self, name: &'static str) -> TimeType {
        let mut timers = self.timers.borrow_mut();
        let instant = timers.entry(name).or_insert(Instant::now());
        instant.elapsed().as_secs_f32()
    }

    pub fn reset_timer(&self, name: &'static str) {
        self.timers.borrow_mut().insert(name, Instant::now());
    }

    pub fn static_duration(&self) -> Duration {
        Duration::from_secs_f32(self.static_delta_time)
    }

    pub fn tick_rate(&self) -> TimeType {
        self.tick_rate
    }

    pub fn current_tick(&self) -> u32 {
        (self.time / self.tick_period) as u32
    }

    pub fn time_to_tick(&self, time: TimeType) -> u32 {
        (time / self.tick_period) as u32
    }

    time_member_getter!(time);
    time_member_getter!(delta_time);
    time_member_getter!(static_time);
    time_member_getter!(static_delta_time);
    time_member_getter!(time_scale);
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
    fn time_scale_affects_delta() {
        let mut t = Time::new(60.0);
        t.time_scale = 2.0;
        t.update_time();
        assert!((t.delta_time() - t.static_delta_time() * 2.0).abs() < 1e-4);
    }
}
