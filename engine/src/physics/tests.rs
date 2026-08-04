#[cfg(test)]
mod tests {
    use crate::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
    use crate::core::Time;
    use crate::math::Transform;
    use crate::physics::{PhysicsConfiguration, PhysicsContext};
    use crate::test_utils::test_scene;
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::{vec3, Vec3};
    use rapier3d::dynamics::RigidBodyType;
    use rapier3d::pipeline::QueryFilter;

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
    fn prepare_removes_deleted_physics_objects() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(go, ComponentRigidBody::default());
        scene.add_component(go, ComponentCollider::default());
        scene.prepare();

        assert_eq!(scene.physics.bodies.len(), 1);
        assert_eq!(scene.physics.colliders.len(), 1);
        assert!(scene.physics.entity_rigid_body.contains_key(&go.entity));
        assert!(scene.physics.entity_collider.contains_key(&go.entity));

        scene.delete(go);
        scene.prepare();

        assert_eq!(scene.physics.bodies.len(), 0);
        assert_eq!(scene.physics.colliders.len(), 0);
        assert!(!scene.physics.entity_rigid_body.contains_key(&go.entity));
        assert!(!scene.physics.entity_collider.contains_key(&go.entity));
        assert!(scene.physics.collider_entity.is_empty());
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
    fn moving_dynamic_transform_syncs_to_rapier() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let rotation = UnitQuaternion::from_euler_angles(0.0, 0.75, 0.0);
        scene.set_world_transform(
            go,
            Transform::from_components(Vec3::zeros(), rotation, vec3(1.0, 1.0, 1.0)).matrix(),
        );
        scene.add_component(
            go,
            ComponentRigidBody {
                ty: RigidBodyType::Dynamic,
                ..Default::default()
            },
        );

        scene.prepare();
        let entity = go.entity;
        let handle = *scene.physics.entity_rigid_body.get(&entity).unwrap();
        let pos = *scene.physics.bodies[handle].translation();
        assert!((pos.y - 0.0).abs() < 1e-5);

        let mut moved = scene.world_transform(go);
        moved.position = Vec3::new(0.0, 7.0, 0.0);
        scene.set_world_transform(go, moved.matrix());

        scene.prepare();
        let pos = *scene.physics.bodies[handle].translation();
        let body_rotation = scene.physics.bodies[handle].rotation();
        assert!(
            (pos.y - 7.0).abs() < 1e-5,
            "dynamic rapier body should be at y=7 after an explicit transform edit, got y={}",
            pos.y
        );
        assert!(body_rotation.angle_to(&rotation) < 1e-5);

        let time = time_with_delta(0.0);
        let config = PhysicsConfiguration::default();
        PhysicsContext::update(&mut scene, &time, &config);
        let pos = scene.world_transform(go).position;
        let updated_rotation = scene.world_transform(go).rotation;
        assert!(
            (pos.y - 7.0).abs() < 1e-5,
            "dynamic scene transform should not be reset by a stale rapier pose, got y={}",
            pos.y
        );
        assert!(updated_rotation.angle_to(&rotation) < 1e-5);
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

    #[test]
    fn raycast_hits_collider_entity() {
        let mut scene = test_scene();
        let target = scene.create(None, None);
        scene.set_transform(
            target,
            &nalgebra_glm::translation(&Vec3::new(0.0, 0.0, 5.0)),
        );
        scene.add_component(
            target,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );
        scene.add_component(
            target,
            ComponentCollider {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..Default::default()
            },
        );

        scene.prepare();

        let hit = scene
            .physics
            .cast_ray(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
                10.0,
                true,
                QueryFilter::default(),
            )
            .expect("ray should hit the target sphere");

        assert_eq!(hit.entity, target.entity);
        assert!((hit.toi - 4.5).abs() < 1e-4, "unexpected toi {}", hit.toi);
        assert!(
            (hit.point.z - 4.5).abs() < 1e-4,
            "unexpected hit point {:?}",
            hit.point
        );
    }

