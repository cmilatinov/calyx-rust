use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use rapier3d::dynamics::RigidBodyType;
use serde::{Deserialize, Serialize};

/// Physics rigid-body component for dynamic or kinematic simulation.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "8cac1bae-e3c3-4ee6-b672-7689e9c10f7e"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Rigid Body")]
#[serde(default)]
#[repr(C)]
pub struct ComponentRigidBody {
    /// Whether this rigid body is active.
    pub enabled: bool,
    /// Rapier rigid-body type.
    #[reflect_attr(name = "Type")]
    pub ty: RigidBodyType,
    /// Mass value used by the physics backend.
    pub mass: f32,
    /// Gravity multiplier applied to this rigid body.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub gravity_scale: f32,
    /// Whether the body may enter Rapier's sleep state.
    pub can_sleep: bool,
    /// Internal dirty flag that requests a physics rebuild.
    #[serde(skip)]
    #[reflect_skip]
    pub dirty: bool,
}

impl Default for ComponentRigidBody {
    fn default() -> Self {
        Self {
            enabled: true,
            ty: RigidBodyType::Dynamic,
            mass: 100.0,
            gravity_scale: 1.0,
            can_sleep: true,
            dirty: true,
        }
    }
}

impl Component for ComponentRigidBody {}
