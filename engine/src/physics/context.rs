use crate::component::{
    ColliderShape, ComponentCollider, ComponentRigidBody, ComponentTransform, Orientation,
};
use crate::core::{Time, TimeType};
use crate::math::Transform;
use crate::physics::PhysicsConfiguration;
use crate::scene::{GameObject, Scene};
use glm::Mat4;
use legion::{Entity, IntoQuery};
use nalgebra::{UnitQuaternion, Vector3};
use rapier3d::prelude::*;
use std::collections::HashMap;

#[derive(Default)]
pub struct PhysicsContext {
    /// The island manager, which detects what object is sleeping
    /// (not moving much) to reduce computations.
    pub islands: IslandManager,
    /// The broad-phase, which detects potential contact pairs.
    pub broad_phase: DefaultBroadPhase,
    /// The narrow-phase, which computes contact points, tests intersections,
    /// and maintain the contact and intersection graphs.
    pub narrow_phase: NarrowPhase,
    /// The set of rigid-bodies part of the simulation.
    pub bodies: RigidBodySet,
    /// The set of colliders part of the simulation.
    pub colliders: ColliderSet,
    /// The set of impulse joints part of the simulation.
    pub impulse_joints: ImpulseJointSet,
    /// The set of multibody joints part of the simulation.
    pub multibody_joints: MultibodyJointSet,
    /// The solver, which handles Continuous Collision Detection (CCD).
    pub ccd_solver: CCDSolver,
    /// The physics pipeline, which advance the simulation step by step.
    pub physics_pipeline: PhysicsPipeline,
    /// The query pipeline, which performs scene queries (ray-casting, point projection, etc.)
    pub query_pipeline: QueryPipeline,
    /// The integration parameters, controlling various low-level coefficient of the simulation.
    pub integration_parameters: IntegrationParameters,
    entity_rigid_body: HashMap<Entity, RigidBodyHandle>,
    entity_collider: HashMap<Entity, ColliderHandle>,
    accumulated_time: TimeType,
}

impl PhysicsContext {
    const TIME_STEP: f32 = 1.0 / 60.0;

    fn rigid_body_from_component(
        transform: &Transform,
        rigid_body: &ComponentRigidBody,
    ) -> RigidBody {
        let (z, y, x) = transform.rotation.euler_angles();
        RigidBodyBuilder::new(rigid_body.ty)
            .translation(transform.position.into())
            .rotation(Vector3::new(x, y, z).into())
            .enabled(rigid_body.enabled)
            .additional_mass(rigid_body.mass)
            .gravity_scale(rigid_body.gravity_scale)
            .can_sleep(rigid_body.can_sleep)
            .build()
    }

