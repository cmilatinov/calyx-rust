use engine_derive::impl_reflect_value;
use nalgebra_glm::Vec3;
use serde::{Deserialize, Serialize};

use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Orientation {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, PartialEq, TypeUuid, Serialize, Deserialize)]
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

impl_reflect_value!(ColliderShape());

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
