use crate::reflect::Reflect;
use crate::utils::TypeUuid;
use std::any::TypeId;
use std::collections::HashMap;
use uuid::Uuid;

/// Getter used to read a reflected field by shared reference.
pub type FieldGetter = fn(&dyn Reflect) -> Option<&dyn Reflect>;
/// Getter used to read a reflected field by mutable reference.
pub type FieldGetterMut = fn(&mut dyn Reflect) -> Option<&mut dyn Reflect>;
/// Setter used to assign a reflected field.
pub type FieldSetter = fn(&mut dyn Reflect, Box<dyn Reflect>) -> Option<()>;

/// Supported attribute payloads stored on reflected types and fields.
#[derive(Copy, Clone)]
#[repr(C)]
pub enum AttributeValue {
    /// Marker attribute with no payload.
    None,
    /// Floating-point payload.
    Float(f64),
    /// Integer payload.
    Integer(isize),
    /// String payload.
    String(&'static str),
}
/// Attribute map keyed by attribute name.
pub type AttributeMap = HashMap<&'static str, AttributeValue>;

/// Allows you to introspect structs (and potentially other types in the future)
/// at runtime by listing, querying and setting their fields
/// Runtime metadata for a reflected type.
#[repr(C)]
pub enum TypeInfo {
    /// Struct-like type metadata.
    Struct(StructInfo),
    /// Enum metadata.
    Enum(EnumInfo),
    /// List metadata.
    List(ListInfo),
    /// Option metadata.
    Option(OptionInfo),
    /// Map metadata.
    Map(MapInfo),
    /// Placeholder for registered types without detailed metadata.
    None,
}

/// Minimal type descriptor embedded inside higher-level metadata.
#[repr(C)]
pub struct TypeDescriptor {
    /// Rust `TypeId` for the described type.
    pub type_id: TypeId,
    /// Stable UUID for the described type.
    pub type_uuid: Uuid,
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
}

/// Reflection metadata for a struct-like type.
#[repr(C)]
pub struct StructInfo {
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Rust `TypeId`.
    pub type_id: TypeId,
    /// Attributes attached to the type.
    pub attrs: AttributeMap,
    /// Named fields keyed by their Rust field name.
    pub fields: HashMap<&'static str, NamedField>,
}

impl StructInfo {
    /// Looks up a field by name.
    pub fn field(&self, name: &str) -> Option<&NamedField> {
        self.fields.get(name)
    }

    /// Looks up a type attribute by name.
    pub fn attr(&self, name: &str) -> Option<AttributeValue> {
        self.attrs.get(name).copied()
    }
}

/// Reflection metadata for an enum type.
#[repr(C)]
pub struct EnumInfo {
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Rust `TypeId`.
    pub type_id: TypeId,
    /// Attributes attached to the enum.
    pub attrs: AttributeMap,
    /// Enum variants in declaration order.
    pub variants: Vec<EnumVariantInfo>,
}

impl EnumInfo {
    /// Looks up a variant by name.
    pub fn variant(&self, name: &str) -> Option<&EnumVariantInfo> {
        self.variants.iter().find(|variant| variant.name == name)
    }

