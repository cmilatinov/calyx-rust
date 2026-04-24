use crate::component::{
    ColliderShape, ComponentCollider, ComponentRigidBody, ComponentTransform, Orientation,
};
use crate::core::{Time, TimeType};
use crate::math::Transform;
use crate::physics::events::{CollisionEvents, ContactKind};
use crate::physics::PhysicsConfiguration;
use crate::scene::{GameObject, Scene};
use legion::{Entity, IntoQuery};
use nalgebra::{UnitQuaternion, Vector3};
use nalgebra_glm::Mat4;
use rapier3d::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
    pub(crate) entity_rigid_body: HashMap<Entity, RigidBodyHandle>,
    pub(crate) entity_collider: HashMap<Entity, ColliderHandle>,
    pub(crate) collider_entity: HashMap<ColliderHandle, Entity>,
    accumulated_time: TimeType,
    /// Collision and contact-force events from the most recent physics step.
    pub events: CollisionEvents,
}

impl PhysicsContext {
    const TIME_STEP: f32 = 1.0 / 60.0;

    fn rigid_body_from_component(
        transform: &Transform,
        rigid_body: &ComponentRigidBody,
    ) -> RigidBody {
        let (z, y, x) = transform.rotation.euler_angles();
        RigidBodyBuilder::new(rigid_body.ty)
            .translation(transform.position)
            .rotation(Vector3::new(x, y, z))
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
            .rotation(Vector3::new(x, y, z))
            .friction(collider.friction)
            .density(collider.density)
            .active_events(ActiveEvents::COLLISION_EVENTS | ActiveEvents::CONTACT_FORCE_EVENTS)
            .build()
    }

    pub fn prepare(scene: &mut Scene) {
        Self::sync_rigid_bodies(scene);
        Self::sync_colliders(scene);
        Self::sync_transforms(scene);
    }

    /// Create or update rigid body properties when the component is dirty.
    fn sync_rigid_bodies(scene: &mut Scene) {
        let mut query = <(Entity, &ComponentRigidBody)>::query();
        let mut dirty_entities: Vec<Entity> = Vec::new();
        for (entity, c_rb) in query.iter(&scene.world) {
            let Some(go) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            // Ensure a rapier body exists
            if !scene.physics.entity_rigid_body.contains_key(&go.entity) {
                let transform = scene.world_transform(go);
                let rigid_body = Self::rigid_body_from_component(&transform, c_rb);
                let handle = scene.physics.bodies.insert(rigid_body);
                scene.physics.entity_rigid_body.insert(go.entity, handle);
            }
            // Sync properties only when dirty
            if c_rb.dirty {
                let handle = scene.physics.entity_rigid_body[&go.entity];
                let rb = &mut scene.physics.bodies[handle];
                rb.set_enabled(c_rb.enabled);
                rb.set_body_type(c_rb.ty, true);
                rb.set_additional_mass(c_rb.mass, true);
                rb.set_gravity_scale(c_rb.gravity_scale, true);
                if !c_rb.can_sleep {
                    rb.activation_mut().normalized_linear_threshold = -1.0;
                    rb.activation_mut().angular_threshold = -1.0;
                }
                dirty_entities.push(*entity);
            }
        }
        for entity in dirty_entities {
            if let Some(mut entry) = scene.world.entry(entity) {
                if let Ok(c_rb) = entry.get_component_mut::<ComponentRigidBody>() {
                    c_rb.dirty = false;
                }
            }
        }
    }

