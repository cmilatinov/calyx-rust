//! PROTOTYPE (poise_poc, #610): the poise state machine and the grappling
//! hook's swing, kept free of the scene so they can be tested alone.

use nalgebra_glm::{vec3, Vec3};

/// What a tank with poise is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Stance {
    /// Driving under kinematic control; hits drain poise.
    #[default]
    Upright,
    /// Tumbling on a dynamic rigid body, `elapsed` seconds after the break.
    KnockedDown { elapsed: f32 },
    /// Back on kinematic control, easing upright onto its tracks.
    Righting { elapsed: f32 },
    /// Held and swung by a grappling hook.
    Hooked,
}

/// The tuning [`PoiseState`] needs, copied from the tank's poise component.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoiseTuning {
    pub max_poise: f32,
    pub regen_delay: f32,
    pub regen_rate: f32,
    pub recovery_time: f32,
    pub righting_time: f32,
    pub immunity_time: f32,
}

/// The result of a hit on a tank's poise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitOutcome {
    /// Poise dropped but held.
    Drained,
    /// Poise reached zero: the tank goes down.
    Broken,
    /// The tank is already down or held; its poise is not touched.
    AlreadyDown,
}

/// A change of stance [`PoiseState::tick`] asks the scene to carry out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// The recovery time is over: back to kinematic control, and upright.
    StartRighting,
    /// Upright again, at full poise and immune for a while.
    Recovered,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoiseState {
    pub poise: f32,
    pub stance: Stance,
    /// Seconds since the last hit.
    pub since_hit: f32,
    /// Seconds left in which poise cannot break.
    pub immunity: f32,
}

impl PoiseState {
    pub fn new(max_poise: f32) -> Self {
        Self {
            poise: max_poise.max(0.0),
            stance: Stance::Upright,
            since_hit: f32::INFINITY,
            immunity: 0.0,
        }
    }

    pub fn is_upright(&self) -> bool {
        self.stance == Stance::Upright
    }

    pub fn is_immune(&self) -> bool {
        self.is_upright() && self.immunity > 0.0
    }

    /// Drains `amount` of poise. While immune, poise stops at 1 and cannot
    /// break.
    pub fn hit(&mut self, amount: f32) -> HitOutcome {
        if !self.is_upright() {
            return HitOutcome::AlreadyDown;
        }
        let amount = if amount.is_finite() {
            amount.max(0.0)
        } else {
            0.0
        };
        self.since_hit = 0.0;
        let floor = if self.immunity > 0.0 {
            self.poise.min(1.0)
        } else {
            0.0
        };
        self.poise = (self.poise - amount).max(floor);
        if self.poise <= 0.0 {
            self.poise = 0.0;
            HitOutcome::Broken
        } else {
            HitOutcome::Drained
        }
    }

    /// Puts the tank down on physics with no poise, whatever it had.
    pub fn knock_down(&mut self) {
        self.poise = 0.0;
        self.immunity = 0.0;
        self.stance = Stance::KnockedDown { elapsed: 0.0 };
    }

    /// Advances the state by `dt` seconds: regeneration while upright, the
    /// recovery and righting clocks while down.
    pub fn tick(&mut self, dt: f32, tuning: &PoiseTuning) -> Option<Transition> {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        match self.stance {
            Stance::Upright => {
                self.immunity = (self.immunity - dt).max(0.0);
                self.since_hit += dt;
                if self.since_hit >= tuning.regen_delay {
                    self.poise = (self.poise + tuning.regen_rate.max(0.0) * dt)
                        .min(tuning.max_poise.max(0.0));
                }
                None
            }
            Stance::KnockedDown { elapsed } => {
                let elapsed = elapsed + dt;
                if elapsed >= tuning.recovery_time {
                    self.stance = Stance::Righting { elapsed: 0.0 };
                    Some(Transition::StartRighting)
                } else {
                    self.stance = Stance::KnockedDown { elapsed };
                    None
                }
            }
            Stance::Righting { elapsed } => {
                let elapsed = elapsed + dt;
                if elapsed >= tuning.righting_time {
                    self.recover(tuning);
                    Some(Transition::Recovered)
                } else {
                    self.stance = Stance::Righting { elapsed };
                    None
                }
            }
            Stance::Hooked => None,
        }
    }

    /// Upright at full poise, immune for the immunity time.
    pub fn recover(&mut self, tuning: &PoiseTuning) {
        self.stance = Stance::Upright;
        self.poise = tuning.max_poise.max(0.0);
        self.immunity = tuning.immunity_time.max(0.0);
        self.since_hit = f32::INFINITY;
    }

