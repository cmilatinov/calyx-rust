//! PROTOTYPE (poise_poc, #610): the knockout meter's state machine, how
//! hard a hit knocks a tank back, the grappling hook's swing and the spring
//! boot's reach, kept free of the scene so they can be tested alone.

use nalgebra_glm::{vec3, Vec3};

/// What a tank with a knockout meter is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Stance {
    /// Driving under kinematic control.
    #[default]
    Upright,
    /// Tumbling on a dynamic rigid body, `elapsed` seconds after the launch.
    KnockedDown { elapsed: f32 },
    /// Back on kinematic control, easing upright onto its tracks.
    Righting { elapsed: f32 },
    /// Held and swung by a grappling hook.
    Hooked,
    /// Out of the arena, `elapsed` seconds ago, waiting to respawn.
    KnockedOut { elapsed: f32 },
}

/// The timings [`MeterState`] needs, copied from the tank's component.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterTuning {
    pub recovery_time: f32,
    pub righting_time: f32,
    pub immunity_time: f32,
    pub respawn_delay: f32,
    pub respawn_invulnerability: f32,
}

/// A change of stance [`MeterState::tick`] asks the scene to carry out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// The recovery time is over: back to kinematic control, and upright.
    StartRighting,
    /// Upright again, immune to launches for a while.
    Recovered,
    /// Back at the spawn, at 0% and invulnerable for a while.
    Respawn,
}

/// A tank's knockout meter: a percentage that only goes up until it is
/// knocked out, and the clocks of its knockdowns, shields and respawns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterState {
    /// Damage taken, in percent; it never caps.
    pub percent: f32,
    pub stance: Stance,
    /// Seconds left, after righting, in which a hit cannot launch the tank.
    pub immunity: f32,
    /// Seconds left, after a respawn, in which hits do nothing at all.
    pub invulnerable: f32,
    /// Seconds left of an anchor: hits add to the meter but move nothing.
    pub anchored: f32,
    /// How many times the tank has been knocked out.
    pub knockouts: u32,
}

impl Default for MeterState {
    fn default() -> Self {
        Self {
            percent: 0.0,
            stance: Stance::Upright,
            immunity: 0.0,
            invulnerable: 0.0,
            anchored: 0.0,
            knockouts: 0,
        }
    }
}

/// What a hit does to a tank, besides its meter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reaction {
    /// Nothing: the tank is invulnerable, anchored, righting, hooked or out.
    Ignored,
    /// A shove at this speed, in metres per second; the tank keeps driving.
    Shove(f32),
    /// A launch at this speed into a tumble.
    Launch(f32),
    /// A push at this speed to a tank already tumbling.
    Juggle(f32),
}

/// How hard a hit knocks a tank back, Smash-style: `base` metres per second
/// at 0%, growing by `growth` of that for every percent on the meter.
pub fn knockback_speed(base: f32, growth: f32, percent: f32) -> f32 {
    base.max(0.0) * (1.0 + growth.max(0.0) * percent.max(0.0))
}

impl MeterState {
    pub fn is_upright(&self) -> bool {
        self.stance == Stance::Upright
    }

    pub fn is_knocked_out(&self) -> bool {
        matches!(self.stance, Stance::KnockedOut { .. })
    }

    pub fn is_immune(&self) -> bool {
        self.is_upright() && self.immunity > 0.0
    }

    pub fn is_invulnerable(&self) -> bool {
        self.invulnerable > 0.0
    }

    pub fn is_anchored(&self) -> bool {
        self.anchored > 0.0
    }

