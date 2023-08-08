use std::collections::HashMap;
use std::time::{Instant, SystemTime};
use crate::utils::{Init, singleton};

pub type TimeType = f64;

pub struct Time {
    timers: HashMap<&'static str, Instant>,
    last_time: Instant,
    pub time: TimeType,
    pub static_time: TimeType,
    pub delta_time: TimeType,
    pub static_delta_time: TimeType,
    pub time_scale: TimeType,
}

singleton!(Time);

macro_rules! time_member_getter {
    ($member:ident) => {
        pub fn $member() -> TimeType {
            let instance = Self::get();
            instance.$member
        }
    }
}

impl Time {
    pub fn update_time() {
        let mut instance = Self::get();
        instance.static_delta_time = instance.last_time.elapsed().as_secs_f64();
        instance.static_time += instance.static_delta_time;
        instance.delta_time = instance.static_delta_time * instance.time_scale;
        instance.time += instance.delta_time;
        instance.last_time = Instant::now();
    }

    pub fn timer(name: &'static str) -> TimeType {
        let mut instance = Self::get();
        let instant = instance.timers.entry(name).or_insert(Instant::now());
        instant.elapsed().as_secs_f64()
    }

    pub fn reset_timer(name: &'static str) {
        let mut instance = Self::get();
        instance.timers.insert(name, Instant::now());
    }

    time_member_getter!(time);
    time_member_getter!(delta_time);
    time_member_getter!(static_time);
    time_member_getter!(static_delta_time);
    time_member_getter!(time_scale);
}

impl Default for Time {
    fn default() -> Self {
        Self {
            timers: HashMap::new(),
            last_time: Instant::now(),
            time: 0.0,
            static_time: 0.0,
            delta_time: 0.0,
            static_delta_time: 0.0,
            time_scale: 1.0
        }
    }
}

impl Init<Time> for Time {}