    /// Looks up an enum attribute by name.
    pub fn attr(&self, name: &str) -> Option<AttributeValue> {
        self.attrs.get(name).copied()
    }
}

/// Reflection metadata for one enum variant.
#[repr(C)]
pub struct EnumVariantInfo {
    /// Variant name.
    pub name: &'static str,
    /// Variant fields in declaration order.
    pub fields: Vec<EnumVariantFieldInfo>,
}

/// Reflection metadata for one enum variant field.
#[repr(C)]
pub struct EnumVariantFieldInfo {
    /// Optional field name for named variants.
    pub name: Option<&'static str>,
    /// Rust `TypeId` of the field.
    pub type_id: TypeId,
    /// Stable UUID of the field type.
    pub type_uuid: Uuid,
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
}

/// Reflection metadata for a list-like container.
#[repr(C)]
pub struct ListInfo {
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Rust `TypeId`.
    pub type_id: TypeId,
    /// Descriptor for the element type.
    pub element: TypeDescriptor,
}

/// Reflection metadata for an `Option<T>`-like type.
#[repr(C)]
pub struct OptionInfo {
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Rust `TypeId`.
    pub type_id: TypeId,
    /// Descriptor for the wrapped value type.
    pub value: TypeDescriptor,
}

/// Reflection metadata for a map-like container.
#[repr(C)]
pub struct MapInfo {
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Rust `TypeId`.
    pub type_id: TypeId,
    /// Descriptor for the key type.
    pub key: TypeDescriptor,
    /// Descriptor for the value type.
    pub value: TypeDescriptor,
}

/// Reflection metadata for one named struct field.
#[repr(C)]
pub struct NamedField {
    /// Rust field name.
    pub name: &'static str,
    /// Rust `TypeId` of the field.
    pub type_id: TypeId,
    /// Stable UUID of the field type.
    pub type_uuid: Uuid,
    /// Fully qualified Rust type name.
    pub type_name: &'static str,
    /// Field attributes keyed by attribute name.
    pub attrs: AttributeMap,
    /// Doc comment captured from the source field, when available.
    pub doc: Option<&'static str>,
    /// Shared field getter.
    pub getter: FieldGetter,
    /// Mutable field getter.
    pub getter_mut: FieldGetterMut,
    /// Field setter.
    pub setter: FieldSetter,
}

impl NamedField {
    /// Reads the field as a concrete `T`.
    pub fn get<'a, T: 'static + Reflect + TypeUuid>(
        &'a self,
        instance: &'a dyn Reflect,
    ) -> Option<&'a T> {
        let value = (self.getter)(instance)?;
        value.downcast_ref::<T>()
    }
    /// Reads the field mutably as a concrete `T`.
    pub fn get_mut<'a, T: 'static + Reflect + TypeUuid>(
        &'a self,
        instance: &'a mut dyn Reflect,
    ) -> Option<&'a mut T> {
        let value = (self.getter_mut)(instance)?;
        value.downcast_mut::<T>()
    }
    /// Reads the field as a reflected trait object.
    pub fn get_reflect<'a>(&'a self, instance: &'a dyn Reflect) -> Option<&'a dyn Reflect> {
        (self.getter)(instance)
    }
    /// Reads the field mutably as a reflected trait object.
    pub fn get_reflect_mut<'a>(
        &'a self,
        instance: &'a mut dyn Reflect,
    ) -> Option<&'a mut dyn Reflect> {
        (self.getter_mut)(instance)
    }
    /// Assigns a concrete reflected value to the field.
    pub fn set<T: Reflect + 'static>(&self, instance: &mut dyn Reflect, value: T) -> Option<()> {
        (self.setter)(instance, Box::new(value))
    }
    /// Looks up a field attribute by name.
    pub fn attr(&self, name: &str) -> Option<AttributeValue> {
        self.attrs.get(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as engine;
    use crate::math::Transform;
    use crate::reflect::type_registry::TypeRegistry;
    use crate::reflect::{Reflect, ReflectedType};
    use crate::utils::TypeUuid;
    use nalgebra_glm::Vec3;

    fn transform_registry() -> TypeRegistry {
        let mut reg = TypeRegistry {
            types: Default::default(),
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
        };
        ExampleEnum::register(&mut reg);

        let TypeInfo::Enum(info) = reg.type_info::<ExampleEnum>().unwrap() else {
            panic!("expected enum info");
        };
        assert!(info.variant("Unit").is_some());
        assert_eq!(info.variant("Named").unwrap().fields[0].name, Some("value"));
        assert_eq!(info.variant("Tuple").unwrap().fields[0].name, None);
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
}
