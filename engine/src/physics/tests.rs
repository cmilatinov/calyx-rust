#[cfg(test)]
mod tests {
    use crate::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
    use crate::core::Time;
    use crate::physics::{PhysicsConfiguration, PhysicsContext};
    use crate::test_utils::test_scene;
    use nalgebra_glm::Vec3;
    use rapier3d::dynamics::RigidBodyType;

    fn time_with_delta(dt: f32) -> Time {
        let mut time = Time::new(120.0);
        time.delta_time = dt;
        time.time_scale = 1.0;
        time
    }

    #[test]
    fn prepare_registers_rigid_body() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(go, ComponentRigidBody::default());

        scene.prepare();

        assert!(!scene.physics.bodies.is_empty());
    }

    #[test]
    fn prepare_registers_collider() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(go, ComponentRigidBody::default());
        scene.add_component(go, ComponentCollider::default());

        scene.prepare();

        assert!(!scene.physics.colliders.is_empty());
    }

    #[test]
    fn dynamic_body_falls_under_gravity() {
        let mut scene = test_scene();
        let go = scene.create(None, None);

        scene.set_transform(go, &nalgebra_glm::translation(&Vec3::new(0.0, 10.0, 0.0)));
        scene.add_component(
            go,
            ComponentRigidBody {
                ty: RigidBodyType::Dynamic,
                ..Default::default()
            },
        );
        scene.add_component(
            go,
            ComponentCollider {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..Default::default()
            },
        );

        scene.prepare();

        let time = time_with_delta(1.0 / 60.0);
        let config = PhysicsConfiguration::default();

        // Step multiple frames to accumulate movement
        for _ in 0..60 {
            PhysicsContext::update(&mut scene, &time, &config);
        }

        let pos = scene.world_transform(go).position;
        assert!(pos.y < 10.0, "body should have fallen, y={}", pos.y);
    }

    #[test]
    fn static_body_does_not_move() {
        let mut scene = test_scene();
        let go = scene.create(None, None);

        scene.set_transform(go, &nalgebra_glm::translation(&Vec3::new(0.0, 5.0, 0.0)));
        scene.add_component(
            go,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );
        scene.add_component(
            go,
            ComponentCollider {
                shape: ColliderShape::Cuboid {
                    half_extents: Vec3::new(5.0, 0.5, 5.0),
                },
                ..Default::default()
            },
        );

        scene.prepare();

        let time = time_with_delta(1.0 / 60.0);
        let config = PhysicsConfiguration::default();

        for _ in 0..60 {
            PhysicsContext::update(&mut scene, &time, &config);
        }

        let pos = scene.world_transform(go).position;
        assert!(
            (pos.y - 5.0).abs() < 1e-5,
            "static body should not move, y={}",
            pos.y
        );
    }

    #[test]
    fn collider_attached_to_parent_rigid_body() {
        let mut scene = test_scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));

        scene.add_component(parent, ComponentRigidBody::default());
        scene.add_component(child, ComponentCollider::default());

        scene.prepare();

        assert!(!scene.physics.bodies.is_empty());
        assert!(!scene.physics.colliders.is_empty());
    }

    #[test]
    fn moving_transform_syncs_to_rapier() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(
            go,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );

        // Initial prepare registers the body
        scene.prepare();
        let entity = go.entity;
        let handle = *scene.physics.entity_rigid_body.get(&entity).unwrap();
        let pos = *scene.physics.bodies[handle].translation();
        assert!((pos.y - 0.0).abs() < 1e-5);

        // Move the game object (simulating editor gizmo drag)
        scene.set_transform(go, &nalgebra_glm::translation(&Vec3::new(0.0, 7.0, 0.0)));

        // Next prepare should sync the new position to rapier
        scene.prepare();
        let pos = *scene.physics.bodies[handle].translation();
        assert!(
            (pos.y - 7.0).abs() < 1e-5,
            "rapier body should be at y=7, got y={}",
            pos.y
        );
    }

    #[test]
    fn cuboid_collider_shape() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(go, ComponentRigidBody::default());
        scene.add_component(
            go,
            ComponentCollider {
                shape: ColliderShape::Cuboid {
                    half_extents: Vec3::new(1.0, 2.0, 3.0),
                },
                ..Default::default()
            },
        );

        scene.prepare();

        assert_eq!(scene.physics.colliders.len(), 1);
    }
}