    /// Create or update collider properties when the component is dirty.
    fn sync_colliders(scene: &mut Scene) {
        let mut query = <(Entity, &ComponentCollider)>::query();
        let mut dirty_entities: Vec<Entity> = Vec::new();
        for (entity, c_collider) in query.iter(&scene.world) {
            let Some(go) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            let parent = scene.ancestor_with::<ComponentRigidBody>(go);
            let rb_handle =
                parent.and_then(|p| scene.physics.entity_rigid_body.get(&p.entity).copied());
            // Ensure a rapier collider exists
            if !scene.physics.entity_collider.contains_key(&go.entity) {
                let transform = parent
                    .map(|p| scene.transform_relative_to(go, p))
                    .unwrap_or_else(|| scene.world_transform(go));
                let collider = Self::collider_from_component(&transform, c_collider);
                let handle = match rb_handle {
                    None => scene.physics.colliders.insert(collider),
                    Some(rb_handle) => scene.physics.colliders.insert_with_parent(
                        collider,
                        rb_handle,
                        &mut scene.physics.bodies,
                    ),
                };
                scene.physics.entity_collider.insert(go.entity, handle);
                scene.physics.collider_entity.insert(handle, go.entity);
            }
            // Sync properties only when dirty
            if c_collider.dirty {
                let c_handle = scene.physics.entity_collider[&go.entity];
                let c = &mut scene.physics.colliders[c_handle];
                c.set_shape(Self::collider_shape(c_collider.shape));
                c.set_friction(c_collider.friction);
                c.set_density(c_collider.density);
                dirty_entities.push(*entity);
            }
        }
        for entity in dirty_entities {
            if let Some(mut entry) = scene.world.entry(entity) {
                if let Ok(c_c) = entry.get_component_mut::<ComponentCollider>() {
                    c_c.dirty = false;
                }
            }
        }
    }

    /// Sync scene transforms → rapier for kinematic and fixed bodies only.
    /// Dynamic bodies are owned by rapier during simulation — their
    /// transforms flow back to the scene via update().
    fn sync_transforms(scene: &mut Scene) {
        let mut query = <(Entity, &ComponentRigidBody)>::query();
        for (entity, c_rb) in query.iter(&scene.world) {
            if c_rb.ty == RigidBodyType::Dynamic {
                continue;
            }
            let Some(go) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            let Some(&handle) = scene.physics.entity_rigid_body.get(&go.entity) else {
                continue;
            };
            let transform = scene.world_transform(go);
            let rb = &mut scene.physics.bodies[handle];
            rb.set_position(transform.position.into(), true);
            rb.set_rotation(transform.rotation, true);
        }
        let mut query = <(Entity, &ComponentCollider)>::query();
        for (entity, _) in query.iter(&scene.world) {
            let Some(go) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            let Some(&c_handle) = scene.physics.entity_collider.get(&go.entity) else {
                continue;
            };
            let parent = scene.ancestor_with::<ComponentRigidBody>(go);
            let transform = parent
                .map(|p| scene.transform_relative_to(go, p))
                .unwrap_or_else(|| scene.world_transform(go));
            let c = &mut scene.physics.colliders[c_handle];
            c.set_position(transform.position.into());
            c.set_rotation(transform.rotation);
        }
    }

