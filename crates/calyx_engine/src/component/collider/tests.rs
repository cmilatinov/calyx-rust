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