    /// Takes a hit of `damage` percent pushing at `speed` (already scaled by
    /// the meter, see [`knockback_speed`]) and says what it does. A speed at
    /// or above `launch_threshold` launches an upright tank, unless it is
    /// immune; below it only shoves.
    pub fn hit(&mut self, damage: f32, speed: f32, launch_threshold: f32) -> Reaction {
        if self.is_invulnerable() || self.is_knocked_out() {
            return Reaction::Ignored;
        }
        if damage.is_finite() {
            self.percent += damage.max(0.0);
        }
        let speed = if speed.is_finite() {
            speed.max(0.0)
        } else {
            0.0
        };
        if self.is_anchored() {
            return Reaction::Ignored;
        }
        match self.stance {
            Stance::Upright if speed >= launch_threshold && !self.is_immune() => {
                Reaction::Launch(speed)
            }
            Stance::Upright => Reaction::Shove(speed),
            Stance::KnockedDown { .. } => Reaction::Juggle(speed),
            Stance::Righting { .. } | Stance::Hooked | Stance::KnockedOut { .. } => {
                Reaction::Ignored
            }
        }
    }

    /// Puts the tank down on physics.
    pub fn knock_down(&mut self) {
        self.immunity = 0.0;
        self.stance = Stance::KnockedDown { elapsed: 0.0 };
    }

    /// Takes the tank out of the arena, counting the knockout.
    pub fn knock_out(&mut self) {
        if !self.is_knocked_out() {
            self.knockouts += 1;
            self.stance = Stance::KnockedOut { elapsed: 0.0 };
            self.immunity = 0.0;
            self.anchored = 0.0;
        }
    }

    /// Advances the state by `dt` seconds: the shields' clocks, and the
    /// recovery, righting and respawn clocks.
    pub fn tick(&mut self, dt: f32, tuning: &MeterTuning) -> Option<Transition> {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.invulnerable = (self.invulnerable - dt).max(0.0);
        self.anchored = (self.anchored - dt).max(0.0);
        match self.stance {
            Stance::Upright => {
                self.immunity = (self.immunity - dt).max(0.0);
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
            Stance::KnockedOut { elapsed } => {
                let elapsed = elapsed + dt;
                if elapsed >= tuning.respawn_delay {
                    self.stance = Stance::Upright;
                    self.percent = 0.0;
                    self.immunity = 0.0;
                    self.invulnerable = tuning.respawn_invulnerability.max(0.0);
                    Some(Transition::Respawn)
                } else {
                    self.stance = Stance::KnockedOut { elapsed };
                    None
                }
            }
        }
    }

    /// Upright, immune to launches for the immunity time.
    pub fn recover(&mut self, tuning: &MeterTuning) {
        self.stance = Stance::Upright;
        self.immunity = tuning.immunity_time.max(0.0);
    }

    /// How far through the righting the tank is, 0 to 1.
    pub fn righting_progress(&self, tuning: &MeterTuning) -> f32 {
        match self.stance {
            Stance::Righting { elapsed } if tuning.righting_time > 0.0 => {
                (elapsed / tuning.righting_time).clamp(0.0, 1.0)
            }
            Stance::Righting { .. } => 1.0,
            _ => 0.0,
        }
    }
}

/// Whether a hook held for `elapsed` seconds lets go: when the key is
/// released, or when the swing's time limit runs out and the line snaps.
pub fn hook_lets_go(held: bool, elapsed: f32, time_limit: f32) -> bool {
    !held || elapsed >= time_limit
}

/// Whether `position` is on a square patch centred at `center` with
/// `half_size`, measured on the ground plane.
pub fn on_patch(center: Vec3, half_size: f32, position: Vec3) -> bool {
    (position.x - center.x).abs() <= half_size && (position.z - center.z).abs() <= half_size
}

/// The velocity a bumper mine launches a tank at `percent` with: straight
/// up and a push out along `away`, both growing with the meter.
pub fn bumper_launch(lift: f32, push: f32, growth: f32, percent: f32, away: Vec3) -> Vec3 {
    let scale = knockback_speed(1.0, growth, percent);
    let away = vec3(away.x, 0.0, away.z);
    let away = away.try_normalize(1e-6).unwrap_or_else(Vec3::zeros);
    Vec3::y() * lift.max(0.0) * scale + away * push.max(0.0) * scale
}

/// Directions tried on each ring of [`nearest_free_spot`]'s search.
const FREE_SPOT_DIRECTIONS: usize = 16;

/// The free spot nearest `start` on the ground plane, as `is_free` judges
/// spots: `start` itself when it is free, else the first free spot on rings
/// `step` metres apart, up to `max_distance` out. On each ring the spots
/// nearest the way to `toward` are tried first, so a tank is moved back
/// into the arena rather than out of it. None when nothing within reach is
/// free.
pub fn nearest_free_spot(
    start: Vec3,
    toward: Vec3,
    step: f32,
    max_distance: f32,
    is_free: impl Fn(Vec3) -> bool,
) -> Option<Vec3> {
    if is_free(start) {
        return Some(start);
    }
    let step = step.max(0.01);
    let preferred = vec3(toward.x - start.x, 0.0, toward.z - start.z)
        .try_normalize(1e-6)
        .unwrap_or_else(Vec3::z);
    let mut directions: Vec<Vec3> = (0..FREE_SPOT_DIRECTIONS)
        .map(|i| {
            let angle = i as f32 / FREE_SPOT_DIRECTIONS as f32 * std::f32::consts::TAU;
            vec3(angle.cos(), 0.0, angle.sin())
        })
        .collect();
    directions.sort_by(|a, b| b.dot(&preferred).total_cmp(&a.dot(&preferred)));
    let rings = (max_distance / step).floor() as usize;
    (1..=rings).find_map(|ring| {
        let radius = ring as f32 * step;
        directions
            .iter()
            .map(|direction| start + direction * radius)
            .find(|&spot| is_free(spot))
    })
}

/// The longest step the swing's spring integrates at once, in seconds.
const SWING_STEP: f32 = 1.0 / 240.0;

/// The tuning a hook swing needs: the swing chases the angle the player's
/// turret points at with an angular spring, limited in acceleration and
/// speed, so a fast turn of the turret whips the hooked tank round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingTuning {
    /// The radius the swing reels in to, in metres; a shorter line stays.
    pub radius: f32,
    /// How fast the radius changes towards it, in metres per second.
    pub reel_speed: f32,
    /// How hard the swing pulls towards the aimed angle, in radians per
    /// second squared per radian off.
    pub stiffness: f32,
    /// How much the swing's own speed is damped, per second.
    pub damping: f32,
    /// The most the swing's angular speed changes, in radians per second
    /// squared.
    pub max_acceleration: f32,
    /// Top angular speed, in radians per second.
    pub max_speed: f32,
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

