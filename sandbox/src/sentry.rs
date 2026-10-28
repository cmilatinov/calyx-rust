use crate::spawn::ComponentRespawnState;
use crate::tank::{flatten_xz, spawn_round, yaw_rotation, ComponentProjectile};
use engine::component::{
    Component, ComponentEventContext, ComponentMesh, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use engine::input::Input;
use engine::reflect::{Reflect, ReflectDefault};
use engine::resource::ResourceMap;
use engine::scene::{GameObject, GameObjectRef, Scene};
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// A fixed turret that turns to its target and fires rounds at it while the
/// target is alive and in range. Cover absorbs its rounds like the tank's.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "3e9d6c1a-7b52-4f08-a4c3-95e1d2b7f604"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Sentry")]
#[serde(default)]
#[repr(C)]
pub struct ComponentSentry {
    pub target: GameObjectRef,
    pub range: f32,
    pub fire_interval: f32,
    pub projectile_speed: f32,
    pub projectile_damage: f32,
    pub projectile_lifetime: f32,
    pub projectile_radius: f32,
    pub muzzle_offset: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub fire_cooldown_remaining: f32,
}

impl Default for ComponentSentry {
    fn default() -> Self {
        Self {
            target: Default::default(),
            range: 16.0,
            fire_interval: 1.5,
            projectile_speed: 16.0,
            projectile_damage: 25.0,
            projectile_lifetime: 1.5,
            projectile_radius: 0.2,
            muzzle_offset: 1.2,
            fire_cooldown_remaining: 0.0,
        }
    }
}

impl Component for ComponentSentry {}

impl ComponentUpdate for ComponentSentry {
    fn update(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        update_sentry(scene, game_object, resources.time().delta_time());
    }
}

fn update_sentry(scene: &mut Scene, game_object: GameObject, dt: f32) {
    let Some(mut sentry) =
        scene.read_component::<ComponentSentry, _, _>(game_object, |sentry| *sentry)
    else {
        return;
    };
    sentry.fire_cooldown_remaining = (sentry.fire_cooldown_remaining - dt.max(0.0)).max(0.0);
    fire_at_target(scene, game_object, &mut sentry);
    let _ = scene.write_component::<ComponentSentry, _>(game_object, |component| {
        component.fire_cooldown_remaining = sentry.fire_cooldown_remaining;
    });
}

fn fire_at_target(scene: &mut Scene, game_object: GameObject, sentry: &mut ComponentSentry) {
    let Some(target) = sentry.target.game_object(scene) else {
        return;
    };
    let target_locked = scene
        .read_component::<ComponentRespawnState, _, _>(target, |state| state.is_gameplay_locked())
        .unwrap_or(false);
    if target_locked {
        return;
    }

    let mut transform = scene.world_transform(game_object);
    let offset = scene.world_transform(target).position - transform.position;
    let distance = vec_xz_length(offset);
    if distance <= f32::EPSILON || distance > sentry.range {
        return;
    }

    let direction = flatten_xz(offset);
    transform.rotation = yaw_rotation(&direction);
    scene.set_world_transform(game_object, transform.matrix());
    if sentry.fire_cooldown_remaining > f32::EPSILON {
        return;
    }

    let visual = scene.read_component::<ComponentMesh, _, _>(game_object, |mesh| ComponentMesh {
        mesh: mesh.mesh.clone(),
        material: mesh.material.clone(),
    });
    spawn_round(
        scene,
        transform.position + direction * sentry.muzzle_offset,
        transform.rotation,
        ComponentProjectile {
            direction,
            speed: sentry.projectile_speed,
            damage: sentry.projectile_damage,
            lifetime_remaining: sentry.projectile_lifetime,
            radius: sentry.projectile_radius,
            shooter: GameObjectRef::new(scene.uuid(game_object)),
        },
        visual,
    );
    sentry.fire_cooldown_remaining = sentry.fire_interval;
}

fn vec_xz_length(vector: nalgebra_glm::Vec3) -> f32 {
    (vector.x * vector.x + vector.z * vector.z).sqrt()
}

#[cfg(test)]
mod tests {
    use super::ComponentSentry;
    use crate::spawn::ComponentRespawnState;
    use crate::tank::{ComponentHealth, ComponentProjectile, ComponentProjectileTarget};
    use engine::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
    use engine::math::Transform;
    use engine::scene::{GameObject, GameObjectRef, Scene};
    use engine::test_support::HeadlessSceneRunner;
    use nalgebra_glm::vec3;
    use rapier3d::dynamics::RigidBodyType;

    /// A sentry at the origin aimed along nothing yet, and a tank `distance`
    /// metres down +Z.
    fn sentry_and_tank(distance: f32) -> (Scene, GameObject, GameObject) {
        let mut scene = engine::test_support::test_scene();
        let tank = scene.create(None, None);
        scene.set_world_transform(tank, Transform::from_xyz(0.0, 0.0, distance).matrix());
        scene.add_component(
            tank,
            ComponentProjectileTarget {
                hit_radius: 1.25,
                ..Default::default()
            },
        );
        scene.add_component(tank, ComponentHealth::default());
        scene.add_component(tank, ComponentRespawnState::default());

        let sentry = scene.create(None, None);
        scene.set_world_transform(sentry, Transform::from_xyz(0.0, 0.5, 0.0).matrix());
        let target = GameObjectRef::new(scene.uuid(tank));
        scene.add_component(
            sentry,
            ComponentSentry {
                target,
                range: 12.0,
                fire_interval: 1.0,
                projectile_damage: 25.0,
                ..Default::default()
            },
        );
        (scene, sentry, tank)
    }

    fn health(scene: &Scene, tank: GameObject) -> f32 {
        scene
            .read_component::<ComponentHealth, _, _>(tank, |health| health.current_health)
            .expect("tank should keep its health")
    }

    fn rounds(scene: &Scene) -> usize {
        scene
            .objects()
            .filter(|&object| {
                scene
                    .read_component::<ComponentProjectile, _, _>(object, |_| ())
                    .is_some()
            })
            .count()
    }

    #[test]
    fn sentry_fires_at_a_tank_in_range_and_damages_it() {
        let (scene, sentry, tank) = sentry_and_tank(8.0);
        let mut runner = HeadlessSceneRunner::from_scene(scene);

        runner.step();
        assert_eq!(rounds(runner.scene()), 1, "the sentry should fire at once");
        let facing = runner.scene().world_transform(sentry).forward();
        assert!(
            facing.z > 0.99,
            "the sentry should turn to the tank: {facing:?}"
        );

        runner.step_many(60);

        assert!(
            health(runner.scene(), tank) < 100.0,
            "a round should reach the tank and damage it"
        );
    }

    #[test]
    fn cover_between_the_sentry_and_the_tank_absorbs_its_rounds() {
        let (mut scene, _, tank) = sentry_and_tank(8.0);
        let cover = scene.create(None, None);
        scene.set_world_transform(cover, Transform::from_xyz(0.0, 0.5, 4.0).matrix());
        scene.add_component(
            cover,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );
        scene.add_component(
            cover,
            ComponentCollider {
                shape: ColliderShape::Cuboid {
                    half_extents: vec3(2.0, 1.0, 0.5),
                },
                ..Default::default()
            },
        );
        let mut runner = HeadlessSceneRunner::from_scene(scene);

        runner.step_many(120);

        assert_eq!(health(runner.scene(), tank), 100.0);
    }

    #[test]
    fn sentry_holds_fire_while_the_tank_is_out_of_range() {
        let (scene, _, tank) = sentry_and_tank(20.0);
        let mut runner = HeadlessSceneRunner::from_scene(scene);

        runner.step_many(120);

        assert_eq!(rounds(runner.scene()), 0);
        assert_eq!(health(runner.scene(), tank), 100.0);
    }

    #[test]
    fn sentry_waits_its_fire_interval_between_rounds() {
        let (mut scene, sentry, _) = sentry_and_tank(11.0);
        // Slow rounds stay in flight across the interval, so each is counted.
        let _ = scene.write_component::<ComponentSentry, _>(sentry, |sentry| {
            sentry.projectile_speed = 1.0;
        });
        let mut runner = HeadlessSceneRunner::from_scene(scene);
        runner.set_step_seconds(0.25);

        runner.step();
        runner.step_many(3);
        assert_eq!(
            rounds(runner.scene()),
            1,
            "0.75 s after the first round is inside the interval"
        );

        runner.step();
        assert_eq!(rounds(runner.scene()), 2, "1 s after the first round");
    }

    #[test]
    fn sentry_holds_fire_while_the_tank_is_dead() {
        let (mut scene, _, tank) = sentry_and_tank(8.0);
        let _ = scene.write_component::<ComponentRespawnState, _>(tank, |state| {
            state.alive = false;
            state.respawn_remaining = 10.0;
        });
        let mut runner = HeadlessSceneRunner::from_scene(scene);

        runner.step_many(30);

        assert_eq!(rounds(runner.scene()), 0);
    }
}
