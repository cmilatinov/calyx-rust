use engine_derive::impl_reflect_value;
use glm::Vec3;
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

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
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
pub struct ComponentCollider {
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
            shape: ColliderShape::Sphere { radius: 1.0 },
            friction: 0.5,
            density: 100.0,
            dirty: true,
        }
    }
}

impl Component for ComponentCollider {}