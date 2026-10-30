#[cfg(test)]
mod tests {
    use crate::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
    use crate::core::Time;
    use crate::math::Transform;
    use crate::physics::{PhysicsConfiguration, PhysicsContext};
    use crate::test_support::test_scene;
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::{vec3, Vec3};
    use rapier3d::dynamics::{RigidBodyActivation, RigidBodyType};
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
    fn constructed_body_and_collider_match_transform_rotation() {
        let rotation = UnitQuaternion::from_euler_angles(0.3, 0.75, -0.5);
        let transform =
            Transform::from_components(vec3(1.0, 2.0, 3.0), rotation, vec3(1.0, 1.0, 1.0));

        let body =
            PhysicsContext::rigid_body_from_component(&transform, &ComponentRigidBody::default());
        let collider =
            PhysicsContext::collider_from_component(&transform, &ComponentCollider::default());

        assert!(body.rotation().angle_to(&rotation) < 1e-5);
        assert!(collider.rotation().angle_to(&rotation) < 1e-5);
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

    #[test]
    fn child_of_dynamic_body_follows_it_after_update() {
        let mut scene = test_scene();
        let body = scene.create(None, None);
        let child = scene.create(None, Some(body));
        scene.set_transform(body, &nalgebra_glm::translation(&Vec3::new(0.0, 10.0, 0.0)));
        scene.set_transform(child, &nalgebra_glm::translation(&Vec3::new(1.0, 0.0, 0.0)));
        scene.add_component(
            body,
            ComponentRigidBody {
                ty: RigidBodyType::Dynamic,
                ..Default::default()
            },
        );
        scene.add_component(
            body,
            ComponentCollider {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..Default::default()
            },
        );
        scene.prepare();
        // Cache the child's world transform before physics moves the body.
        assert!((scene.world_transform(child).position.y - 10.0).abs() < 1e-5);

        let time = time_with_delta(1.0 / 60.0);
        let config = PhysicsConfiguration::default();
        for _ in 0..30 {
            PhysicsContext::update(&mut scene, &time, &config);
        }

        let body_world = scene.world_transform(body).position;
        let child_world = scene.world_transform(child).position;
        assert!(body_world.y < 10.0, "body should fall, y={}", body_world.y);
        assert!(
            (child_world - (body_world + Vec3::new(1.0, 0.0, 0.0))).norm() < 1e-4,
            "child {child_world:?} should follow body {body_world:?}"
        );
    }

    #[test]
    fn prepare_removes_body_when_rigid_body_component_removed() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(go, ComponentRigidBody::default());
        scene.add_component(go, ComponentCollider::default());
        scene.prepare();
        assert_eq!(scene.physics.bodies.len(), 1);

        scene.remove_component::<ComponentRigidBody>(go);
        scene.prepare();

        assert_eq!(scene.physics.bodies.len(), 0);
        assert!(!scene.physics.entity_rigid_body.contains_key(&go.entity));
        // The collider survives, detached, and is now a standalone collider.
        assert_eq!(scene.physics.colliders.len(), 1);
        let handle = scene.physics.entity_collider[&go.entity];
        assert!(scene.physics.colliders[handle].parent().is_none());
    }

    #[test]
    fn prepare_removes_collider_when_collider_component_removed() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(go, ComponentRigidBody::default());
        scene.add_component(go, ComponentCollider::default());
        scene.prepare();
        assert_eq!(scene.physics.colliders.len(), 1);

        scene.remove_component::<ComponentCollider>(go);
        scene.prepare();