    /// Swings towards `target_angle` (from +X towards +Z, in radians) for
    /// `dt` seconds and reels the line in towards the tuned radius, never
    /// out. The swing goes the short way round, so a turret turning round
    /// keeps it circling, and the further the turret is ahead the harder it
    /// pulls.
    pub fn follow(&mut self, target_angle: f32, dt: f32, tuning: &SwingTuning) {
        use std::f32::consts::{PI, TAU};
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        // Small steps keep the spring stable at any frame rate.
        let steps = (dt / SWING_STEP).ceil().max(1.0) as u32;
        let h = dt / steps as f32;
        let max_acceleration = tuning.max_acceleration.max(0.0);
        let max_speed = tuning.max_speed.max(0.0);
        for _ in 0..steps {
            let error = (target_angle - self.angle + PI).rem_euclid(TAU) - PI;
            let acceleration = (tuning.stiffness.max(0.0) * error
                - tuning.damping.max(0.0) * self.angular_speed)
                .clamp(-max_acceleration, max_acceleration);
            self.angular_speed =
                (self.angular_speed + acceleration * h).clamp(-max_speed, max_speed);
            self.angle += self.angular_speed * h;
        }
        // The line only ever shortens: a tank hooked inside the swing radius
        // stays where it was caught.
        let reel = tuning.reel_speed.max(0.0) * dt;
        if self.radius > tuning.radius.max(0.0) {
            self.radius = (self.radius - reel).max(tuning.radius.max(0.0));
        }
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

/// Where along a boot's flight from `origin` along the unit `direction`,
/// between `from` and `to` metres out, a boot head of `radius` first touches
/// one of `targets` (each a centre and a radius), and which one. Distances
/// are measured on the ground plane.
pub fn boot_hit(
    origin: Vec3,
    direction: Vec3,
    from: f32,
    to: f32,
    radius: f32,
    targets: &[(Vec3, f32)],
) -> Option<(usize, f32)> {
    let flat = |v: Vec3| vec3(v.x, 0.0, v.z);
    let origin = flat(origin);
    let direction = flat(direction);
    targets
        .iter()
        .enumerate()
        .filter_map(|(index, &(center, target_radius))| {
            let reach = radius.max(0.0) + target_radius.max(0.0);
            let offset = flat(center) - origin;
            let along = offset.dot(&direction);
            let across_squared = (offset - direction * along).magnitude_squared();
            if across_squared > reach * reach {
                return None;
            }
            // The first distance out at which the head is within reach.
            let entry = (along - (reach * reach - across_squared).sqrt()).max(from);
            (entry <= to && along + reach >= from).then_some((index, entry))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> MeterTuning {
        MeterTuning {
            recovery_time: 2.0,
            righting_time: 0.5,
            immunity_time: 1.0,
            respawn_delay: 1.0,
            respawn_invulnerability: 2.0,
        }
    }

    #[test]
    fn knockback_grows_with_the_meter_and_launches_past_the_threshold() {
        assert_eq!(knockback_speed(10.0, 0.01, 0.0), 10.0);
        assert!((knockback_speed(10.0, 0.01, 100.0) - 20.0).abs() < 1e-5);
        assert!(knockback_speed(10.0, 0.01, 150.0) > knockback_speed(10.0, 0.01, 50.0));

        let mut state = MeterState::default();
        // A weak hit at a low meter only shoves; the meter counts up.
        let speed = knockback_speed(2.5, 0.012, state.percent + 5.0);
        assert_eq!(state.hit(5.0, speed, 11.0), Reaction::Shove(speed));
        assert_eq!(state.percent, 5.0);
        // The meter never caps, and the same hit grows with it.
        for _ in 0..60 {
            state.hit(5.0, 0.0, 11.0);
        }
        assert_eq!(state.percent, 305.0);
        let speed = knockback_speed(2.5, 0.012, state.percent);
        assert!(speed >= 11.0);
        assert_eq!(state.hit(0.0, speed, 11.0), Reaction::Launch(speed));
        // Invalid damage adds nothing.
        for invalid in [-5.0, f32::NAN, f32::INFINITY] {
            state.hit(invalid, 0.0, 11.0);
        }
        assert_eq!(state.percent, 305.0);
    }

    #[test]
    fn a_launch_tumbles_recovers_and_grants_immunity() {
        let tuning = tuning();
        let mut state = MeterState::default();
        state.knock_down();
        // A tumbling tank takes the meter and a push.
        assert_eq!(state.hit(4.0, 6.0, 11.0), Reaction::Juggle(6.0));
        assert_eq!(state.tick(1.9, &tuning), None);
        assert_eq!(state.tick(0.2, &tuning), Some(Transition::StartRighting));
        assert_eq!(state.hit(4.0, 20.0, 11.0), Reaction::Ignored);
        assert_eq!(state.tick(0.6, &tuning), Some(Transition::Recovered));
        assert!(state.is_immune());
        // Immune: a strong hit only shoves.
        assert_eq!(state.hit(10.0, 30.0, 11.0), Reaction::Shove(30.0));
        state.tick(1.1, &tuning);
        assert_eq!(state.hit(10.0, 30.0, 11.0), Reaction::Launch(30.0));
        // The meter did not reset: only a knockout resets it.
        assert_eq!(state.percent, 28.0);
    }

    #[test]
    fn a_knockout_respawns_at_zero_and_invulnerable() {
        let tuning = tuning();
        let mut state = MeterState {
            percent: 120.0,
            ..Default::default()
        };
        state.knock_out();
        state.knock_out();
        assert_eq!(state.knockouts, 1, "one knockout, however often asked");
        assert_eq!(state.hit(10.0, 40.0, 11.0), Reaction::Ignored);
        assert_eq!(state.percent, 120.0);
        assert_eq!(state.tick(0.5, &tuning), None);
        assert_eq!(state.tick(0.6, &tuning), Some(Transition::Respawn));
        assert!(state.is_upright());
        assert_eq!(state.percent, 0.0);
        assert!(state.is_invulnerable());
        // Invulnerable: hits change nothing at all.
        assert_eq!(state.hit(10.0, 40.0, 11.0), Reaction::Ignored);
        assert_eq!(state.percent, 0.0);
        state.tick(2.1, &tuning);
        assert!(!state.is_invulnerable());
        assert_eq!(state.hit(10.0, 4.0, 11.0), Reaction::Shove(4.0));
    }

    #[test]
    fn an_anchor_takes_the_meter_but_no_knockback() {
        let mut state = MeterState {
            anchored: 1.0,
            ..Default::default()
        };
        assert_eq!(state.hit(16.0, 40.0, 11.0), Reaction::Ignored);
        assert_eq!(state.percent, 16.0);
        state.tick(1.1, &tuning());
        assert_eq!(state.hit(16.0, 40.0, 11.0), Reaction::Launch(40.0));
    }

    #[test]
    fn a_blocked_spot_moves_to_the_nearest_free_one_towards_the_arena() {
        // A wall fills z < -15.4; a hull 1 m deep is free 1 m clear of it.
        let free = |spot: Vec3| spot.z - 1.0 >= -15.4;
        let start = vec3(10.0, 0.0, -16.0);
        let spot = nearest_free_spot(start, Vec3::zeros(), 0.25, 8.0, free).unwrap();
        assert!(free(spot));
        assert!((spot - start).magnitude() <= 1.75, "{spot:?}");
        assert!(spot.z > start.z, "back towards the arena");
        // A free spot stays where it is.
        let open = vec3(3.0, 0.0, 2.0);
        assert_eq!(
            nearest_free_spot(open, Vec3::zeros(), 0.25, 8.0, free),
            Some(open)
        );
        // Of two equally near ways out, the one towards the arena wins.
        let pillar = |spot: Vec3| spot.x.abs() >= 1.0;
        let spot = nearest_free_spot(Vec3::zeros(), vec3(-10.0, 0.0, 0.0), 0.25, 8.0, pillar);
        assert!(spot.unwrap().x < 0.0);
        // Nothing within reach is free.
        assert_eq!(
            nearest_free_spot(start, Vec3::zeros(), 0.25, 2.0, |_| false),
            None
        );
    }

    #[test]
    fn a_hook_snaps_when_its_time_runs_out() {
        assert!(!hook_lets_go(true, 1.0, 1.75));
        assert!(hook_lets_go(true, 1.75, 1.75));
        assert!(hook_lets_go(false, 0.1, 1.75));
    }

    #[test]
    fn bumpers_launch_harder_with_the_meter_and_oil_covers_its_patch() {
        let low = bumper_launch(9.0, 4.0, 0.01, 0.0, vec3(1.0, 0.0, 0.0));
        let high = bumper_launch(9.0, 4.0, 0.01, 100.0, vec3(1.0, 0.0, 0.0));
        assert_eq!(low, vec3(4.0, 9.0, 0.0));
        assert!((high - low * 2.0).magnitude() < 1e-4);
        // Straight up when the tank sits on the mine's centre.
        assert_eq!(
            bumper_launch(9.0, 4.0, 0.0, 0.0, Vec3::zeros()),
            vec3(0.0, 9.0, 0.0)
        );

        let center = vec3(2.0, 0.0, 2.0);
        assert!(on_patch(center, 1.5, vec3(3.0, 0.5, 1.0)));
        assert!(!on_patch(center, 1.5, vec3(4.0, 0.0, 2.0)));
    }

    fn swing_tuning() -> SwingTuning {
        SwingTuning {
            radius: 5.0,
            reel_speed: 10.0,
            stiffness: 60.0,
            damping: 9.0,
            max_acceleration: 80.0,
            max_speed: 6.0,
        }
    }

    #[test]
    fn a_swing_follows_the_turret_the_short_way_and_only_reels_in() {
        let tuning = swing_tuning();
        let mut swing = Swing::starting_at(Vec3::zeros(), vec3(8.0, 0.0, 0.0));
        assert_eq!(swing.radius, 8.0);
        assert_eq!(swing.angle, 0.0);
        // A turret a quarter turn anticlockwise pulls it anticlockwise...
        swing.follow(1.5, 0.1, &tuning);
        assert!(swing.angular_speed > 0.0);
        assert!((swing.radius - 7.0).abs() < 1e-5);
        // ...and it settles there.
        for _ in 0..40 {
            swing.follow(1.5, 0.05, &tuning);
        }
        assert!((swing.angle - 1.5).abs() < 0.01, "{swing:?}");
        assert!(swing.angular_speed.abs() < 0.05);
        assert!((swing.radius - 5.0).abs() < 1e-5);
        // A tank caught inside the swing radius is never pushed out.
        let mut close = Swing {
            angle: 0.0,
            angular_speed: 0.0,
            radius: 3.0,
        };
        close.follow(2.0, 1.0, &tuning);
        assert_eq!(close.radius, 3.0);
        // A turret just past the +-pi seam is reached the short way.
        let mut seam = Swing {
            angle: 3.0,
            angular_speed: 0.0,
            radius: 5.0,
        };
        seam.follow(-3.0, 0.05, &tuning);
        assert!(seam.angular_speed > 0.0, "{seam:?}");
        let mut clockwise = Swing {
            angle: 0.0,
            angular_speed: 0.0,
            radius: 5.0,
        };
        clockwise.follow(-1.0, 0.05, &tuning);
        assert!(clockwise.angular_speed < 0.0);
    }

    #[test]
    fn a_fast_turret_turn_whips_faster_up_to_the_top_speed() {
        let tuning = swing_tuning();
        // The turret turns at `rate` radians per second; the swing chases it.
        let chase = |rate: f32, steps: usize| {
            let mut swing = Swing {
                angle: 0.0,
                angular_speed: 0.0,
                radius: 5.0,
            };
            let mut turret = 0.0f32;
            for _ in 0..steps {
                turret += rate * 0.02;
                swing.follow(turret, 0.02, &tuning);
            }
            swing.angular_speed
        };
        let slow = chase(1.0, 60);
        let fast = chase(4.0, 60);
        assert!(fast > slow * 2.0, "slow {slow} fast {fast}");
        // A turret turning faster than the swing can go: it tops out.
        assert!((chase(9.0, 20) - tuning.max_speed).abs() < 1e-4);
        assert!(chase(-4.0, 60) < -3.0);
    }

    #[test]
    fn a_boot_kicks_the_first_tank_along_its_flight() {
        let near = (vec3(0.0, 0.0, 5.0), 1.0);
        let far = (vec3(0.3, 0.0, 9.0), 1.0);
        let aside = (vec3(4.0, 0.0, 6.0), 1.0);
        let targets = [far, aside, near];
        // Out of reach this step.
        assert_eq!(
            boot_hit(Vec3::zeros(), Vec3::z(), 0.0, 3.0, 0.5, &targets),
            None
        );
        let (index, at) = boot_hit(Vec3::zeros(), Vec3::z(), 3.0, 12.0, 0.5, &targets).unwrap();
        assert_eq!(index, 2, "the nearest along the flight");
        assert!((at - 3.5).abs() < 1e-4);
        // A tank to the side is never kicked.
        assert_eq!(
            boot_hit(Vec3::zeros(), Vec3::z(), 0.0, 12.0, 0.5, &[aside]),
            None
        );
        // Nor one behind.
        let behind = (vec3(0.0, 0.0, -4.0), 1.0);
        assert_eq!(
            boot_hit(Vec3::zeros(), Vec3::z(), 0.0, 12.0, 0.5, &[behind]),
            None
        );
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
