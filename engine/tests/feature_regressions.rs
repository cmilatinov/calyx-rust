use engine::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
use engine::test_support::{
    assert_position_near, test_registries, HeadlessSceneRunner, SceneBuilder,
};
use nalgebra_glm::Vec3;
use rapier3d::dynamics::RigidBodyType;

#[test]
fn editor_teleport_of_dynamic_body_survives_the_next_simulation_frame() {
    let mut builder = SceneBuilder::new(test_registries());
    let body = builder.spawn("Dynamic Body");
    builder.add_component(
        body,
        ComponentRigidBody {
            ty: RigidBodyType::Dynamic,
            ..Default::default()
        },
    );
    builder.add_component(
        body,
        ComponentCollider {
            shape: ColliderShape::Sphere { radius: 0.5 },
            ..Default::default()
        },
    );

    let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
    runner.prepare();

    // This models an editor gizmo moving a simulated dynamic object.
    runner
        .scene_mut()
        .set_transform(body, &nalgebra_glm::translation(&Vec3::new(3.0, 7.0, -2.0)));
    runner.step();

    assert_position_near(runner.scene(), body, Vec3::new(3.0, 7.0, -2.0), 0.01);
}

#[test]
fn deleting_a_simulated_physics_object_removes_its_runtime_handles() {
    let mut builder = SceneBuilder::new(test_registries());
    let body = builder.spawn("Temporary Physics Object");
    builder.add_component(body, ComponentRigidBody::default());
    builder.add_component(body, ComponentCollider::default());

    let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
    runner.prepare();
    assert_eq!(runner.scene().physics.bodies.len(), 1);
    assert_eq!(runner.scene().physics.colliders.len(), 1);

    runner.scene_mut().delete(body);
    runner.prepare();

    assert_eq!(runner.scene().physics.bodies.len(), 0);
    assert_eq!(runner.scene().physics.colliders.len(), 0);
}