    fn collider_shape(shape: ColliderShape) -> SharedShape {
        match shape {
            ColliderShape::Sphere { radius } => SharedShape::ball(radius),
            ColliderShape::Capsule {
                orientation,
                height,
                radius,
            } => match orientation {
                Orientation::X => SharedShape::capsule_x(height, radius),
                Orientation::Y => SharedShape::capsule_y(height, radius),
                Orientation::Z => SharedShape::capsule_z(height, radius),
            },
            ColliderShape::Cuboid { half_extents } => {
                SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z)
            }
            ColliderShape::Cone { height, radius } => SharedShape::cone(height, radius),
        }
    }

    fn collider_from_component(transform: &Transform, collider: &ComponentCollider) -> Collider {
        let (z, y, x) = transform.rotation.euler_angles();
        ColliderBuilder::new(Self::collider_shape(collider.shape))
            .position(transform.position.into())
            .rotation(Vector3::new(x, y, z).into())
            .friction(collider.friction)
            .density(collider.density)
            .build()
    }

    pub fn prepare(scene: &mut Scene) {
        let mut query = <(Entity, &ComponentRigidBody)>::query();
        let mut entities: Vec<Entity> = Default::default();
        for (entity, c_rigid_body) in query.iter(&scene.world).filter(|(_, c_rb)| c_rb.dirty) {
            if let Some(go) = scene.get_game_object_from_entity(*entity) {
                let transform = scene.get_world_transform(go);
                let rb_handle =
                    if let Some(handle) = scene.physics.entity_rigid_body.get(&go.entity) {
                        *handle
                    } else {
                        let rigid_body = Self::rigid_body_from_component(&transform, c_rigid_body);
                        let handle = scene.physics.bodies.insert(rigid_body);
                        scene.physics.entity_rigid_body.insert(go.entity, handle);
                        handle
                    };
                let rb = &mut scene.physics.bodies[rb_handle];
                rb.set_position(transform.position.into(), true);
                rb.set_rotation(transform.rotation, true);
                rb.set_enabled(c_rigid_body.enabled);
                rb.set_body_type(c_rigid_body.ty, true);
                rb.set_additional_mass(c_rigid_body.mass, true);
                rb.set_gravity_scale(c_rigid_body.gravity_scale, true);
                if !c_rigid_body.can_sleep {
                    rb.activation_mut().normalized_linear_threshold = -1.0;
                    rb.activation_mut().angular_threshold = -1.0;
                }
                entities.push(*entity);
            }
        }
        for entity in entities.drain(0..) {
            if let Some(mut entry) = scene.world.entry(entity) {
                if let Ok(c_rb) = entry.get_component_mut::<ComponentRigidBody>() {
                    c_rb.dirty = false;
                }
            }
        }
        let mut query = <(Entity, &ComponentCollider)>::query();
        entities.clear();
        for (entity, c_collider) in query.iter(&scene.world).filter(|(_, c_c)| c_c.dirty) {
            if let Some(go) = scene.get_game_object_from_entity(*entity) {
                let parent = scene.get_ancestor_with_component::<ComponentRigidBody>(go);
                let rb_handle =
                    parent.and_then(|parent| scene.physics.entity_rigid_body.get(&parent.entity));
                let transform = parent
                    .map(|parent| scene.get_transform_relative_to(go, parent))
                    .unwrap_or_else(|| scene.get_world_transform(go));
                let c_handle = if let Some(handle) = scene.physics.entity_collider.get(&go.entity) {
                    *handle
                } else {
                    let collider = Self::collider_from_component(&transform, c_collider);
                    let handle = match rb_handle {
                        None => scene.physics.colliders.insert(collider),
                        Some(rb_handle) => scene.physics.colliders.insert_with_parent(
                            collider,
                            *rb_handle,
                            &mut scene.physics.bodies,
                        ),
                    };
                    scene.physics.entity_collider.insert(go.entity, handle);
                    handle
                };
                let c = &mut scene.physics.colliders[c_handle];
                c.set_position(transform.position.into());
                c.set_rotation(transform.rotation);
                c.set_shape(Self::collider_shape(c_collider.shape));
                c.set_friction(c_collider.friction);
                c.set_density(c_collider.density);
                entities.push(*entity);
            }
        }
        for entity in entities.drain(0..) {
            if let Some(mut entry) = scene.world.entry(entity) {
                if let Ok(c_c) = entry.get_component_mut::<ComponentCollider>() {
                    c_c.dirty = false;
                }
            }
        }
    }

    pub fn update(scene: &mut Scene, time: &Time, config: &PhysicsConfiguration) {
        scene.physics.step_simulation(time, config);
        let mut query = <(Entity, &ComponentTransform, &ComponentRigidBody)>::query();
        let mut transforms: HashMap<GameObject, Mat4> = Default::default();
        for (entity, _, _) in query.iter(&scene.world) {
            if let Some(go) = scene.get_game_object_from_entity(*entity) {
                if let Some(rb_handle) = scene.physics.entity_rigid_body.get(entity).copied() {
                    let rb = &scene.physics.bodies[rb_handle];
                    let old_transform = scene.get_world_transform(go);
                    let transform = Transform::from_components(
                        (*rb.translation()).into(),
                        UnitQuaternion::from(*rb.rotation()),
                        old_transform.scale,
                    )
                    .matrix;
                    transforms.insert(go, transform);
                }
            }
        }
        for (go, transform) in transforms {
            scene.set_world_transform(go, transform);
        }
    }

    pub fn step_simulation(&mut self, time: &Time, config: &PhysicsConfiguration) {
        self.accumulated_time += time.delta_time * time.time_scale;
        while self.accumulated_time > Self::TIME_STEP {
            self.integration_parameters.dt = Self::TIME_STEP;
            self.physics_pipeline.step(
                &config.gravity,
                &self.integration_parameters,
                &mut self.islands,
                &mut self.broad_phase,
                &mut self.narrow_phase,
                &mut self.bodies,
                &mut self.colliders,
                &mut self.impulse_joints,
                &mut self.multibody_joints,
                &mut self.ccd_solver,
                Some(&mut self.query_pipeline),
                &(),
                &(),
            );
            self.accumulated_time -= Self::TIME_STEP;
        }
    }
}
