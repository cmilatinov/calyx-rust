use engine::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
use engine::test_support::{
    assert_component_value, assert_entity_exists, assert_position_near, test_registries,
    HeadlessSceneRunner, SceneBuilder,
};
use nalgebra_glm::Vec3;
use rapier3d::dynamics::RigidBodyType;

#[test]
fn headless_runner_steps_scene_and_reports_collision_events() {
    let mut builder = SceneBuilder::new(test_registries());

    let ball = builder.spawn("Ball");
    builder.set_transform(ball, nalgebra_glm::translation(&Vec3::new(0.0, 2.0, 0.0)));
    builder.add_component(
        ball,
        ComponentRigidBody {
            ty: RigidBodyType::Dynamic,
            ..Default::default()
        },
    );
    let ball_collider = builder.spawn_child(ball, "Ball Collider");
    builder.add_component(
        ball_collider,
        ComponentCollider {
            shape: ColliderShape::Sphere { radius: 0.5 },
            ..Default::default()
        },
    );

    let floor = builder.spawn("Floor");
    builder.add_component(
        floor,
        ComponentRigidBody {
            ty: RigidBodyType::Fixed,
            ..Default::default()
        },
    );
    let floor_collider = builder.spawn_child(floor, "Floor Collider");
    builder.add_component(
        floor_collider,
        ComponentCollider {
            shape: ColliderShape::Cuboid {
                half_extents: Vec3::new(10.0, 0.1, 10.0),
            },
            ..Default::default()
        },
    );

    let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
    assert_entity_exists(runner.scene(), ball);
    assert_position_near(runner.scene(), floor, Vec3::new(0.0, 0.0, 0.0), 0.001);

    let mut collided = false;
    for _ in 0..120 {
        runner.step();
        if runner
            .scene()
            .physics
            .events
            .started(ball_collider)
            .next()
            .is_some()
        {
            collided = true;
            break;
        }
    }

    assert!(
        collided,
        "expected the headless runner to surface collision events"
    );
    assert!(
        runner.scene().world_transform(ball).position.y < 2.0,
        "expected the ball to move under physics after stepping the simulation"
    );
}

#[test]
fn scene_builder_and_assertions_work_with_component_state() {
    let mut builder = SceneBuilder::new(test_registries());
    let anchor = builder.spawn("Anchor");
    builder.add_component(
        anchor,
        ComponentRigidBody {
            ty: RigidBodyType::Fixed,
            ..Default::default()
        },
    );

    let runner = HeadlessSceneRunner::from_scene(builder.finish());
    assert_entity_exists(runner.scene(), anchor);
    assert_position_near(runner.scene(), anchor, Vec3::new(0.0, 0.0, 0.0), 0.001);
    assert_component_value::<ComponentRigidBody, _, _>(
        runner.scene(),
        anchor,
        |rigid_body| rigid_body.ty,
        RigidBodyType::Fixed,
    );
}
