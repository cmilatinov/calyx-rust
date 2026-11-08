use nalgebra_glm::Vec3;
use rapier3d::prelude::{Group, InteractionGroups, QueryFilter};
use serde::{Deserialize, Serialize};

use crate as calyx_engine;
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

/// The collision groups of a collider: the groups it belongs to and the
/// groups it interacts with, as bit masks of up to 32 groups (bit `n` is
/// group `n + 1`). Two colliders interact, and a query that carries groups
/// sees a collider, only when each one's memberships share a bit with the
/// other's filter, as Rapier's [`InteractionGroups`] decide.
///
/// The default, every group in both masks, interacts with everything, as
/// colliders did before they carried groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Reflect)]
#[uuid = "0f4c2b8e-6d1a-4e57-9a3c-b7e25d81f694"]
#[reflect(Default)]
#[serde(default)]
pub struct CollisionGroups {
    /// The groups the collider belongs to.
    pub memberships: u32,
    /// The groups the collider interacts with.
    pub filter: u32,
}

impl CollisionGroups {
    /// Every group: belongs to and interacts with all of them.
    pub const ALL: Self = Self::new(u32::MAX, u32::MAX);

    /// Groups from a membership mask and a filter mask.
    pub const fn new(memberships: u32, filter: u32) -> Self {
        Self {
            memberships,
            filter,
        }
    }

    /// Whether these are the default groups, which serialization leaves out
    /// so a collider saved before groups existed saves unchanged.
    pub fn is_all(&self) -> bool {
        *self == Self::ALL
    }

    /// Whether a collider with these groups and one with `other` interact.
    pub const fn interacts_with(&self, other: &Self) -> bool {
        self.memberships & other.filter != 0 && other.memberships & self.filter != 0
    }

    /// A query filter that sees only the colliders these groups interact
    /// with.
    pub fn query_filter(self) -> QueryFilter<'static> {
        QueryFilter::default().groups(self.into())
    }
}

impl Default for CollisionGroups {
    fn default() -> Self {
        Self::ALL
    }
}

impl From<CollisionGroups> for InteractionGroups {
    fn from(groups: CollisionGroups) -> Self {
        InteractionGroups::new(
            Group::from_bits_retain(groups.memberships),
            Group::from_bits_retain(groups.filter),
        )
    }
}

/// Physics collider component for a game object.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "53a682cb-a207-4c4c-8795-63f38351c7ef"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Collider")]
#[serde(default)]
pub struct ComponentCollider {
    /// Primitive shape used to build the collider.
    pub shape: ColliderShape,
    /// Surface friction coefficient.
    pub friction: f32,
    /// Density used to derive collider mass properties.
    pub density: f32,
    /// The groups the collider belongs to and interacts with, for contacts
    /// and for queries that filter by groups.
    #[serde(skip_serializing_if = "CollisionGroups::is_all")]
    pub groups: CollisionGroups,
}

impl Default for ComponentCollider {
    fn default() -> Self {
        Self {
            shape: ColliderShape::Sphere { radius: 1.0 },
            friction: 0.5,
            density: 100.0,
            groups: CollisionGroups::ALL,
        }
    }
}

impl Component for ComponentCollider {
    /// Physics leaves a disabled collider out of the simulation.
    fn can_disable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{ColliderShape, CollisionGroups, ComponentCollider};
    use crate::reflect::{ReflectedType, TypeInfo};
    use crate::utils::TypeUuid;

    #[test]
    fn collider_shape_registers_enum_type_info() {
        let mut registry = crate::reflect::type_registry::TypeRegistry {
            types: Default::default(),
            identities: Default::default(),
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

    #[test]
    fn default_groups_serialize_as_before_groups_existed() {
        let saved = serde_json::to_value(ComponentCollider::default()).unwrap();
        assert!(saved.get("groups").is_none(), "{saved}");

        // A collider saved before groups existed loads with every group.
        let legacy: ComponentCollider =
            serde_json::from_str(r#"{"density": 100.0, "friction": 0.8}"#).unwrap();
        assert_eq!(legacy.groups, CollisionGroups::ALL);
    }

    #[test]
    fn custom_groups_round_trip() {
        let collider = ComponentCollider {
            groups: CollisionGroups::new(0b1, 0b110),
            ..Default::default()
        };
        let saved = serde_json::to_string(&collider).unwrap();
        let loaded: ComponentCollider = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.groups, collider.groups);

        // A partly written mask keeps the default for the other.
        let partial: ComponentCollider =
            serde_json::from_str(r#"{"groups": {"memberships": 1}}"#).unwrap();
        assert_eq!(partial.groups, CollisionGroups::new(1, u32::MAX));
    }
}
