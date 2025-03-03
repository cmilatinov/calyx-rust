use std::collections::HashMap;
use std::ops::DerefMut;
use std::time::Instant;

use crate as engine;
use crate::utils::singleton_with_init;

pub type TimeType = f32;

pub struct Time {
    timers: HashMap<&'static str, Instant>,
    last_time: Instant,
    pub time: TimeType,
    pub static_time: TimeType,
    pub delta_time: TimeType,
    pub static_delta_time: TimeType,
    pub time_scale: TimeType,
}

singleton_with_init!(Time);

macro_rules! time_member_getter {
    ($member:ident) => {
        pub fn $member() -> TimeType {
            let instance = Self::get();
            instance.$member
        }
    };
}

impl Time {
    pub fn update_time() {
        let mut instance = Self::get_mut();
        instance.static_delta_time = instance.last_time.elapsed().as_secs_f32();
        instance.static_time += instance.static_delta_time;
        instance.delta_time = instance.static_delta_time * instance.time_scale;
        instance.time += instance.delta_time;
        instance.last_time = Instant::now();
    }

    pub fn timer(name: &'static str) -> TimeType {
        let mut instance = Self::get_mut();
        let instant = instance.timers.entry(name).or_insert(Instant::now());
        instant.elapsed().as_secs_f32()
    }

    pub fn reset_timer(name: &'static str) {
        let mut instance = Self::get_mut();
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
            time_scale: 1.0,
        }
    }
}
