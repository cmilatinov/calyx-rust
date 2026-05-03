use nalgebra_glm::Vec3;
use serde::{Deserialize, Serialize};

use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};

/// Principal axis used by collider shapes that need an orientation.
#[derive(Clone, Copy, PartialEq, TypeUuid, Serialize, Deserialize, Reflect)]
#[uuid = "6a52e396-0d72-439a-b5c7-8f93231b64da"]
pub enum Orientation {
    /// X axis.
    X,
    /// Y axis.
    Y,
    /// Z axis.
    Z,
}

/// Collider primitives supported by the physics integration.
#[derive(Clone, Copy, PartialEq, TypeUuid, Serialize, Deserialize, Reflect)]
#[uuid = "9b1a794d-df37-4abe-a7a0-c4423bb9edd3"]
pub enum ColliderShape {
    /// Sphere defined by radius.
    Sphere {
        /// Sphere radius.
        radius: f32,
    },
    /// Capsule aligned to an axis.
    Capsule {
        /// Capsule axis orientation.
        orientation: Orientation,
        /// Cylinder section height.
        height: f32,
        /// Capsule radius.
        radius: f32,
    },
    /// Axis-aligned cuboid defined by half extents.
    Cuboid {
        /// Positive half extents on each axis.
        half_extents: Vec3,
    },
    /// Cone aligned to the Y axis.
    Cone {
        /// Cone height.
        height: f32,
        /// Cone base radius.
        radius: f32,
    },
}

/// Physics collider component for a game object.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "53a682cb-a207-4c4c-8795-63f38351c7ef"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Collider")]
#[serde(default)]
#[repr(C)]
pub struct ComponentCollider {
    /// Whether the collider participates in physics.
    pub enabled: bool,
    /// Primitive shape used to build the collider.
    pub shape: ColliderShape,
    /// Surface friction coefficient.
    pub friction: f32,
    /// Density used to derive collider mass properties.
    pub density: f32,
    /// Internal dirty flag that requests a physics rebuild.
    #[serde(skip)]
    #[reflect_skip]
    pub dirty: bool,
}

impl Default for ComponentCollider {
    fn default() -> Self {
        Self {
            enabled: true,
            shape: ColliderShape::Sphere { radius: 1.0 },
            friction: 0.5,
            density: 100.0,
            dirty: true,
        }
    }
}

impl Component for ComponentCollider {}

#[cfg(test)]
mod tests {
    use super::ColliderShape;
    use crate::reflect::{ReflectedType, TypeInfo};
    use crate::utils::TypeUuid;

    #[test]
    fn collider_shape_registers_enum_type_info() {
        let mut registry = crate::reflect::type_registry::TypeRegistry {
            types: Default::default(),
        };
        ColliderShape::register(&mut registry);

        let Some(TypeInfo::Enum(info)) = registry.type_info::<ColliderShape>() else {
            panic!("expected enum type info");
        };
        assert!(info.variant("Sphere").is_some());
        assert!(info.variant("Capsule").is_some());
        assert!(info.variant("Cuboid").is_some());
        assert!(info.variant("Cone").is_some());
        assert_eq!(
            info.variant("Sphere").unwrap().fields[0].type_uuid,
            f32::type_uuid()
        );
    }
}
