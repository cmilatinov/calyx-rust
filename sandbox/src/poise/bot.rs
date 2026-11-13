//! PROTOTYPE (poise_poc, #610): a simple bot for the dummy, so knockouts can
//! be tested against something that fights back. It chases the player,
//! keeps a medium distance, shoots, boots the player when it is close and
//! air-boosts back from a knockout edge. It is not meant to be smart.

use super::{
    ground_height, has_ground, in_knockout_zone, line_clear, ComponentDummyDriver,
    ComponentKnockout, DummyMode, TankControl,
};
use crate::tank::{flatten_xz, rotation_yaw};
use calyx_engine::scene::{GameObject, Scene};
use nalgebra_glm::Vec3;

/// How far ahead the bot checks for ground before driving on, in metres.
const LOOKAHEAD: f32 = 4.0;
/// The bot keeps within this band around its distance, in metres.
const DISTANCE_BAND: f32 = 2.0;

/// The tank the bot fights: the first tank with a meter that no dummy
/// drives.
pub(crate) fn player(scene: &Scene, bot: GameObject) -> Option<GameObject> {
    scene
        .objects_with::<ComponentKnockout>()
        .filter(|&tank| tank != bot)
        .find(|&tank| {
            scene
                .read_component::<ComponentDummyDriver, _, _>(tank, |_| ())
                .is_none()
        })
}

/// Whether `tank` is driven by the bot.
pub(crate) fn is_bot(scene: &Scene, tank: GameObject) -> bool {
    scene
        .read_component::<ComponentDummyDriver, _, _>(tank, |driver| driver.mode == DummyMode::Bot)
        .unwrap_or(false)
}

/// The throttle and steering that take a tank at `position` facing `yaw`
/// towards `target`, keeping `distance` from it: closing in when far,
/// backing off when close and circling it in between.
pub(crate) fn steer_towards(position: Vec3, yaw: f32, target: Vec3, distance: f32) -> (f32, f32) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    let to = target - position;
    let range = (to.x * to.x + to.z * to.z).sqrt();
    let facing = to.x.atan2(to.z);
    let (throttle, heading) = if range > distance + DISTANCE_BAND {
        (1.0, facing)
    } else if range < distance - DISTANCE_BAND {
        (-0.7, facing)
    } else {
        // Circle the target, side on.
        (0.5, facing + FRAC_PI_2)
    };
    let off = (heading - yaw + PI).rem_euclid(TAU) - PI;
    (throttle, (off * 2.0).clamp(-1.0, 1.0))
}

/// What the bot driving `tank` does this frame.
pub(crate) fn control(
    scene: &Scene,
    tank: GameObject,
    driver: &ComponentDummyDriver,
) -> TankControl {
    let pose = scene.world_transform(tank);
    let Some(target) = player(scene, tank) else {
        return TankControl::Dummy {
            throttle: 0.0,
            steer: 0.0,
        };
    };
    let aim = scene.world_transform(target).position;
    let yaw = rotation_yaw(&pose.rotation);
    let (mut throttle, steer) = steer_towards(pose.position, yaw, aim, driver.bot_distance);
    // Never drive off the ground or into a knockout zone.
    let ahead =
        pose.position + flatten_xz(pose.rotation * Vec3::z()) * LOOKAHEAD * throttle.signum();
    let unsafe_ahead = (has_ground(scene) && ground_height(scene, ahead).is_none())
        || in_knockout_zone(scene, ahead + Vec3::y() * 0.6);
    if unsafe_ahead {
        throttle = -throttle * 0.7;
    }
    TankControl::Bot {
        throttle,
        steer,
        aim,
    }
}

/// Whether the bot driving `tank` fires this frame: when its interval is up
/// and the player is in range and in sight.
pub(crate) fn fires(scene: &mut Scene, tank: GameObject, dt: f32) -> bool {
    let Some(driver) = scene.read_component::<ComponentDummyDriver, _, _>(tank, |driver| *driver)
    else {
        return false;
    };
    let timer = (driver.fire_timer - dt).max(0.0);
    let ready = timer <= 0.0
        && player(scene, tank).is_some_and(|target| {
            let from = scene.world_transform(tank).position + Vec3::y();
            let to = scene.world_transform(target).position + Vec3::y();
            (to - from).magnitude() <= driver.bot_fire_range && line_clear(scene, from, to)
        });
    let _ = scene.write_component::<ComponentDummyDriver, _>(tank, |driver| {
        driver.fire_timer = if ready {
            driver.bot_fire_interval
        } else {
            timer
        };
    });
    ready
}

/// Whether the bot driving `tank`, tumbling with `velocity`, is about to
/// leave the ground: then it boosts back towards the middle.
pub(crate) fn about_to_fall(scene: &Scene, tank: GameObject, velocity: Vec3) -> bool {
    if !has_ground(scene) {
        return false;
    }
    let position = scene.world_transform(tank).position;
    let ahead = position + Vec3::new(velocity.x, 0.0, velocity.z) * 0.5;
    ground_height(scene, ahead).is_none()
        || in_knockout_zone(scene, ahead + Vec3::y() * 0.6)
        || ground_height(scene, position).is_none()
}

#[cfg(test)]
mod tests {
    use super::steer_towards;
    use nalgebra_glm::vec3;

    #[test]
    fn the_bot_closes_in_backs_off_and_circles() {
        let target = vec3(0.0, 0.0, 20.0);
        // Far, facing it: full ahead, no steering.
        let (throttle, steer) = steer_towards(vec3(0.0, 0.0, 0.0), 0.0, target, 9.0);
        assert_eq!(throttle, 1.0);
        assert!(steer.abs() < 1e-5);
        // Far, facing away to the left: turns right towards it.
        let (_, steer) = steer_towards(vec3(0.0, 0.0, 0.0), -1.0, target, 9.0);
        assert!(steer > 0.0);
        // Too close: backs off.
        let (throttle, _) = steer_towards(vec3(0.0, 0.0, 17.0), 0.0, target, 9.0);
        assert!(throttle < 0.0);
        // In the band: circles side on.
        let (throttle, steer) = steer_towards(vec3(0.0, 0.0, 11.0), 0.0, target, 9.0);
        assert!(throttle > 0.0);
        assert!(steer > 0.0);
    }
}
