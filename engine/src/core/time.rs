use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

pub type TimeType = f32;

pub struct Time {
    timers: RefCell<HashMap<&'static str, Instant>>,
    last_time: Instant,
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

impl Time {
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

    pub fn reset_timer(&mut self, name: &'static str) {
        self.timers.borrow_mut().insert(name, Instant::now());
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
            timers: RefCell::new(HashMap::new()),
            last_time: Instant::now(),
            time: 0.0,
            static_time: 0.0,
            delta_time: 0.0,
            static_delta_time: 0.0,
            time_scale: 1.0,
        }
    }
}