    pub fn update(scene: &mut Scene, time: &Time, config: &PhysicsConfiguration) {
        let (raw_collisions, raw_forces) = scene.physics.step_simulation(time, config);

        // Resolve raw Rapier events directly to GameObjects.
        for event in &raw_collisions {
            let (h1, h2, started, sensor) = match *event {
                rapier3d::geometry::CollisionEvent::Started(h1, h2, flags) => (
                    h1, h2, true,
                    flags.contains(rapier3d::geometry::CollisionEventFlags::SENSOR),
                ),
                rapier3d::geometry::CollisionEvent::Stopped(h1, h2, flags) => (
                    h1, h2, false,
                    flags.contains(rapier3d::geometry::CollisionEventFlags::SENSOR),
                ),
            };
            let e1 = scene.physics.collider_entity.get(&h1).copied();
            let e2 = scene.physics.collider_entity.get(&h2).copied();
            if let (Some(a), Some(b)) = (
                e1.and_then(|e| scene.game_object_from_entity(e)),
                e2.and_then(|e| scene.game_object_from_entity(e)),
            ) {
                scene.physics.events.collisions.push(
                    crate::physics::events::CollisionEvent {
                        object_a: a,
                        object_b: b,
                        kind: if started { ContactKind::Started } else { ContactKind::Stopped },
                        sensor,
                    },
                );
            }
        }
        for &(h1, h2, magnitude) in &raw_forces {
            let e1 = scene.physics.collider_entity.get(&h1).copied();
            let e2 = scene.physics.collider_entity.get(&h2).copied();
            if let (Some(a), Some(b)) = (
                e1.and_then(|e| scene.game_object_from_entity(e)),
                e2.and_then(|e| scene.game_object_from_entity(e)),
            ) {
                scene.physics.events.contact_forces.push(
                    crate::physics::events::ContactForceEvent {
                        object_a: a,
                        object_b: b,
                        total_force_magnitude: magnitude,
                    },
                );
            }
        }

        let mut query = <(Entity, &ComponentTransform, &ComponentRigidBody)>::query();
        let mut transforms: HashMap<GameObject, Mat4> = Default::default();
        for (entity, _, _) in query.iter(&scene.world) {
            if let Some(go) = scene.game_object_from_entity(*entity) {
                if let Some(rb_handle) = scene.physics.entity_rigid_body.get(entity).copied() {
                    let rb = &scene.physics.bodies[rb_handle];
                    let old_transform = scene.world_transform(go);
                    let transform = Transform::from_components(
                        *rb.translation(),
                        UnitQuaternion::from(*rb.rotation()),
                        old_transform.scale,
                    )
                    .matrix();
                    transforms.insert(go, transform);
                }
            }
        }
        for (go, transform) in transforms {
            scene.set_world_transform(go, transform);
        }
        scene.clear_transform_cache();
    }

    pub fn step_simulation(
        &mut self,
        time: &Time,
        config: &PhysicsConfiguration,
    ) -> (
        Vec<rapier3d::geometry::CollisionEvent>,
        Vec<(ColliderHandle, ColliderHandle, f32)>,
    ) {
        self.events.clear();

        let raw_collisions: Arc<Mutex<Vec<rapier3d::geometry::CollisionEvent>>> =
            Arc::new(Mutex::new(Vec::new()));
        let raw_forces: Arc<Mutex<Vec<(ColliderHandle, ColliderHandle, f32)>>> =
            Arc::new(Mutex::new(Vec::new()));

        let collector = PhysicsEventCollector {
            collisions: Arc::clone(&raw_collisions),
            forces: Arc::clone(&raw_forces),
        };

        self.accumulated_time += time.delta_time * time.time_scale;
        while self.accumulated_time >= Self::TIME_STEP {
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
                &collector,
            );
            self.accumulated_time -= Self::TIME_STEP;
        }

        drop(collector);

        let collisions = Arc::try_unwrap(raw_collisions)
            .unwrap()
            .into_inner()
            .unwrap();
        let forces = Arc::try_unwrap(raw_forces)
            .unwrap()
            .into_inner()
            .unwrap();
        (collisions, forces)
    }
}

/// Collects Rapier physics events during `step_simulation()`.
///
/// Uses `Arc<Mutex<Vec<_>>>` because Rapier's `EventHandler` requires `Send + Sync`.
struct PhysicsEventCollector {
    collisions: Arc<Mutex<Vec<rapier3d::geometry::CollisionEvent>>>,
    forces: Arc<Mutex<Vec<(ColliderHandle, ColliderHandle, f32)>>>,
}

impl EventHandler for PhysicsEventCollector {
    fn handle_collision_event(
        &self,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        event: rapier3d::geometry::CollisionEvent,
        _contact_pair: Option<&ContactPair>,
    ) {
        if let Ok(mut collisions) = self.collisions.lock() {
            collisions.push(event);
        }
    }

    fn handle_contact_force_event(
        &self,
        _dt: Real,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        contact_pair: &ContactPair,
        total_force_magnitude: Real,
    ) {
        if let Ok(mut forces) = self.forces.lock() {
            forces.push((
                contact_pair.collider1,
                contact_pair.collider2,
                total_force_magnitude,
            ));
        }
    }
}
