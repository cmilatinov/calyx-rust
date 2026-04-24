use crate::scene::GameObject;

/// Describes the type of contact between two colliders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactKind {
    /// The two colliders started touching this frame.
    Started,
    /// The two colliders stopped touching this frame.
    Stopped,
}

/// A collision event between two game objects.
#[derive(Debug, Clone, Copy)]
pub struct CollisionEvent {
    /// First game object involved in the collision.
    pub object_a: GameObject,
    /// Second game object involved in the collision.
    pub object_b: GameObject,
    /// Whether this is a contact start or stop.
    pub kind: ContactKind,
    /// True if at least one collider was marked as sensor.
    pub sensor: bool,
}

/// A contact-force event between two game objects.
#[derive(Debug, Clone, Copy)]
pub struct ContactForceEvent {
    /// First game object involved.
    pub object_a: GameObject,
    /// Second game object involved.
    pub object_b: GameObject,
    /// Total force magnitude applied at the contact.
    pub total_force_magnitude: f32,
}

/// Stores collision and contact-force events for the current frame.
///
/// Events are collected during `step_simulation()` and cleared at the
/// start of the next physics update.
#[derive(Default)]
pub struct CollisionEvents {
    pub(crate) collisions: Vec<CollisionEvent>,
    pub(crate) contact_forces: Vec<ContactForceEvent>,
    /// Raw entity pairs from step_simulation, resolved to GameObjects in Scene::update.
    pub(crate) raw_collision_pairs: Vec<(legion::Entity, legion::Entity, ContactKind, bool)>,
    pub(crate) raw_force_pairs: Vec<(legion::Entity, legion::Entity, f32)>,
}

impl CollisionEvents {
    /// All collision events (start + stop) from the last physics step.
    pub fn collisions(&self) -> &[CollisionEvent] {
        &self.collisions
    }

    /// All contact-force events from the last physics step.
    pub fn contact_forces(&self) -> &[ContactForceEvent] {
        &self.contact_forces
    }

    /// Collisions that started this frame involving `object`.
    pub fn started(&self, object: GameObject) -> impl Iterator<Item = &CollisionEvent> {
        self.collisions.iter().filter(move |e| {
            e.kind == ContactKind::Started
                && (e.object_a == object || e.object_b == object)
        })
    }

    /// Collisions that stopped this frame involving `object`.
    pub fn stopped(&self, object: GameObject) -> impl Iterator<Item = &CollisionEvent> {
        self.collisions.iter().filter(move |e| {
            e.kind == ContactKind::Stopped
                && (e.object_a == object || e.object_b == object)
        })
    }

    pub(crate) fn clear(&mut self) {
        self.collisions.clear();
        self.contact_forces.clear();
        self.raw_collision_pairs.clear();
        self.raw_force_pairs.clear();
    }
}
