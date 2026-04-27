use nalgebra_glm::Vec3;
use serde::{Deserialize, Serialize};

use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};

#[derive(Clone, Copy, PartialEq, TypeUuid, Serialize, Deserialize, Reflect)]
#[uuid = "6a52e396-0d72-439a-b5c7-8f93231b64da"]
pub enum Orientation {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, PartialEq, TypeUuid, Serialize, Deserialize, Reflect)]
#[uuid = "9b1a794d-df37-4abe-a7a0-c4423bb9edd3"]
pub enum ColliderShape {
    Sphere {
        radius: f32,
    },
    Capsule {
        orientation: Orientation,
        height: f32,
        radius: f32,
    },
    Cuboid {
        half_extents: Vec3,
    },
    Cone {
        height: f32,
        radius: f32,
    },
}

#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "53a682cb-a207-4c4c-8795-63f38351c7ef"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Collider")]
#[serde(default)]
#[repr(C)]
pub struct ComponentCollider {
    pub enabled: bool,
    pub shape: ColliderShape,
    pub friction: f32,
    pub density: f32,
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