    #[test]
    fn shape_cast_hits_collider_entity() {
        let mut scene = test_scene();
        let target = scene.create(None, None);
        scene.set_transform(
            target,
            &nalgebra_glm::translation(&Vec3::new(0.0, 0.0, 5.0)),
        );
        scene.add_component(
            target,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );
        scene.add_component(
            target,
            ComponentCollider {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..Default::default()
            },
        );

        scene.prepare();

        let hit = scene
            .physics
            .cast_shape(
                ColliderShape::Sphere { radius: 0.5 },
                Vec3::new(0.0, 0.0, 0.0),
                UnitQuaternion::identity(),
                Vec3::new(0.0, 0.0, 10.0),
                1.0,
                QueryFilter::default(),
            )
            .expect("shape cast should hit the target sphere");

        assert_eq!(hit.entity, target.entity);
        assert!(
            (hit.hit.time_of_impact - 0.4).abs() < 1e-4,
            "unexpected time of impact {}",
            hit.hit.time_of_impact
        );
    }

    #[test]
    fn collision_events_generated_on_contact() {
        let mut scene = test_scene();

        // Create a dynamic sphere that will fall onto a static floor.
        // Rigid body on parent, collider on child — matches engine convention.
        let ball = scene.create(None, None);
        scene.set_transform(ball, &nalgebra_glm::translation(&Vec3::new(0.0, 2.0, 0.0)));
        scene.add_component(
            ball,
            ComponentRigidBody {
                ty: RigidBodyType::Dynamic,
                ..Default::default()
            },
        );
        let ball_col = scene.create(None, Some(ball));
        scene.add_component(
            ball_col,
            ComponentCollider {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..Default::default()
            },
        );

        let floor = scene.create(None, None);
        scene.set_transform(floor, &nalgebra_glm::translation(&Vec3::new(0.0, 0.0, 0.0)));
        scene.add_component(
            floor,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );
        let floor_col = scene.create(None, Some(floor));
        scene.add_component(
            floor_col,
            ComponentCollider {
                shape: ColliderShape::Cuboid {
                    half_extents: Vec3::new(10.0, 0.1, 10.0),
                },
                ..Default::default()
            },
        );

        scene.prepare();

        let time = time_with_delta(1.0 / 60.0);
        let config = PhysicsConfiguration::default();

        let mut found_start = false;
        // Step enough frames for the ball to fall and hit the floor.
        for _ in 0..120 {
            PhysicsContext::update(&mut scene, &time, &config);
            if scene.physics.events.started(ball_col).count() > 0 {
                found_start = true;
            }
        }
        assert!(
            found_start,
            "expected a collision-start event between ball and floor"
        );
    }

    #[test]
    fn collision_events_cleared_each_step() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(go, ComponentRigidBody::default());
        scene.add_component(go, ComponentCollider::default());
        scene.prepare();

        let time = time_with_delta(1.0 / 60.0);
        let config = PhysicsConfiguration::default();

        // Step once — events should be empty (no contacts).
        PhysicsContext::update(&mut scene, &time, &config);
        assert!(
            scene.physics.events.collisions().is_empty(),
            "no collisions expected for a single isolated object"
        );
    }

    #[test]
    fn involves_filter_returns_matching_events() {
        let mut scene = test_scene();

        let a = scene.create(None, None);
        scene.set_transform(a, &nalgebra_glm::translation(&Vec3::new(0.0, 2.0, 0.0)));
        scene.add_component(
            a,
            ComponentRigidBody {
                ty: RigidBodyType::Dynamic,
                ..Default::default()
            },
        );
        let a_col = scene.create(None, Some(a));
        scene.add_component(
            a_col,
            ComponentCollider {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..Default::default()
            },
        );

        let b = scene.create(None, None);
        scene.add_component(
            b,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );
        let b_col = scene.create(None, Some(b));
        scene.add_component(
            b_col,
            ComponentCollider {
                shape: ColliderShape::Cuboid {
                    half_extents: Vec3::new(10.0, 0.1, 10.0),
                },
                ..Default::default()
            },
        );

        scene.prepare();

        let time = time_with_delta(1.0 / 60.0);
        let config = PhysicsConfiguration::default();

        let mut found_via_involves = false;
        for _ in 0..120 {
            PhysicsContext::update(&mut scene, &time, &config);
            if scene.physics.events.started(a_col).count() > 0 {
                found_via_involves = true;
            }
        }
        assert!(
            found_via_involves,
            "involves() should find events for collider a_col"
        );
    }
}