        assert_eq!(scene.physics.colliders.len(), 0);
        assert!(!scene.physics.entity_collider.contains_key(&go.entity));
        assert!(scene.physics.collider_entity.is_empty());
        assert_eq!(scene.physics.bodies.len(), 1);
    }

    /// Builds a dynamic ball above a fixed floor and returns `(ball, floor_col)`.
    fn ball_over_floor(
        scene: &mut crate::scene::Scene,
        floor_enabled: bool,
    ) -> (crate::scene::GameObject, crate::scene::GameObject) {
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
                enabled: floor_enabled,
                shape: ColliderShape::Cuboid {
                    half_extents: Vec3::new(10.0, 0.1, 10.0),
                },
                ..Default::default()
            },
        );
        (ball, floor_col)
    }

    fn step(scene: &mut crate::scene::Scene, frames: usize) {
        let time = time_with_delta(1.0 / 60.0);
        let config = PhysicsConfiguration::default();
        for _ in 0..frames {
            PhysicsContext::update(scene, &time, &config);
        }
    }

    #[test]
    fn disabled_collider_lets_bodies_pass_through() {
        let mut scene = test_scene();
        let (ball, floor_col) = ball_over_floor(&mut scene, false);
        scene.prepare();

        let handle = scene.physics.entity_collider[&floor_col.entity];
        assert!(!scene.physics.colliders[handle].is_enabled());

        step(&mut scene, 120);

        let y = scene.world_transform(ball).position.y;
        assert!(y < -1.0, "ball should fall through a disabled floor, y={y}");
    }

    #[test]
    fn enabled_collider_stops_bodies() {
        let mut scene = test_scene();
        let (ball, _) = ball_over_floor(&mut scene, true);
        scene.prepare();

        step(&mut scene, 120);

        let y = scene.world_transform(ball).position.y;
        assert!(y > 0.0, "ball should rest on an enabled floor, y={y}");
    }

    #[test]
    fn toggling_collider_enabled_applies_to_physics() {
        let mut scene = test_scene();
        let (ball, floor_col) = ball_over_floor(&mut scene, true);
        scene.prepare();
        step(&mut scene, 120);
        assert!(scene.world_transform(ball).position.y > 0.0);

        scene.write_component::<ComponentCollider, _>(floor_col, |collider| {
            collider.enabled = false;
        });
        scene.prepare();
        let handle = scene.physics.entity_collider[&floor_col.entity];
        assert!(!scene.physics.colliders[handle].is_enabled());

        step(&mut scene, 120);
        let y = scene.world_transform(ball).position.y;
        assert!(
            y < -1.0,
            "ball should fall once the floor is disabled, y={y}"
        );

        scene.write_component::<ComponentCollider, _>(floor_col, |collider| {
            collider.enabled = true;
        });
        scene.prepare();
        assert!(scene.physics.colliders[handle].is_enabled());
    }

    /// Builds a dynamic body with a unit sphere collider on the same object and
    /// syncs it into the physics world.
    fn synced_body(scene: &mut crate::scene::Scene) -> crate::scene::GameObject {
        let go = scene.create(None, None);
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
                shape: ColliderShape::Sphere { radius: 1.0 },
                ..Default::default()
            },
        );
        scene.prepare();
        go
    }

    fn collider_radius(scene: &crate::scene::Scene, go: crate::scene::GameObject) -> f32 {
        let handle = scene.physics.entity_collider[&go.entity];
        scene.physics.colliders[handle]
            .shape()
            .as_ball()
            .expect("collider should be a ball")
            .radius
    }

    fn body_type(scene: &crate::scene::Scene, go: crate::scene::GameObject) -> RigidBodyType {
        let handle = scene.physics.entity_rigid_body[&go.entity];
        scene.physics.bodies[handle].body_type()
    }

    /// Syncs physics and asserts the edits every write-path test makes, a
    /// radius of 2.5 and a fixed body, reached the physics world.
    fn assert_resized_and_fixed(scene: &mut crate::scene::Scene, go: crate::scene::GameObject) {
        scene.prepare();
        assert_eq!(collider_radius(scene, go), 2.5);
        assert_eq!(body_type(scene, go), RigidBodyType::Fixed);
    }

    #[test]
    fn write_component_edits_reach_physics() {
        let mut scene = test_scene();
        let go = synced_body(&mut scene);

        scene.write_component::<ComponentCollider, _>(go, |c| {
            c.shape = ColliderShape::Sphere { radius: 2.5 }
        });
        scene.write_component::<ComponentRigidBody, _>(go, |c| c.ty = RigidBodyType::Fixed);

        assert_resized_and_fixed(&mut scene, go);
    }

    #[test]
    fn inspector_edits_reach_physics() {
        let mut scene = test_scene();
        let go = synced_body(&mut scene);

        // The inspector edits a component through `edit_component_dyn`, with a
        // registry prototype as the type-erased handle.
        scene.edit_component_dyn(go, &ComponentCollider::default(), |instance, _| {
            let c = instance
                .as_reflect_mut()
                .downcast_mut::<ComponentCollider>()
                .expect("instance should be a collider");
            c.shape = ColliderShape::Sphere { radius: 2.5 };
            crate::scene::ComponentEdit {
                value: (),
                written: true,
            }
        });
        scene.edit_component_dyn(go, &ComponentRigidBody::default(), |instance, _| {
            let c = instance
                .as_reflect_mut()
                .downcast_mut::<ComponentRigidBody>()
                .expect("instance should be a rigid body");
            c.ty = RigidBodyType::Fixed;
            crate::scene::ComponentEdit {
                value: (),
                written: true,
            }
        });

        assert_resized_and_fixed(&mut scene, go);
    }

    /// Writes `value` at `path` inside `prototype`'s component on `go` by
    /// deserializing the component in place, as the editor's undo does.
    fn restore_value(
        scene: &mut crate::scene::Scene,
        go: crate::scene::GameObject,
        prototype: &dyn crate::component::Component,
        path: &[&str],
        value: serde_json::Value,
    ) {
        let restored = scene.write_component_dyn(go, prototype, |instance| {
            let mut serialized = instance.serialize().expect("component serializes");
            let mut target = &mut serialized;
            for segment in path {
                target = &mut target[*segment];
            }
            *target = value;
            instance.deserialize_in_place(&serialized)
        });
        assert_eq!(restored, Some(true));
    }

    #[test]
    fn undo_restores_reach_physics() {
        let mut scene = test_scene();
        let go = synced_body(&mut scene);
        let fixed = serde_json::to_value(RigidBodyType::Fixed).expect("body type serializes");

        restore_value(
            &mut scene,
            go,
            &ComponentCollider::default(),
            &["shape", "Sphere", "radius"],
            serde_json::json!(2.5),
        );
        restore_value(
            &mut scene,
            go,
            &ComponentRigidBody::default(),
            &["ty"],
            fixed,
        );

        assert_resized_and_fixed(&mut scene, go);
    }

    #[test]
    fn write_after_sync_applies_on_next_sync() {
        let mut scene = test_scene();
        let go = synced_body(&mut scene);
        step(&mut scene, 1);

        scene.write_component::<ComponentCollider, _>(go, |c| {
            c.shape = ColliderShape::Sphere { radius: 2.5 }
        });
        scene.write_component::<ComponentRigidBody, _>(go, |c| c.ty = RigidBodyType::Fixed);
        step(&mut scene, 1);
        assert_eq!(collider_radius(&scene, go), 1.0);

        assert_resized_and_fixed(&mut scene, go);
    }

    #[test]
    fn sync_does_not_record_its_own_writes() {
        let mut scene = test_scene();
        synced_body(&mut scene);

        assert!(scene.changes.take::<ComponentRigidBody>().is_empty());
        assert!(scene.changes.take::<ComponentCollider>().is_empty());
    }

    #[test]
    fn can_sleep_toggle_restores_sleep_thresholds() {
        let mut scene = test_scene();
        let go = synced_body(&mut scene);
        let handle = scene.physics.entity_rigid_body[&go.entity];

        scene.write_component::<ComponentRigidBody, _>(go, |c| c.can_sleep = false);
        scene.prepare();
        let activation = *scene.physics.bodies[handle].activation();
        assert!(activation.normalized_linear_threshold < 0.0);
        assert!(activation.angular_threshold < 0.0);

        scene.write_component::<ComponentRigidBody, _>(go, |c| c.can_sleep = true);
        scene.prepare();
        let activation = *scene.physics.bodies[handle].activation();
        assert_eq!(
            activation.normalized_linear_threshold,
            RigidBodyActivation::default_normalized_linear_threshold()
        );
        assert_eq!(
            activation.angular_threshold,
            RigidBodyActivation::default_angular_threshold()
        );
    }

    #[test]
    fn rigid_body_enabled_applies_at_creation_and_on_change() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(
            go,
            ComponentRigidBody {
                enabled: false,
                ..Default::default()
            },
        );
        scene.prepare();
        let handle = scene.physics.entity_rigid_body[&go.entity];
        assert!(!scene.physics.bodies[handle].is_enabled());

        scene.write_component::<ComponentRigidBody, _>(go, |c| c.enabled = true);
        scene.prepare();
        assert!(scene.physics.bodies[handle].is_enabled());
    }

    /// A static ground with its top face at y = 0.
    fn add_ground(scene: &mut crate::scene::Scene) {
        let ground = scene.create(None, None);
        scene.set_transform(
            ground,
            &nalgebra_glm::translation(&Vec3::new(0.0, -0.5, 0.0)),
        );
        scene.add_component(
            ground,
            ComponentCollider {
                shape: ColliderShape::Cuboid {
                    half_extents: Vec3::new(10.0, 0.5, 10.0),
                },
                ..Default::default()
            },
        );
    }

    /// Runs `frames` editor frames: a prepare, then a physics update.
    fn run_frames(scene: &mut crate::scene::Scene, frames: usize) {
        let time = time_with_delta(1.0 / 60.0);
        let config = PhysicsConfiguration::default();
        for _ in 0..frames {
            scene.prepare();
            PhysicsContext::update(scene, &time, &config);
        }
    }

    fn falling_ball(
        scene: &mut crate::scene::Scene,
        collider_on_child: bool,
    ) -> crate::scene::GameObject {
        let ball = scene.create(None, None);
        scene.set_transform(ball, &nalgebra_glm::translation(&Vec3::new(2.0, 2.0, 1.0)));
        scene.add_component(ball, ComponentRigidBody::default());
        let collider = if collider_on_child {
            scene.create(None, Some(ball))
        } else {
            ball
        };
        scene.add_component(
            collider,
            ComponentCollider {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..Default::default()
            },
        );
        ball
    }

    #[test]
    fn collider_on_the_rigid_body_object_lands_on_the_ground() {
        let mut scene = test_scene();
        add_ground(&mut scene);
        let ball = falling_ball(&mut scene, false);

        run_frames(&mut scene, 180);

        let y = scene.world_transform(ball).position.y;
        assert!(
            (y - 0.5).abs() < 0.05,
            "ball should rest on the ground, y={y}"
        );
    }

    #[test]
    fn child_collider_lands_on_the_ground() {
        let mut scene = test_scene();
        add_ground(&mut scene);
        let ball = falling_ball(&mut scene, true);

        run_frames(&mut scene, 180);

        let y = scene.world_transform(ball).position.y;
        assert!(
            (y - 0.5).abs() < 0.05,
            "ball should rest on the ground, y={y}"
        );
    }

    #[test]
    fn moved_body_collider_is_queried_at_its_new_position() {
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

        // Move the body without stepping physics, as a frame where the fixed
        // step does not run, then query right after the prepare.
        scene.set_transform(
            target,
            &nalgebra_glm::translation(&Vec3::new(0.0, 0.0, 8.0)),
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
            .expect("ray should hit the moved sphere");
        assert!((hit.toi - 7.5).abs() < 1e-4, "unexpected toi {}", hit.toi);
    }
}
