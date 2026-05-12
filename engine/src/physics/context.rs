use crate::component::{
    ColliderShape, ComponentCollider, ComponentRigidBody, ComponentTransform, Orientation,
};
use crate::core::{Time, TimeType};
use crate::math::Transform;
use crate::physics::events::{CollisionEvents, ContactKind};
use crate::physics::PhysicsConfiguration;
use crate::scene::{GameObject, Scene};
use legion::{Entity, IntoQuery};
use nalgebra::{Point3, Translation3, UnitQuaternion, Vector3};
use nalgebra_glm::Mat4;
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;

/// Result of a physics ray cast resolved to the engine collider entity.
#[derive(Clone, Copy, Debug)]
pub struct PhysicsRayHit {
    pub entity: Entity,
    pub collider: ColliderHandle,
    pub toi: f32,
    pub point: Vector3<f32>,
    pub normal: Vector3<f32>,
}

/// Result of a physics shape cast resolved to the engine collider entity.
#[derive(Clone, Copy, Debug)]
pub struct PhysicsShapeCastHit {
    pub entity: Entity,
    pub collider: ColliderHandle,
    pub hit: ShapeCastHit,
}

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
        scene
            .physics
            .query_pipeline
            .update(&scene.physics.colliders);
    }

    /// Casts a normalized ray through the current physics world.
    ///
    /// `max_distance` is measured in world units. A zero-length direction
    /// returns no hit.
    pub fn cast_ray(
        &self,
        origin: Vector3<f32>,
        direction: Vector3<f32>,
        max_distance: f32,
        solid: bool,
        filter: QueryFilter,
    ) -> Option<PhysicsRayHit> {
        let distance = direction.norm();
        if distance <= f32::EPSILON || max_distance <= 0.0 {
            return None;
        }

        let ray_direction = direction / distance;
        let ray = Ray::new(Point3::from(origin), ray_direction);
        let (collider, intersection) = self.query_pipeline.cast_ray_and_get_normal(
            &self.bodies,
            &self.colliders,
            &ray,
            max_distance,
            solid,
            filter,
        )?;
        let entity = *self.collider_entity.get(&collider)?;

        Some(PhysicsRayHit {
            entity,
            collider,
            toi: intersection.time_of_impact,
            point: origin + ray_direction * intersection.time_of_impact,
            normal: intersection.normal,
        })
    }

    /// Casts a collider primitive through the current physics world.
    ///
    /// The velocity vector determines the cast direction and distance. Use
    /// `max_time_of_impact` to clamp how far along that velocity the query may
    /// report hits.
    pub fn cast_shape(
        &self,
        shape: ColliderShape,
        position: Vector3<f32>,
        rotation: UnitQuaternion<f32>,
        velocity: Vector3<f32>,
        max_time_of_impact: f32,
        filter: QueryFilter,
    ) -> Option<PhysicsShapeCastHit> {
        if velocity.norm_squared() <= f32::EPSILON || max_time_of_impact <= 0.0 {
            return None;
        }

        let shape = Self::collider_shape(shape);
        let shape_pos = Isometry::from_parts(Translation3::from(position), rotation);
        let options = ShapeCastOptions::with_max_time_of_impact(max_time_of_impact);
        let (collider, hit) = self.query_pipeline.cast_shape(
            &self.bodies,
            &self.colliders,
            &shape_pos,
            &velocity,
            shape.as_ref(),
            options,
            filter,
        )?;
        let entity = *self.collider_entity.get(&collider)?;

        Some(PhysicsShapeCastHit {
            entity,
            collider,
            hit,
        })
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

    /// Sync explicit scene transform edits into Rapier before stepping.
    ///
    /// Dynamic bodies normally flow Rapier -> scene in `update()`, so their
    /// scene transforms already match their rigid bodies on the next frame.
    /// When editor tools or gameplay code intentionally teleport a dynamic
    /// object, this catches the mismatch before Rapier overwrites it with a
    /// stale body pose.
    fn sync_transforms(scene: &mut Scene) {
        let mut query = <(Entity, &ComponentRigidBody)>::query();
        for (entity, c_rb) in query.iter(&scene.world) {
            let Some(go) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            let Some(&handle) = scene.physics.entity_rigid_body.get(&go.entity) else {
                continue;
            };
            let transform = scene.world_transform(go);
            let rb = &mut scene.physics.bodies[handle];
            let position_changed =
                (*rb.translation() - transform.position).magnitude_squared() > 1e-8;
            let rotation_changed = rb.rotation().angle_to(&transform.rotation).abs() > 1e-5;
            if c_rb.ty != RigidBodyType::Dynamic || position_changed {
                rb.set_position(transform.position.into(), true);
            }
            if c_rb.ty != RigidBodyType::Dynamic || rotation_changed {
                rb.set_rotation(transform.rotation, true);
            }
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
        let collider_entity = &scene.physics.collider_entity;
        let resolve = |h: ColliderHandle| {
            collider_entity
                .get(&h)
                .and_then(|&e| scene.game_object_from_entity(e))
        };

        let collisions = raw_collisions
            .iter()
            .filter_map(|event| {
                let (h1, h2, started, sensor) = match *event {
                    rapier3d::geometry::CollisionEvent::Started(h1, h2, flags) => (
                        h1,
                        h2,
                        true,
                        flags.contains(rapier3d::geometry::CollisionEventFlags::SENSOR),
                    ),
                    rapier3d::geometry::CollisionEvent::Stopped(h1, h2, flags) => (
                        h1,
                        h2,
                        false,
                        flags.contains(rapier3d::geometry::CollisionEventFlags::SENSOR),
                    ),
                };
                Some(crate::physics::events::CollisionEvent {
                    object_a: resolve(h1)?,
                    object_b: resolve(h2)?,
                    kind: if started {
                        ContactKind::Started
                    } else {
                        ContactKind::Stopped
                    },
                    sensor,
                })
            })
            .collect();

        let contact_forces = raw_forces
            .iter()
            .filter_map(|&(h1, h2, magnitude)| {
                Some(crate::physics::events::ContactForceEvent {
                    object_a: resolve(h1)?,
                    object_b: resolve(h2)?,
                    total_force_magnitude: magnitude,
                })
            })
            .collect();

        scene.physics.events.collisions = collisions;
        scene.physics.events.contact_forces = contact_forces;

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

        let collector = PhysicsEventCollector::default();

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

        collector.take()
    }
}

/// Collects Rapier physics events during `step_simulation()`.
///
/// `Mutex` is needed because Rapier's `EventHandler` requires `Sync`
/// (the handler is called via `&self`). No `Arc` — the collector owns
/// the vecs outright and is consumed via `take()` after stepping.
#[derive(Default)]
struct PhysicsEventCollector {
    collisions: Mutex<Vec<rapier3d::geometry::CollisionEvent>>,
    forces: Mutex<Vec<(ColliderHandle, ColliderHandle, f32)>>,
}

impl PhysicsEventCollector {
    fn take(
        self,
    ) -> (
        Vec<rapier3d::geometry::CollisionEvent>,
        Vec<(ColliderHandle, ColliderHandle, f32)>,
    ) {
        (
            self.collisions.into_inner().unwrap(),
            self.forces.into_inner().unwrap(),
        )
    }
}

impl EventHandler for PhysicsEventCollector {
    fn handle_collision_event(
        &self,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        event: rapier3d::geometry::CollisionEvent,
        _contact_pair: Option<&ContactPair>,
    ) {
        self.collisions.lock().unwrap().push(event);
    }

    fn handle_contact_force_event(
        &self,
        _dt: Real,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        contact_pair: &ContactPair,
        total_force_magnitude: Real,
    ) {
        self.forces.lock().unwrap().push((
            contact_pair.collider1,
            contact_pair.collider2,
            total_force_magnitude,
        ));
    }
}
