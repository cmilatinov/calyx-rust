use super::*;
use crate as calyx_engine;
use crate::math::Transform;
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{Reflect, ReflectedType};
use crate::utils::TypeUuid;
use nalgebra_glm::Vec3;

fn transform_registry() -> TypeRegistry {
    let mut reg = TypeRegistry {
        types: Default::default(),
        identities: Default::default(),
    };
    Transform::register(&mut reg);
    reg
}

fn transform_struct_info(reg: &TypeRegistry) -> &StructInfo {
    match reg.type_info::<Transform>().unwrap() {
        TypeInfo::Struct(s) => s,
        _ => panic!("expected StructInfo"),
    }
}

#[derive(TypeUuid, Reflect)]
#[allow(dead_code)]
enum ExampleEnum {
    Unit,
    Named { value: f32 },
    Tuple(Vec3),
}

#[test]
fn enum_type_info_stores_variants() {
    let mut reg = TypeRegistry {
        types: Default::default(),
        identities: Default::default(),
    };
    ExampleEnum::register(&mut reg);

    let TypeInfo::Enum(info) = reg.type_info::<ExampleEnum>().unwrap() else {
        panic!("expected enum info");
    };
    assert!(info.variant("Unit").is_some());
    assert_eq!(info.variant("Named").unwrap().fields[0].name, Some("value"));
    assert_eq!(info.variant("Tuple").unwrap().fields[0].name, None);
    assert!(!info.is_unit_only());
}

#[derive(Debug, PartialEq, TypeUuid, Reflect)]
#[allow(dead_code)]
enum Mode {
    First,
    #[reflect_attr(name = "Second Mode")]
    Second,
    Data(f32),
}

#[derive(TypeUuid, Reflect)]
#[allow(dead_code)]
enum Quality {
    Low,
    High,
}

#[test]
fn enum_values_read_and_set_their_variant_through_reflection() {
    let mut mode = Mode::First;
    let reflected = mode.as_reflect_mut();
    assert_eq!(reflected.as_enum().unwrap().variant_name(), "First");
    assert!(reflected.as_enum_mut().unwrap().set_unit_variant("Second"));
    assert_eq!(mode, Mode::Second);
    assert_eq!(
        mode.as_reflect().as_enum().unwrap().variant_name(),
        "Second"
    );

    // A data-carrying variant reads, but cannot be set by name.
    let mut data = Mode::Data(1.5);
    assert_eq!(data.as_reflect().as_enum().unwrap().variant_name(), "Data");
    let reflected = data.as_reflect_mut().as_enum_mut().unwrap();
    assert!(!reflected.set_unit_variant("Data"));
    assert!(!reflected.set_unit_variant("Missing"));
    assert_eq!(data, Mode::Data(1.5));

    // A struct is not an enum.
    assert!(Transform::default().as_reflect().as_enum().is_none());
}

#[test]
fn enum_variants_carry_their_attributes() {
    let mut reg = TypeRegistry {
        types: Default::default(),
        identities: Default::default(),
    };
    Mode::register(&mut reg);
    Quality::register(&mut reg);

    let TypeInfo::Enum(mode) = reg.type_info::<Mode>().unwrap() else {
        panic!("expected enum info");
    };
    assert!(matches!(
        mode.variant("Second").unwrap().attr("name"),
        Some(AttributeValue::String("Second Mode"))
    ));
    assert!(mode.variant("First").unwrap().attr("name").is_none());
    assert!(mode.variant("First").unwrap().is_unit());
    assert!(!mode.variant("Data").unwrap().is_unit());
    assert!(!mode.is_unit_only());

    let TypeInfo::Enum(quality) = reg.type_info::<Quality>().unwrap() else {
        panic!("expected enum info");
    };
    assert!(quality.is_unit_only());
    let names = quality.variants.iter().map(|v| v.name).collect::<Vec<_>>();
    assert_eq!(names, ["Low", "High"]);
}

#[derive(TypeUuid, Reflect)]
#[allow(dead_code)]
struct ManyFields {
    zeta: f32,
    alpha: f32,
    mid: f32,
    beta: f32,
    omega: f32,
    gamma: f32,
    #[reflect_skip]
    skipped: f32,
    delta: f32,
    epsilon: f32,
    kappa: f32,
    lambda: f32,
    iota: f32,
    theta: f32,
}