    /// How far through the righting the tank is, 0 to 1.
    pub fn righting_progress(&self, tuning: &PoiseTuning) -> f32 {
        match self.stance {
            Stance::Righting { elapsed } if tuning.righting_time > 0.0 => {
                (elapsed / tuning.righting_time).clamp(0.0, 1.0)
            }
            Stance::Righting { .. } => 1.0,
            _ => 0.0,
        }
    }
}

/// The tuning a hook swing needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingTuning {
    /// The radius the swing reels in or pays out to, in metres.
    pub radius: f32,
    /// How fast the radius changes towards it, in metres per second.
    pub reel_speed: f32,
    /// Angular acceleration, in radians per second squared.
    pub acceleration: f32,
    /// Top angular speed, in radians per second.
    pub max_speed: f32,
    /// +1 to swing anticlockwise seen from above, -1 clockwise.
    pub direction: f32,
}

/// A hooked tank's swing around the hooking tank: where it is on the circle,
/// how fast it goes round and how far out it is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Swing {
    /// Angle from +X towards +Z, in radians.
    pub angle: f32,
    /// Signed angular speed, in radians per second.
    pub angular_speed: f32,
    /// Distance from the centre, in metres.
    pub radius: f32,
}

impl Swing {
    /// A swing that starts at rest where `target` is around `center`.
    pub fn starting_at(center: Vec3, target: Vec3) -> Self {
        let offset = target - center;
        Self {
            angle: offset.z.atan2(offset.x),
            angular_speed: 0.0,
            radius: (offset.x * offset.x + offset.z * offset.z).sqrt(),
        }
    }

