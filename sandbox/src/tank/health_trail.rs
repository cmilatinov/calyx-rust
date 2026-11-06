//! Target-owned transient presentation of recently removed health.
/// The white lost-health segment holds, then drains to the live health edge.
#[derive(Clone, Copy, Debug, Default)]
pub struct HealthDamageTrail {
    observed: Option<f32>,
    maximum: f32,
    dead: bool,
    edge: f32,
    start: f32,
    age: f32,
}

impl HealthDamageTrail {
    pub const HOLD: f32 = 0.35;
    pub const SHRINK: f32 = 0.60;
    pub(crate) fn reset(&mut self, health: f32, maximum: f32, dead: bool) {
        *self = Self {
            observed: Some(health),
            maximum,
            dead,
            edge: health,
            start: health,
            age: Self::HOLD + Self::SHRINK,
        };
    }

    pub(crate) fn observe(&mut self, health: f32, maximum: f32, dead: bool) -> bool {
        let Some(previous) = self.observed else {
            self.reset(health, maximum, dead);
            return true;
        };
        if !health.is_finite()
            || !maximum.is_finite()
            || health > previous
            || maximum != self.maximum
            || dead != self.dead
        {
            self.reset(health, maximum, dead);
            return true;
        }
        if health < previous {
            self.edge = self.edge.max(previous);
            self.start = self.edge;
            self.age = 0.0;
            self.observed = Some(health);
            return true;
        }
        false
    }

    pub(crate) fn advance(&mut self, dt: f32, health: f32, maximum: f32, dead: bool) {
        if self.observe(health, maximum, dead) {
            return;
        }
        if dt.is_finite() {
            self.age += dt.max(0.0);
        }
        let t = ((self.age - Self::HOLD) / Self::SHRINK).clamp(0.0, 1.0);
        self.edge = self.start + (health - self.start) * t;
    }

    pub(crate) fn edge(&self, health: f32, maximum: f32, dead: bool) -> f32 {
        if self.observed.is_none_or(|old| health > old)
            || maximum != self.maximum
            || dead != self.dead
        {
            return health;
        }
        self.edge.max(health)
    }
}