#[test]
fn struct_fields_keep_declaration_order() {
    let mut reg = TypeRegistry {
        types: Default::default(),
        identities: Default::default(),
    };
    ManyFields::register(&mut reg);
    let TypeInfo::Struct(info) = reg.type_info::<ManyFields>().unwrap() else {
        panic!("expected StructInfo");
    };

    let names = info.fields.values().map(|f| f.name).collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "zeta", "alpha", "mid", "beta", "omega", "gamma", "delta", "epsilon", "kappa",
            "lambda", "iota", "theta"
        ]
    );
    assert_eq!(info.field("lambda").unwrap().name, "lambda");
}

#[derive(TypeUuid, Reflect)]
#[allow(dead_code)]
struct Grouped {
    #[reflect_attr(group = "Physics", min = 0.0)]
    mass: f32,
    label: f32,
}

#[test]
fn group_attribute_names_the_field_group() {
    let mut reg = TypeRegistry {
        types: Default::default(),
        identities: Default::default(),
    };
    Grouped::register(&mut reg);
    let TypeInfo::Struct(info) = reg.type_info::<Grouped>().unwrap() else {
        panic!("expected StructInfo");
    };

    assert_eq!(info.field("mass").unwrap().group(), Some("Physics"));
    assert!(info.field("mass").unwrap().attr("min").is_some());
    assert_eq!(info.field("label").unwrap().group(), None);
}

#[derive(TypeUuid, Reflect)]
#[allow(dead_code)]
struct Signed {
    #[reflect_attr(min = -1.5, max = 1.5)]
    balance: f32,
    #[reflect_attr(min = -3)]
    offset: i32,
}

#[test]
fn negative_attribute_literals_keep_their_sign() {
    let mut reg = TypeRegistry {
        types: Default::default(),
        identities: Default::default(),
    };
    Signed::register(&mut reg);
    let TypeInfo::Struct(info) = reg.type_info::<Signed>().unwrap() else {
        panic!("expected StructInfo");
    };

    let attr = |field: &str, name: &str| info.field(field).unwrap().attr(name);
    assert!(matches!(
        attr("balance", "min"),
        Some(AttributeValue::Float(-1.5))
    ));
    assert!(matches!(
        attr("balance", "max"),
        Some(AttributeValue::Float(1.5))
    ));
    assert!(matches!(
        attr("offset", "min"),
        Some(AttributeValue::Integer(-3))
    ));
}

#[test]
fn field_lookup() {
    let reg = transform_registry();
    let info = transform_struct_info(&reg);
    assert!(info.field("position").is_some());
    assert!(info.field("missing").is_none());
}

#[test]
fn all_expected_fields_present() {
    let reg = transform_registry();
    let info = transform_struct_info(&reg);
    for name in ["position", "rotation", "scale"] {
        assert!(info.field(name).is_some(), "missing field: {name}");
    }
}

#[test]
fn field_getter_returns_correct_value() {
    let reg = transform_registry();
    let info = transform_struct_info(&reg);
    let field = info.field("position").unwrap();

    let t = Transform::from_xyz(1.0, 2.0, 3.0);
    let pos = field.get::<Vec3>(t.as_reflect()).unwrap();
    assert_eq!(*pos, Vec3::new(1.0, 2.0, 3.0));
}

#[test]
fn field_getter_wrong_type_returns_none() {
    let reg = transform_registry();
    let info = transform_struct_info(&reg);
    let field = info.field("position").unwrap();

    let t = Transform::default();
    assert!(field.get::<f32>(t.as_reflect()).is_none());
}

#[test]
fn field_getter_mut_allows_mutation() {
    let reg = transform_registry();
    let info = transform_struct_info(&reg);
    let field = info.field("position").unwrap();

    let mut t = Transform::from_xyz(0.0, 0.0, 0.0);
    let pos = field.get_mut::<Vec3>(t.as_reflect_mut()).unwrap();
    *pos = Vec3::new(9.0, 8.0, 7.0);
    assert_eq!(t.position, Vec3::new(9.0, 8.0, 7.0));
}

#[test]
fn field_setter_updates_value() {
    let reg = transform_registry();
    let info = transform_struct_info(&reg);
    let field = info.field("position").unwrap();

    let mut t = Transform::default();
    field
        .set(t.as_reflect_mut(), Vec3::new(5.0, 6.0, 7.0))
        .unwrap();
    assert_eq!(t.position, Vec3::new(5.0, 6.0, 7.0));
}

#[test]
fn field_setter_wrong_type_returns_none() {
    let reg = transform_registry();
    let info = transform_struct_info(&reg);
    let field = info.field("position").unwrap();

    let mut t = Transform::default();
    assert!(field.set(t.as_reflect_mut(), 42.0f32).is_none());
}