    /// Speeds the swing up towards its top speed and moves the radius
    /// towards the tuned one, over `dt` seconds.
    pub fn advance(&mut self, dt: f32, tuning: &SwingTuning) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let direction = if tuning.direction < 0.0 { -1.0 } else { 1.0 };
        let target = direction * tuning.max_speed.max(0.0);
        let step = tuning.acceleration.max(0.0) * dt;
        self.angular_speed += (target - self.angular_speed).clamp(-step, step);
        self.angle += self.angular_speed * dt;
        let reel = tuning.reel_speed.max(0.0) * dt;
        self.radius += (tuning.radius.max(0.0) - self.radius).clamp(-reel, reel);
    }

    /// The hooked tank's horizontal offset from the centre.
    pub fn offset(&self) -> Vec3 {
        vec3(self.angle.cos(), 0.0, self.angle.sin()) * self.radius
    }

    /// The hooked tank's velocity going round: along the tangent, at the
    /// swing's angular speed times its radius.
    pub fn velocity(&self) -> Vec3 {
        vec3(-self.angle.sin(), 0.0, self.angle.cos()) * self.angular_speed * self.radius
    }

    /// The velocity a release throws the hooked tank at: the swing's
    /// velocity scaled by `strength`, plus `lift` upwards. It grows with the
    /// swing's speed, so a faster swing throws harder, and it points along
    /// the tangent where it is let go, so timing aims it.
    pub fn throw_velocity(&self, strength: f32, lift: f32) -> Vec3 {
        self.velocity() * strength.max(0.0) + Vec3::y() * lift
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> PoiseTuning {
        PoiseTuning {
            max_poise: 100.0,
            regen_delay: 1.0,
            regen_rate: 50.0,
            recovery_time: 2.0,
            righting_time: 0.5,
            immunity_time: 1.5,
        }
    }

    #[test]
    fn hits_drain_poise_and_it_regenerates_only_after_the_delay() {
        let tuning = tuning();
        let mut state = PoiseState::new(100.0);
        assert_eq!(state.hit(35.0), HitOutcome::Drained);
        assert_eq!(state.poise, 65.0);
        state.tick(0.9, &tuning);
        assert_eq!(state.poise, 65.0, "no regeneration before the delay");
        state.tick(0.2, &tuning);
        assert!(state.poise > 65.0, "regenerates once the delay is over");
        // A hit restarts the delay.
        let before = state.poise - 10.0;
        state.hit(10.0);
        state.tick(0.5, &tuning);
        assert_eq!(state.poise, before);
        // Regeneration stops at the maximum.
        state.tick(10.0, &tuning);
        state.tick(10.0, &tuning);
        assert_eq!(state.poise, 100.0);
        // Invalid damage changes nothing.
        for invalid in [-5.0, f32::NAN, f32::INFINITY] {
            state.hit(invalid);
        }
        assert_eq!(state.poise, 100.0);
    }

    #[test]
    fn a_break_knocks_down_recovers_and_grants_immunity() {
        let tuning = tuning();
        let mut state = PoiseState::new(100.0);
        state.hit(60.0);
        assert_eq!(state.hit(60.0), HitOutcome::Broken);
        assert_eq!(state.poise, 0.0);
        state.knock_down();
        assert!(!state.is_upright());
        // Hits on a tumbling tank leave its poise alone.
        assert_eq!(state.hit(30.0), HitOutcome::AlreadyDown);

        assert_eq!(state.tick(1.9, &tuning), None);
        assert_eq!(state.tick(0.2, &tuning), Some(Transition::StartRighting));
        assert_eq!(state.stance, Stance::Righting { elapsed: 0.0 });
        assert_eq!(state.tick(0.25, &tuning), None);
        assert!((state.righting_progress(&tuning) - 0.5).abs() < 1e-6);
        assert_eq!(state.tick(0.3, &tuning), Some(Transition::Recovered));
        assert!(state.is_upright());
        assert_eq!(state.poise, 100.0);
        assert!(state.is_immune());

        // Immune: hits drain poise but never break it.
        assert_eq!(state.hit(150.0), HitOutcome::Drained);
        assert_eq!(state.poise, 1.0);
        assert_eq!(state.hit(150.0), HitOutcome::Drained);
        // Once immunity runs out, poise breaks again.
        state.tick(1.6, &tuning);
        assert!(!state.is_immune());
        assert_eq!(state.hit(100.0), HitOutcome::Broken);
    }

    #[test]
    fn a_hooked_tank_waits_for_its_release() {
        let tuning = tuning();
        let mut state = PoiseState::new(100.0);
        state.stance = Stance::Hooked;
        assert_eq!(state.tick(10.0, &tuning), None);
        assert_eq!(state.hit(50.0), HitOutcome::AlreadyDown);
        state.knock_down();
        assert_eq!(state.stance, Stance::KnockedDown { elapsed: 0.0 });
    }

    fn swing_tuning(direction: f32) -> SwingTuning {
        SwingTuning {
            radius: 5.0,
            reel_speed: 10.0,
            acceleration: 8.0,
            max_speed: 4.0,
            direction,
        }
    }

    #[test]
    fn a_swing_speeds_up_to_its_top_speed_and_reels_to_its_radius() {
        let mut swing = Swing::starting_at(Vec3::zeros(), vec3(8.0, 0.0, 0.0));
        assert_eq!(swing.radius, 8.0);
        assert_eq!(swing.angle, 0.0);
        swing.advance(0.25, &swing_tuning(1.0));
        assert!((swing.angular_speed - 2.0).abs() < 1e-6);
        assert!((swing.radius - 5.5).abs() < 1e-6);
        for _ in 0..20 {
            swing.advance(0.1, &swing_tuning(1.0));
        }
        assert!((swing.angular_speed - 4.0).abs() < 1e-6);
        assert!((swing.radius - 5.0).abs() < 1e-6);
        let mut clockwise = Swing::starting_at(Vec3::zeros(), vec3(5.0, 0.0, 0.0));
        clockwise.advance(10.0, &swing_tuning(-1.0));
        assert!((clockwise.angular_speed + 4.0).abs() < 1e-6);
    }

    #[test]
    fn a_release_throws_along_the_tangent_harder_the_faster_the_swing() {
        for angle in [0.0f32, 0.7, 2.0, -2.5] {
            let swing = Swing {
                angle,
                angular_speed: 3.0,
                radius: 5.0,
            };
            let throw = swing.throw_velocity(1.5, 0.0);
            // Along the tangent: square to the line from the centre.
            assert!(throw.dot(&swing.offset()).abs() < 1e-4);
            assert!((throw.magnitude() - 3.0 * 5.0 * 1.5).abs() < 1e-4);
            // Anticlockwise from above goes the way the angle grows.
            let ahead = Swing {
                angle: angle + 0.01,
                ..swing
            };
            assert!(throw.dot(&(ahead.offset() - swing.offset())) > 0.0);
        }
        let slow = Swing {
            angle: 0.0,
            angular_speed: 1.0,
            radius: 5.0,
        };
        let fast = Swing {
            angular_speed: 4.0,
            ..slow
        };
        assert!(
            fast.throw_velocity(1.0, 2.0).magnitude() > slow.throw_velocity(1.0, 2.0).magnitude()
        );
        assert_eq!(fast.throw_velocity(1.0, 2.0).y, 2.0);
        // Swinging the other way throws the other way.
        let back = Swing {
            angular_speed: -4.0,
            ..slow
        };
        assert!(
            back.throw_velocity(1.0, 0.0)
                .dot(&fast.throw_velocity(1.0, 0.0))
                < 0.0
        );
    }
}
